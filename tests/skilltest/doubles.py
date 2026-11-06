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
import traceback
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from email.utils import format_datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, NamedTuple
from urllib.parse import unquote, urlsplit

from cases import Case, MachineState, Moment, instant

#: A heater command in the G-code, and the temperature it sets.
HEATER = re.compile(rb"^M(?P<code>104|109|140|190)\s[^;\n]*?S(?P<value>\d+(?:\.\d+)?)", re.M)
#: The most of a request's body a double reads: nothing a server sends it is longer.
MAX_BODY_BYTES = 1_048_576
#: The longest a stopped camera holds a fetch before giving up on it.
HELD_FETCH_S = 600


class HeaterTarget(NamedTuple):
    """One temperature a G-code asks for, and the byte it takes effect at."""

    position: int
    target_c: float


@dataclass
class Heaters:
    """The temperatures a G-code asks for, in file order."""

    tool: list[HeaterTarget] = field(default_factory=list)
    bed: list[HeaterTarget] = field(default_factory=list)

    @classmethod
    def of(cls, gcode: bytes | None) -> Heaters:
        """What one G-code's heater commands ask for; nothing for a G-code not at hand."""
        heaters = cls()
        for found in HEATER.finditer(gcode or b""):
            target = heaters.tool if found["code"] in {b"104", b"109"} else heaters.bed
            target.append(HeaterTarget(found.start(), float(found["value"])))
        return heaters

    @staticmethod
    def at(commands: list[HeaterTarget], position: int) -> float:
        """The target standing at one file position."""
        before = bisect.bisect_right(commands, HeaterTarget(position, float("inf")))
        return commands[before - 1].target_c if before else 0.0


def numbered(jpeg: bytes, number: int) -> bytes:
    """A JPEG with one comment segment more, right after its start marker."""
    comment = f"frame {number}".encode()
    return jpeg[:2] + b"\xff\xfe" + (len(comment) + 2).to_bytes(2, "big") + comment + jpeg[2:]


#: The one request the Obico adapter makes of Obico's API.
ACKNOWLEDGEMENT = re.compile(r"/api/v1/printers/\d+/acknowledge_alert/")


class FrameDue(NamedTuple):
    """The frame a fetch answers."""

    name: str
    captured: datetime


class CameraFrame(NamedTuple):
    """The bytes a camera answers a fetch with."""

    picture: bytes
    captured: datetime


