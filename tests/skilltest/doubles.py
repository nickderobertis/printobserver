"""What a replay doubles, because no test host has the real thing, on real loopback sockets.

The OctoPrint stand-in serves the case's G-code and moves the job's file position
to each event's; the camera answers each fetch with the case's latest frame
captured at or before the replay's current time; the Obico double serves its
health, takes acknowledgements and hands over the alert images it delivers. All
of them read one [`Timeline`], which is what "the replay's current time" is.

# Why the timeline moves when the harness process exits

The server's scheduled check fires on its own clock, whenever nothing holds the
print. So the moment the turn's harness exits, before the server can release the
print, the timeline stands at the next event: a check that follows always reads
that event's job, position and frame, and there is no window in which a replay
racing the server could hand it the last one's. The launcher in front of the
harness (`printobserver-server-turn-launcher`) reports that exit synchronously.
"""

from __future__ import annotations

import bisect
import json
import re
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import unquote, urlsplit

from cases import Case, Moment, elapsed_seconds

#: A heater command in the G-code, and the temperature it sets.
HEATER = re.compile(rb"^M(?P<code>104|109|140|190)\s[^;\n]*?S(?P<value>\d+(?:\.\d+)?)", re.M)
#: The printer states a read of the machine answers normally.
ANSWERING = {"printing": "Printing", "paused": "Paused"}
#: The longest a stopped camera holds a fetch before giving up on it.
HELD_FETCH_S = 600


@dataclass
class Heaters:
    """The temperatures a G-code asks for, by the byte they take effect at."""

    tool: list[tuple[int, float]] = field(default_factory=list)
    bed: list[tuple[int, float]] = field(default_factory=list)

    @classmethod
    def of(cls, gcode: bytes | None) -> Heaters:
        """What one G-code's heater commands ask for; nothing for a G-code not at hand."""
        heaters = cls()
        for found in HEATER.finditer(gcode or b""):
            target = heaters.tool if found["code"] in {b"104", b"109"} else heaters.bed
            target.append((found.start(), float(found["value"])))
        return heaters

    @staticmethod
    def at(commands: list[tuple[int, float]], position: int) -> float:
        """The target standing at one file position."""
        before = bisect.bisect_right(commands, (position, float("inf")))
        return commands[before - 1][1] if before else 0.0


def numbered(jpeg: bytes, number: int) -> bytes:
    """A JPEG with one comment segment more, right after its start marker."""
    comment = f"frame {number}".encode()
    return jpeg[:2] + b"\xff\xfe" + (len(comment) + 2).to_bytes(2, "big") + comment + jpeg[2:]


def print_time_s(case: Case, at: str, position: int) -> int:
    """How long the job had been running at a time and position, on the print's own clock.

    From the case's first camera frame where its frames carry their times, and
    otherwise from the rate between the first and last event. It never goes back,
    which is how the server tells this job from a later one of the same file.
    """
    if case.has_frame_times:
        return max(0, elapsed_seconds(case.frames[0].captured, at))
    first, last = case.moments[0], case.moments[-1]
    span = elapsed_seconds(first.at, last.at)
    moved = last.file_position_bytes - first.file_position_bytes
    return position * span // moved if moved > 0 else 0


