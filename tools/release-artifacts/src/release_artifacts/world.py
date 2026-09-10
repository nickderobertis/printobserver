"""A real supervisor to prove an installed client against.

A smoke check that reached no server would prove nothing about the artifact it
runs from, so each of the three runs against a **real `printobserver server`**:
the program this repository builds, started under a configuration this writes,
with a print and an image in its store to read.

Two things stand beside it and neither is inside what is being proven. The
printer is an `OctoPrint` stand-in on a real socket — the supervisor reaches it
over HTTP exactly as it reaches the machine beside the printer, and what proves
the same operations against a real `OctoPrint` is the printer integration tier.
The print and the image are opened by posting one `Obico` failure alert to the
supervisor's own ingress, which is how a print comes to exist at all; their
identifiers are then read out of the store the supervisor wrote.
"""

from __future__ import annotations

import json
import sqlite3
import subprocess
import threading
import time
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import TracebackType

from repo_checks.shell import start

#: The shared word the ingress requires of every post.
INGRESS_WORD = "a-shared-word-for-a-smoke-check"

#: The header that word travels in, as the ingress spells it.
INGRESS_HEADER = "x-printobserver-token"

#: The file the supervisor writes the address it bound into, for the clients
#: beside it. Reading it is how this finds a server started on a port the
#: operating system chose.
CLIENT_CONFIG = "client.toml"

#: The store the supervisor keeps its record in.
STORE = "printobserver.sqlite3"

#: A one-pixel image, so the alert this posts carries a snapshot the supervisor
#: really fetches and really stores.
IMAGE = bytes.fromhex(
    "ffd8ffe000104a46494600010101006000600000ffdb0043000806060706"
    "05080707070909080a0c140d0c0b0b0c1912130f141d1a1f1e1d1a1c1c20"
    "242e2720222c231c1c2837292c30313434341f27393d38323c2e333432ff"
    "c0000b080001000101011100ffc400140001000000000000000000000000"
    "00000009ffc40014100100000000000000000000000000000000ffda0008"
    "010100003f002a9fffd9"
)

#: The statuses the ingress answers a post it took under.
ACCEPTED = (200, 202)

#: How long the supervisor is given to answer at the address it bound.
STARTUP_TIMEOUT_SECONDS = 60.0

#: How long the ingress is given to open the print its alert names.
INGRESS_TIMEOUT_SECONDS = 60.0


@dataclass(frozen=True, slots=True)
class Running:
    """A supervisor a client can be pointed at."""

    #: Where it answers, as its own client configuration writes it.
    server: str
    #: The print every read of the smoke checks is about.
    print_id: str
    #: The image the materialization read is about.
    image_id: str
    #: Where its record and its images live.
    state: Path


class WorldError(RuntimeError):
    """A supervisor could not be brought up for a client to be proven against."""


class Machine:
    """An `OctoPrint` the supervisor reaches over HTTP, holding one printing job."""

    def __init__(self) -> None:
        """Start answering on a port the operating system chooses."""
        self.asked: list[str] = []
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), _machine_handler(self))
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._serving.start()

    @property
    def url(self) -> str:
        """Where this machine answers."""
        host, port = self._server.server_address[:2]
        return f"http://{host!s}:{port}"

    def stop(self) -> None:
        """Stop answering, leaving no thread behind."""
        self._server.shutdown()
        self._server.server_close()
        self._serving.join(timeout=10)

    def answer(self, path: str) -> tuple[int, bytes]:
        """What this machine answers one read with."""
        self.asked.append(path)
        if path.startswith("/api/printer"):
            return 200, json.dumps(
                {
                    "state": {
                        "text": "Printing",
                        "flags": {"operational": True, "printing": True},
                    },
                    "temperature": {
                        "tool0": {"actual": 210.0, "target": 210.0, "offset": 0.0},
                        "bed": {"actual": 60.0, "target": 60.0, "offset": 0.0},
                    },
                }
            ).encode()
        if path.startswith("/api/job"):
            return 200, json.dumps(
                {
                    "state": "Printing",
                    "job": {
                        "file": {"name": "benchy.gcode", "origin": "local", "size": 4096},
                        "estimatedPrintTime": 3600.0,
                    },
                    "progress": {
                        "completion": 12.5,
                        "printTime": 450,
                        "printTimeLeft": 3150,
                    },
                }
            ).encode()
        if path.startswith("/snapshot"):
            return 200, IMAGE
        return 204, b""