@dataclass
class Timeline:
    """The replay's current time: which event it stands at, and which frame was last answered.

    It is the server's clock too, through the file `clock` names: a print replayed in
    minutes has to read as the hours it took, or every instant the agent reads
    contradicts the position beside it. The clock runs at real speed from each event's
    recorded time and a look's next frame moves it on to when that frame was captured,
    so every counter a turn reads comes from one instant. It stops just short of the
    next event, and nothing a turn can read passes that moment.
    """

    case: Case
    #: The bytes of one frame, by its file name.
    picture: Callable[[str], bytes]
    #: The file the server's clock is moved by.
    clock: Path
    gcode: bytes | None = None
    index: int = 0
    #: The frame last answered; a later fetch answers the next one before the next event.
    cursor: int = -1
    #: A fetch owed the event's own frame, rather than the next one.
    owed_event_frame: bool = True
    at_event_frame: int = -1
    moved_at_exit: bool = False
    finished: bool = False
    paused: bool = False
    #: Whether a look has been taken at this event, which is when time is first read.
    looked: bool = False
    fetches: list[str] = field(default_factory=list)
    frame_times: list[datetime] = field(default_factory=list)
    offset: timedelta = timedelta()
    ceiling: datetime | None = None
    #: The print's clock never goes back: the server reads that as a later job.
    longest_print_time_s: int = 0
    stopped: threading.Event = field(default_factory=threading.Event)
    lock: threading.Lock = field(default_factory=threading.Lock)
    heaters: Heaters = field(default_factory=Heaters)

    def __post_init__(self) -> None:
        """Read the heater commands and capture times once, and stand at the first event."""
        self.heaters = Heaters.of(self.gcode)
        self.frame_times = self.case.frame_times()
        self.stand_at(0)

    @property
    def moment(self) -> Moment:
        """The event the replay stands at."""
        return self.case.moments[self.index]

    def now(self) -> datetime:
        """The replay's current time, which is what the server's clock answers."""
        moved = datetime.now(UTC) + self.offset
        return min(moved, self.ceiling) if self.ceiling else moved

    def _set_clock(self, when: datetime) -> None:
        """Move the clock to an instant, and tell the server, which reads the file each time."""
        self.offset = when - datetime.now(UTC)
        ceiling = f" {int(self.ceiling.timestamp() * 1000)}" if self.ceiling else ""
        written = self.clock.with_suffix(".new")
        written.write_text(f"{int(self.offset.total_seconds() * 1000)}{ceiling}\n", "ascii")
        written.replace(self.clock)

    def stand_at(self, index: int) -> None:
        """Stand at one event: its recorded time, and its frame next for a fetch to answer."""
        self.index = index
        self.looked = False
        moment = self.case.moments[index]
        following = self.case.moments[index + 1 :]
        self.ceiling = instant(following[0].at) - timedelta(seconds=1) if following else None
        self._set_clock(instant(moment.at))
        at = self.case.frame_at_or_before(moment.at, moment.event_image)
        if moment.is_alert:
            # The alert carries its own picture, so a look answers what came after.
            self.cursor = max(self.cursor, at)
            self.owed_event_frame = False
        else:
            self.owed_event_frame = True
            self.at_event_frame = at
        self.paused = moment.printer_state is MachineState.PAUSED or (
            self.paused and moment.printer_state is MachineState.PRINTING
        )

    def harness_started(self) -> None:
        """A second run within one turn, the engine's own re-prompt, is still that turn's."""
        with self.lock:
            if self.moved_at_exit and self.index > 0:
                self.index -= 1
                self.owed_event_frame = False
            self.moved_at_exit = False

    def _window_end(self) -> int:
        """The newest frame a look of this turn may answer: nothing newer exists yet.

        A frame captured at or after the next event's moment has not been captured,
        whatever its label: a look that has run out answers the latest earlier frame
        again, with its capture time. After the last event a look reads as far as the
        recorded turn's looks did.
        """
        following = self.case.moments[self.index + 1 :]
        if following:
            upcoming = following[0]
            before = instant(upcoming.at)
            names = [frame.file for frame in self.case.frames]
            own = names.index(upcoming.event_image) if upcoming.event_image in names else None
            earlier = [
                index
                for index, when in enumerate(self.frame_times)
                if when < before and (own is None or index < own)
            ]
            return max([*earlier, self.cursor])
        names = [frame.file for frame in self.case.frames]
        reached = [names.index(look) for look in self.moment.looks if look in names]
        return max([*reached, self.cursor])

    def next_fetch(self) -> FrameDue | None:
        """The frame this fetch answers and when it was captured: the event's, then the next."""
        with self.lock:
            if self.finished:
                return None
            if self.owed_event_frame:
                self.cursor = max(self.cursor, self.at_event_frame)
                self.owed_event_frame = False
            else:
                self.looked = True
                if self.cursor < self._window_end():
                    self.cursor += 1
                    when = self.frame_times[self.cursor]
                    if when > self.now():
                        self._set_clock(when)
            shown = max(self.cursor, 0)
            name = self.case.frames[shown].file
            self.fetches.append(name)
            return FrameDue(name, self.frame_times[shown])

    def camera_answer(self) -> CameraFrame | None:
        """The bytes this fetch answers, and when they were captured.

        A real camera's two frames of one still scene never match byte for byte, so
        each answer carries its fetch's number in a comment segment, which changes no
        pixel. When it was captured is what says two fetches saw one frame.
        """
        answered = self.next_fetch()
        if answered is None:
            return None
        return CameraFrame(
            numbered(self.picture(answered.name), len(self.fetches)), answered.captured
        )

    def position(self) -> int:
        """The job's file position at the replay's current time, as the recording has it.

        The event's own until a look has been taken, and short of the next event's.
        """
        low = self.moment.file_position_bytes
        if not self.looked:
            return low
        following = self.case.moments[self.index + 1 :]
        high = following[0].file_position_bytes - 1 if following else self.case.gcode_size - 1
        return max(low, min(self.case.position_at(self.now()), high))

    def _print_time(self) -> int:
        """How long the job has been running: now, less when the recording says it started."""
        running = int((self.now() - self.case.job_started()).total_seconds())
        self.longest_print_time_s = max(self.longest_print_time_s, running)
        return self.longest_print_time_s

    def job_body(self) -> tuple[int, dict[str, Any]]:
        """What `GET /api/job` answers, which the real adapter reads as it reads OctoPrint's."""
        with self.lock:
            # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
            if not self.moment.printer_state.answers:
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
                    "printTime": self._print_time(),
                    "printTimeLeft": None,
                },
                "state": "Paused" if self.paused else "Printing",
                "error": "",
            }

    def printer_body(self) -> tuple[int, dict[str, Any]]:
        """What `GET /api/printer` answers, which the real adapter reads as it reads OctoPrint's."""
        with self.lock:
            # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
            if not self.moment.printer_state.answers:
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
        self,
        status: int,
        body: bytes = b"",
        content_type: str = "application/json",
        taken: datetime | None = None,
    ) -> None:
        """Send one whole response, saying when a picture in it was taken."""
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        if taken is not None:
            self.send_header("Last-Modified", format_datetime(taken, usegmt=True))
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def answer_json(self, status: int, document: object) -> None:
        """Send one JSON response."""
        self.answer(status, json.dumps(document).encode())

    def body(self) -> dict[str, Any] | None:
        """The request's JSON object, empty for no body; `None` once it is refused as malformed."""
        declared = self.headers.get("Content-Length") or "0"
        if not (declared.isascii() and declared.isdigit()):
            self.answer_json(400, {"error": "Malformed Content-Length"})
            return None
        data = self.rfile.read(min(int(declared), MAX_BODY_BYTES))
        try:
            parsed = json.loads(data) if data else {}
        except ValueError:
            parsed = None
        if not isinstance(parsed, dict):
            self.answer_json(400, {"error": "Malformed JSON body"})
            return None
        return parsed


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
            # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
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
            if body is None:
                return
            path = urlsplit(self.path).path
            if path == "/api/login":
                self.answer_json(200, {"name": "operator", "session": "replay"})
                return
            match path, body:
                case "/api/job", {"command": "pause", "action": "pause"}:
                    timeline.set_paused(paused=True)
                case "/api/job", {"command": "pause", "action": "resume"}:
                    timeline.set_paused(paused=False)
                case "/api/job", _:
                    self.answer_json(400, {"error": "Unknown job command"})
                    return
                case "/api/printer/command", {"commands": list()} | {"command": str()}:
                    pass
                case "/api/printer/printhead", {"command": "feedrate" | "jog" | "home"}:
                    pass
                case "/api/printer/tool", {"command": "flowrate" | "target"}:
                    pass
                case "/api/printer/bed", {"command": "target"}:
                    pass
                case (
                    "/api/printer/command"
                    | "/api/printer/printhead"
                    | "/api/printer/tool"
                    | "/api/printer/bed",
                    _,
                ):
                    self.answer_json(400, {"error": "Unknown printer command"})
                    return
                case _:
                    self.answer_json(404, {"error": "Not found"})
                    return
            self.answer(204)

    return _serving(Handler)