@dataclass
class Timeline:
    """The replay's current time: which event it stands at, and which frame was last answered."""

    case: Case
    #: The bytes of one frame, by its file name.
    picture: Callable[[str], bytes]
    #: The epoch second the print started, which an alert names.
    started_epoch: float
    gcode: bytes | None = None
    index: int = 0
    #: The frame last answered; every later fetch answers the next.
    cursor: int = -1
    #: A fetch owed the event's own frame, rather than the next one.
    owed_event_frame: bool = True
    at_event_frame: int = -1
    #: The frame the event stands at; a frame past it means time has moved on.
    event_cursor: int = -1
    moved_at_exit: bool = False
    finished: bool = False
    paused: bool = False
    fetches: list[str] = field(default_factory=list)
    #: Where the job was at each frame, and the real seconds looks have waited past the newest.
    positions: list[int] = field(default_factory=list)
    crept_s: float = 0.0
    fetched_at: float = 0.0
    stopped: threading.Event = field(default_factory=threading.Event)
    lock: threading.Lock = field(default_factory=threading.Lock)
    heaters: Heaters = field(default_factory=Heaters)

    def __post_init__(self) -> None:
        """Read the heater commands once, and stand at the first event."""
        self.heaters = Heaters.of(self.gcode)
        self.positions = self.case.frame_positions()
        self.stand_at(0)

    @property
    def moment(self) -> Moment:
        """The event the replay stands at."""
        return self.case.moments[self.index]

    def stand_at(self, index: int) -> None:
        """Stand at one event: its frame is the next a fetch answers."""
        self.index = index
        self.crept_s = 0.0
        moment = self.case.moments[index]
        at = self.case.frame_at_or_before(moment.at, moment.event_image)
        if moment.is_alert:
            # The alert carries its own picture, so a look answers what came after.
            self.cursor = max(self.cursor, at)
            self.owed_event_frame = False
        else:
            self.owed_event_frame = at > self.cursor
            self.at_event_frame = at
        self.event_cursor = max(self.cursor, at)
        self.paused = moment.printer_state == "paused" or (
            self.paused and moment.printer_state == "printing"
        )

    def harness_started(self) -> None:
        """A second run within one turn, the engine's own re-prompt, is still that turn's."""
        with self.lock:
            if self.moved_at_exit and self.owed_event_frame and self.index > 0:
                self.index -= 1
                self.owed_event_frame = False
            self.moved_at_exit = False

    def _window_end(self) -> int:
        """The last frame a look of this turn may answer: nothing newer exists yet.

        In replay time a frame captured after the next event's moment has not been
        captured, so a turn's looks stop at the last one before that event's frame.
        """
        following = self.case.moments[self.index + 1 :]
        if not following:
            return len(self.case.frames) - 1
        upcoming = following[0]
        return self.case.frame_at_or_before(upcoming.at, upcoming.event_image) - (
            0 if upcoming.is_alert else 1
        )

    def next_fetch(self) -> str | None:
        """The frame this fetch answers: the event's own, then the next before the next event."""
        with self.lock:
            if self.finished:
                return None
            frames = self.case.frames
            if self.owed_event_frame:
                self.cursor = self.at_event_frame
                self.owed_event_frame = False
                self.fetched_at = time.monotonic()
            elif self.cursor < self._window_end():
                self.cursor += 1
                self.fetched_at = time.monotonic()
            else:
                self.crept_s += time.monotonic() - self.fetched_at
                self.fetched_at = time.monotonic()
            name = frames[max(self.cursor, 0)].file
            self.fetches.append(name)
            return name

    def camera_answer(self) -> bytes | None:
        """The bytes this fetch answers, which no two fetches share.

        A real camera's two frames of one still scene never match byte for byte,
        and an agent that compares what the server records of them would read a
        frozen camera. So each answer carries its fetch's number in a comment
        segment, which changes no pixel.
        """
        name = self.next_fetch()
        if name is None:
            return None
        return numbered(self.picture(name), len(self.fetches))

    def position(self) -> int:
        """The job's file position now: the event's, moved on by the frames shown since.

        A look past the newest frame of the turn answers that frame again, and the
        job creeps on by the seconds it really waited, at the print's own rate.
        """
        moment = self.moment
        low = moment.file_position_bytes
        following = self.case.moments[self.index + 1 :]
        high = following[0].file_position_bytes - 1 if following else self.case.gcode_size - 1
        moved = low
        if self.cursor > self.event_cursor:
            moved = self.positions[self.cursor]
        return max(low, min(moved + int(self.crept_s * self._bytes_per_s()), high))

    def _bytes_per_s(self) -> float:
        first, last = self.case.moments[0], self.case.moments[-1]
        span = elapsed_seconds(first.at, last.at)
        return max(0, last.file_position_bytes - first.file_position_bytes) / span if span else 0.0

    def _print_time(self, position: int) -> int:
        at = self.moment.at
        if self.cursor > self.event_cursor and self.case.has_frame_times:
            at = self.case.frames[self.cursor].captured
        return print_time_s(self.case, at, position) + int(self.crept_s)

    def job_body(self) -> tuple[int, dict[str, Any]]:
        """What `GET /api/job` answers."""
        with self.lock:
            state = self.moment.printer_state
            if state not in ANSWERING:
                return 503, {"error": "the printer's serial port is gone"}
            position = self.position()
            size = self.case.gcode_size
            return 200, {
                "job": {
                    "file": {"name": self.case.gcode.name, "origin": "local", "size": size},
                    "estimatedPrintTime": None,
                    "filament": None,
                },
                "progress": {
                    "completion": position / size * 100,
                    "filepos": position,
                    "printTime": self._print_time(position),
                    "printTimeLeft": None,
                },
                "state": "Paused" if self.paused else "Printing",
                "error": "",
            }

    def printer_body(self) -> tuple[int, dict[str, Any]]:
        """What `GET /api/printer` answers."""
        with self.lock:
            if self.moment.printer_state not in ANSWERING:
                return 503, {"error": "the printer's serial port is gone"}
            position = self.position()
            tool = Heaters.at(self.heaters.tool, position)
            bed = Heaters.at(self.heaters.bed, position)
            return 200, {
                "state": {
                    "text": "Paused" if self.paused else "Printing",
                    "flags": {
                        "operational": True,
                        "printing": not self.paused,
                        "paused": self.paused,
                        "cancelling": False,
                        "error": False,
                        "closedOrError": False,
                    },
                },
                "temperature": {
                    "tool0": {"actual": tool, "target": tool, "offset": 0},
                    "bed": {"actual": bed, "target": bed, "offset": 0},
                },
            }

    def set_paused(self, *, paused: bool) -> None:
        """Take a job command the machine has obeyed."""
        with self.lock:
            self.paused = paused