def _machine_handler(machine: Machine) -> type[BaseHTTPRequestHandler]:
    """The request handler the stand-in machine answers through."""

    class Handler(BaseHTTPRequestHandler):
        """Answer a read, and take a write without moving anything."""

        protocol_version = "HTTP/1.1"

        def _answer(self, status: int, body: bytes) -> None:
            self.send_response(status)
            self.send_header(
                "Content-Type",
                "image/jpeg" if body[:2] == b"\xff\xd8" else "application/json",
            )
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self) -> None:
            """Answer a read."""
            status, body = machine.answer(self.path)
            self._answer(status, body)

        def do_POST(self) -> None:
            """Take a write, recording that it arrived."""
            length = int(self.headers.get("Content-Length") or 0)
            self.rfile.read(length)
            machine.asked.append(self.path)
            self._answer(204, b"")

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: a journey's output is its assertions, not its traffic."""

    return Handler


def _configuration(state: Path, machine: str) -> str:
    """The one configuration file the supervisor reads, as a document."""
    return json.dumps(
        {
            "state_dir": str(state),
            "listen": "127.0.0.1:0",
            "octoprint": {"url": machine, "api_key": "a-provisioned-key", "fan": "commandable"},
            "supervisor": {"harness": "claude-code"},
            "ingress": {"shared_secret": INGRESS_WORD, "answer_bound_ms": 1000},
            "safety": {
                "agent_min_interval_s": 0,
                "allowed": {
                    "feedrate": {"min": 0.5, "max": 1.5},
                    "flowrate": {"min": 0.9, "max": 1.1},
                    "fan": {"min": 0.0, "max": 100.0},
                    "bed_target": {"min": 0.0, "max": 110.0},
                    "tool_target:0": {"min": 0.0, "max": 260.0},
                },
                "actions": {
                    "operator": [
                        "pause",
                        "resume",
                        "cancel",
                        "start_print",
                        "set_feedrate_factor",
                        "set_flowrate_factor",
                        "set_tool_target_c",
                        "set_bed_target_c",
                        "set_fan_percent",
                        "acknowledge_failure",
                    ],
                    "agent": ["pause", "set_feedrate_factor"],
                    "system": ["pause"],
                },
            },
        },
        indent=2,
    )