def camera(timeline: Timeline) -> ThreadingHTTPServer:
    """The loopback camera, answering each fetch with the frame the timeline stands at."""

    class Handler(_Quiet):
        def do_GET(self) -> None:
            answered = timeline.camera_answer()
            if answered is None:
                # The last event's turn is over and the server is being stopped;
                # a check that fired meanwhile starts no turn, because this never
                # answers it.
                timeline.stopped.wait(HELD_FETCH_S)
                self.answer(503)
                return
            self.answer(200, answered[0], "image/jpeg", answered[1])

    return _serving(Handler)


def obico(timeline: Timeline) -> ThreadingHTTPServer:
    """The Obico double: its health, its acknowledgements and its alert images."""

    class Handler(_Quiet):
        def do_GET(self) -> None:
            path = urlsplit(self.path).path
            if path.startswith("/media/"):
                name = unquote(path.rsplit("/", 1)[1])
                if name not in {moment.event_image for moment in timeline.case.moments}:
                    self.answer_json(404, {"error": "no such image"})
                    return
                self.answer(200, timeline.picture(name), "image/jpeg", timeline.now())
                return
            self.answer_json(200, {"status": "ok"})

        def do_POST(self) -> None:
            if self.body() is None:
                return
            if not ACKNOWLEDGEMENT.fullmatch(urlsplit(self.path).path):
                self.answer_json(404, {"error": "Not found"})
                return
            self.answer_json(200, {"status": "ok"})

    return _serving(Handler)


def control(
    started: Callable[[], str], exited: Callable[[], None], failed: Callable[[str], None]
) -> ThreadingHTTPServer:
    """Where the launcher reports the harness's start and its exit, each answered when done with."""

    class Handler(_Quiet):
        def do_POST(self) -> None:
            self.body()
            try:
                match self.path:
                    case "/started":
                        self.answer(200, started().encode(), "text/plain")
                    case "/exited":
                        exited()
                        self.answer(200)
                    case _:
                        self.answer(404)
            except Exception:
                failed(traceback.format_exc())
                self.answer(500)

    return _serving(Handler)


def address(server: ThreadingHTTPServer) -> str:
    """The URL one double answers at."""
    return _url(server)