class _Quiet(BaseHTTPRequestHandler):
    """A handler that keeps its requests out of the test's output."""

    protocol_version = "HTTP/1.1"

    def log_message(self, format: str, *args: object) -> None:
        """Say nothing: what happened is in the server's own record."""
        del format, args

    def answer(
        self, status: int, body: bytes = b"", content_type: str = "application/json"
    ) -> None:
        """Send one whole response."""
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def answer_json(self, status: int, document: object) -> None:
        """Send one JSON response."""
        self.answer(status, json.dumps(document).encode())

    def body(self) -> dict[str, Any]:
        """The request's JSON body, or an empty one."""
        length = int(self.headers.get("Content-Length") or 0)
        data = self.rfile.read(length) if length else b""
        try:
            parsed = json.loads(data) if data else {}
        except ValueError:
            return {}
        return parsed if isinstance(parsed, dict) else {}


class _Double(ThreadingHTTPServer):
    """A loopback server a stopped program's closed connection is nothing to."""

    def handle_error(self, request: object, client_address: object) -> None:
        """Say nothing: the server under test dropping a connection is ordinary."""
        del request, client_address


def _serving(handler: type[BaseHTTPRequestHandler]) -> ThreadingHTTPServer:
    server = _Double(("127.0.0.1", 0), handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def _url(server: ThreadingHTTPServer) -> str:
    return f"http://127.0.0.1:{server.server_address[1]}"


def octoprint(timeline: Timeline, api_key: str) -> ThreadingHTTPServer:
    """The OctoPrint stand-in, over one replay's timeline."""

    class Handler(_Quiet):
        def do_GET(self) -> None:
            path = urlsplit(self.path).path
            gcode_name = timeline.case.gcode.name
            if path.startswith("/downloads/files/local/"):
                held = timeline.gcode
                if held is None or unquote(path.rsplit("/", 1)[1]) != gcode_name:
                    self.answer(404)
                    return
                self.answer(200, held, "text/plain")
                return
            if self.headers.get("X-Api-Key") != api_key:
                self.answer_json(403, {"error": "Forbidden"})
                return
            match path:
                case "/api/job":
                    self.answer_json(*timeline.job_body())
                case "/api/printer":
                    self.answer_json(*timeline.printer_body())
                case "/api/connection":
                    status, _ = timeline.printer_body()
                    state = "Printing" if status == 200 else "Closed"
                    self.answer_json(200, {"current": {"state": state, "port": "/dev/ttyACM0"}})
                case "/api/files":
                    self.answer_json(200, {"files": []})
                case _ if path.startswith("/api/files/local/"):
                    self._file_facts(unquote(path.rsplit("/", 1)[1]))
                case _:
                    self.answer_json(404, {"error": "Not found"})

        def _file_facts(self, name: str) -> None:
            if name != timeline.case.gcode.name:
                self.answer_json(404, {"error": "Not found"})
                return
            host = self.headers.get("Host", "127.0.0.1")
            self.answer_json(
                200,
                {
                    "name": name,
                    "origin": "local",
                    "type": "machinecode",
                    "size": timeline.case.gcode_size,
                    "refs": {"download": f"http://{host}/downloads/files/local/{name}"},
                },
            )

        def do_POST(self) -> None:
            if self.headers.get("X-Api-Key") != api_key and self.path != "/api/login":
                self.answer_json(403, {"error": "Forbidden"})
                return
            body = self.body()
            path = urlsplit(self.path).path
            if path == "/api/login":
                self.answer_json(200, {"name": "operator", "session": "replay"})
                return
            if path == "/api/job" and body.get("command") == "pause":
                timeline.set_paused(paused=body.get("action") != "resume")
            self.answer(204)

    return _serving(Handler)


def camera(timeline: Timeline) -> ThreadingHTTPServer:
    """The loopback camera, answering each fetch with the frame the timeline stands at."""

    class Handler(_Quiet):
        def do_GET(self) -> None:
            picture = timeline.camera_answer()
            if picture is None:
                # The last event's turn is over and the server is being stopped;
                # a check that fired meanwhile starts no turn, because this never
                # answers it.
                timeline.stopped.wait(HELD_FETCH_S)
                self.answer(503)
                return
            self.answer(200, picture, "image/jpeg")

    return _serving(Handler)


def obico(timeline: Timeline) -> ThreadingHTTPServer:
    """The Obico double: its health, its acknowledgements and its alert images."""

    class Handler(_Quiet):
        def do_GET(self) -> None:
            path = urlsplit(self.path).path
            if path.startswith("/media/"):
                self.answer(200, timeline.picture(unquote(path.rsplit("/", 1)[1])), "image/jpeg")
                return
            self.answer_json(200, {"status": "ok"})

        def do_POST(self) -> None:
            self.body()
            self.answer_json(200, {"status": "ok"})

    return _serving(Handler)


def control(started: Callable[[], str], exited: Callable[[], None]) -> ThreadingHTTPServer:
    """Where the launcher reports the harness's start and its exit, each answered when done with."""

    class Handler(_Quiet):
        def do_POST(self) -> None:
            self.body()
            if self.path == "/started":
                self.answer(200, started().encode(), "text/plain")
                return
            exited()
            self.answer(200)

    return _serving(Handler)


def address(server: ThreadingHTTPServer) -> str:
    """The URL one double answers at."""
    return _url(server)