class World:
    """A stand-in machine, a real supervisor over it, and a print to read."""

    def __init__(self, program: Path, root: Path) -> None:
        """Bring one up under `root`, running the program at `program`."""
        self.program = program
        self.root = root
        self.machine = Machine()
        self.state = root / "state"
        self.state.mkdir(parents=True, exist_ok=True)
        self._supervisor: subprocess.Popen[str] | None = None

    def __enter__(self) -> Running:
        """Start the supervisor and open a print for a client to read."""
        return self.start()

    def __exit__(
        self,
        kind: type[BaseException] | None,
        value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        """Stop everything this started."""
        self.stop()

    def start(self) -> Running:
        """Start the supervisor and open a print for a client to read.

        Raises:
            WorldError: If the supervisor did not come up, or opened no print.
        """
        configuration = self.root / "supervisor.toml"
        configuration.write_text(
            _as_toml(json.loads(_configuration(self.state, self.machine.url))),
            encoding="utf-8",
        )
        self._supervisor = start(
            [str(self.program), "server", "--config", str(configuration)],
            cwd=self.root,
        )
        server = self._await_address()
        print_id, image_id = self._open_a_print(server)
        return Running(server=server, print_id=print_id, image_id=image_id, state=self.state)

    def stop(self) -> None:
        """Stop the supervisor and the machine, leaving no process behind."""
        if self._supervisor is not None:
            self._supervisor.terminate()
            try:
                self._supervisor.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self._supervisor.kill()
                self._supervisor.wait(timeout=30)
            self._supervisor = None
        self.machine.stop()

    def _await_address(self) -> str:
        """The address the supervisor bound, read from what it wrote for its clients.

        Raises:
            WorldError: If it never wrote one.
        """
        written = self.state / CLIENT_CONFIG
        deadline = time.monotonic() + STARTUP_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            if self._supervisor is not None and self._supervisor.poll() is not None:
                said = self._supervisor.communicate()[1]
                msg = f"the supervisor stopped before it answered:\n{said}"
                raise WorldError(msg)
            if written.is_file():
                for line in written.read_text(encoding="utf-8").splitlines():
                    name, _, value = line.partition("=")
                    if name.strip() == "server":
                        return value.strip().strip('"')
            time.sleep(0.1)
        msg = f"the supervisor wrote no {CLIENT_CONFIG} in {STARTUP_TIMEOUT_SECONDS}s"
        raise WorldError(msg)

    def _open_a_print(self, server: str) -> tuple[str, str]:
        """Open a print and an image by posting one alert to the ingress.

        Raises:
            WorldError: If the ingress opened none.
        """
        import http.client
        from urllib.parse import urlsplit

        alert = json.dumps(
            {
                "event": {"type": "PrintFailure", "is_warning": False, "print_paused": True},
                "printer": {"id": 17, "name": "A stand-in machine"},
                "print": {
                    "id": 4211,
                    "filename": "benchy.gcode",
                    "started_at": 1772366400.5,
                    "ended_at": "",
                },
                "img_url": f"{self.machine.url}/snapshot.jpg",
            }
        )
        connection = http.client.HTTPConnection(urlsplit(server).netloc, timeout=30)
        connection.request(
            "POST",
            "/obico/webhook",
            body=alert,
            headers={"Content-Type": "application/json", INGRESS_HEADER: INGRESS_WORD},
        )
        answered = connection.getresponse()
        answered.read()
        connection.close()
        # The ingress answers before its handling completes — it has a bound to
        # stay inside and a producer that does not retry — so what is asserted
        # here is that it took the body, and the print is waited for below.
        if answered.status not in ACCEPTED:
            msg = f"the ingress answered {answered.status} to the alert that opens a print"
            raise WorldError(msg)

        deadline = time.monotonic() + INGRESS_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            found = self._recorded()
            if found is not None:
                return found
            time.sleep(0.2)
        msg = "the ingress accepted the alert and no print and image were recorded"
        raise WorldError(msg)

    def _recorded(self) -> tuple[str, str] | None:
        """The print and image the ingress opened, read out of the store."""
        store = self.state / STORE
        if not store.is_file():
            return None
        connection = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
        try:
            prints = connection.execute("SELECT id FROM prints").fetchall()
            images = connection.execute("SELECT id FROM images").fetchall()
        except sqlite3.DatabaseError:
            return None
        finally:
            connection.close()
        if not prints or not images:
            return None
        return str(prints[0][0]), str(images[0][0])


def _as_toml(document: dict[str, object]) -> str:
    """One configuration document, as the TOML the supervisor reads.

    Every value below the top level is written as an inline table, which is
    valid TOML whatever a key is spelled like — and one of them is spelled
    `tool_target:0`, which a section header cannot carry unquoted. Written here
    rather than with a serializer, because this repository takes no
    TOML-writing dependency.
    """
    return "".join(f"{_key(name)} = {_inline(value)}\n" for name, value in document.items())


def _key(name: str) -> str:
    """One key, quoted where TOML needs it to be."""
    plain = name.replace("_", "").replace("-", "").isalnum()
    return name if plain else json.dumps(name)


def _inline(value: object) -> str:
    """One value, as TOML writes it inline."""
    if isinstance(value, dict):
        written = ", ".join(f"{_key(str(k))} = {_inline(v)}" for k, v in value.items())
        return "{ " + written + " }"
    if isinstance(value, list):
        return "[" + ", ".join(_inline(entry) for entry in value) + "]"
    return json.dumps(value)
