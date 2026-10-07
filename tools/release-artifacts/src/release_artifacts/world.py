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

import hashlib
import json
import queue
import secrets
import sqlite3
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from socket import socket
from types import TracebackType
from typing import IO, NewType
from urllib.parse import urlsplit

from repo_checks.shell import start


class QuietServer(ThreadingHTTPServer):
    """A server whose peers may go away mid-connection without a traceback for it.

    The supervisor keeps its connections to the stand-in machine open, and
    stopping it closes them from its end. On Windows that reaches the handler
    thread as `ConnectionResetError` rather than as the end of the stream, and
    the default `handle_error` prints a traceback to standard error for each
    — which is nothing a reader of a proof's output needs, and on a tier whose
    pass is one line per proof, it is noise the journeys refuse.
    """

    def handle_error(self, request: socket | tuple[bytes, socket], client_address: object) -> None:
        """Say nothing of a peer that went away; report anything else as the base does."""
        if isinstance(sys.exc_info()[1], ConnectionResetError):
            return
        super().handle_error(request, client_address)


#: The shared word the ingress requires of every post.
INGRESS_WORD = "a-shared-word-for-a-smoke-check"

#: The header that word travels in, as the ingress spells it.
INGRESS_HEADER = "x-printobserver-token"

#: What the supervisor says on standard error, followed by the address it
#: bound, once it is serving. Reading it is how this finds a server started on
#: a port the operating system chose: the supervisor writes the address down
#: nowhere else.
SERVING_ON = "printobserver is serving on "

#: How many random bytes the operator credential this puts in force is drawn
#: from, in the URL-safe alphabet an `Authorization` header carries as it is.
CREDENTIAL_BYTES = 32

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

#: The contract naming every action there is, in the tree this module is in.
#: The operator is granted the whole vocabulary below, and reading it from
#: here rather than copying it is what grants a variant the contracts gain
#: without anybody spelling its name a second time.
ACTION_KINDS = Path(__file__).resolve().parents[4] / "schemas/printobserver-core/ActionKind.json"

#: The agent's skill the supervisor is configured with: the committed one, which
#: is what `gh skill install` puts on a host. The program carries none.
SKILL = Path(__file__).resolve().parents[4] / "skills/printobserver/SKILL.md"

#: How long the supervisor is given to answer at the address it bound.
STARTUP_TIMEOUT_SECONDS = 60.0

#: How long the ingress is given to open the print its alert names.
INGRESS_TIMEOUT_SECONDS = 60.0


#: The API credential a supervisor serves under, as a request presents it.
Credential = NewType("Credential", str)


@dataclass(frozen=True, slots=True)
class Running:
    """A supervisor a client can be pointed at."""

    #: Where it answers, as it said it does.
    server: str
    #: The operator credential it admits, whose verifier its configuration names.
    credential: Credential
    #: The print every read of the smoke checks is about.
    print_id: str
    #: The image the materialization read is about.
    image_id: str
    #: The failure event an acknowledgement is about.
    event_id: str
    #: Where its record and its images live.
    state: Path
    #: The file a journey against this world starts a print of.
    file_name: str


class WorldError(RuntimeError):
    """A supervisor could not be brought up for a client to be proven against."""


def _addressable(server: str) -> bool:
    """Whether a server is an `http://host:port` address a client can connect to."""
    split = urlsplit(server)
    try:
        port = split.port
    except ValueError:
        return False
    return split.scheme == "http" and bool(split.hostname) and port is not None


def verifier_of(credential: str) -> str:
    """The `api.credential_verifier` a supervisor admits one credential by.

    The SHA-256 of its UTF-8 bytes in lowercase hexadecimal, behind `sha256:`:
    what `printobserver credential issue` prints, computed here because the
    world is configured before the program could be asked.
    """
    return "sha256:" + hashlib.sha256(credential.encode("utf-8")).hexdigest()


class Machine:
    """An `OctoPrint` the supervisor reaches over HTTP, holding one printing job."""

    def __init__(self) -> None:
        """Start answering on a port the operating system chooses."""
        self.asked: list[str] = []
        self._server = QuietServer(("127.0.0.1", 0), _machine_handler(self))
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


def every_action(contract: Path = ACTION_KINDS) -> list[str]:
    """Every action kind the contract declares, in the order it declares them.

    Raises:
        WorldError: If the contract is not the closed vocabulary its schema is.
    """
    try:
        document = json.loads(contract.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        msg = f"the action contract at {contract} could not be read: {error}"
        raise WorldError(msg) from error
    variants = document.get("oneOf") if isinstance(document, dict) else None
    kinds = [one.get("const") for one in variants] if isinstance(variants, list) else []
    if not kinds or not all(isinstance(kind, str) and kind for kind in kinds):
        msg = f"the action contract at {contract} does not declare a closed vocabulary of names"
        raise WorldError(msg)
    return kinds


def _configuration(
    state: Path, printer: Printer, credential: str, *, refusing: bool = False
) -> str:
    """The one configuration file the supervisor reads, as a document.

    It names the verifier of `credential` alone, as an operator who issued one
    configures it, and never the credential. `refusing` grants the operator
    nothing: a client of such a supervisor acts as the operator, the one
    identity its credential is, and is refused every action by the grant — the
    one rejection the policy takes before it looks at the state, the interval
    or the bounds — from wherever the machine happens to be.
    """
    document = {
        "state_dir": str(state),
        "listen": "127.0.0.1:0",
        "octoprint": {
            "url": printer.url,
            "api_key": printer.api_key,
            "fan": "commandable",
        },
        "supervisor": {"harness": "claude-code", "skill_path": str(SKILL)},
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
                # Every action there is, read from the contract that names them —
                # or, for a refusing world, none.
                "operator": [] if refusing else every_action(),
                # Nothing: no client of this world is a supervision turn, and
                # an operator's credential claiming the agent is refused
                # before the policy is asked anything.
                "agent": [],
                "system": ["pause"],
            },
        },
    }
    document["api"] = {"credential_verifier": verifier_of(credential)}
    return json.dumps(document, indent=2)


#: Where `just octoprint-up` keeps what it started, and the two files this
#: reads out of it. There is no fallback and no skip: a tier that quietly
#: passed against no printer would prove nothing.
OCTOPRINT_STATE = ".octoprint-env"
OCTOPRINT_RECORD = "instance.json"
OCTOPRINT_KEY = "api-key"

#: The file the scripted environment starts a print of, which is what a journey
#: against it starts again.
HOLD_FILE = "hold.gcode"


@dataclass(frozen=True, slots=True)
class Printer:
    """Where the machine on the far side of the printer port answers."""

    url: str
    api_key: str
    #: Whether it is a real `OctoPrint` rather than the stand-in below.
    scripted: bool


def scripted_printer(root: Path) -> Printer:
    """The `OctoPrint` `just octoprint-up` started.

    Raises:
        WorldError: If the environment is not up, naming the recipe that brings
            one up.
    """
    state = root / OCTOPRINT_STATE
    record = state / OCTOPRINT_RECORD
    key = state / OCTOPRINT_KEY
    if not record.is_file() or not key.is_file():
        msg = (
            f"the scripted OctoPrint environment is not up: {record} could not be read. "
            f"Run `just octoprint-up` first; this tier drives a real OctoPrint and has "
            f"no fixture to fall back to."
        )
        raise WorldError(msg)
    described = json.loads(record.read_text(encoding="utf-8"))
    return Printer(
        url=str(described["url"]), api_key=key.read_text(encoding="utf-8").strip(), scripted=True
    )


class World:
    """A machine, a real supervisor over it, and a print to read."""

    def __init__(
        self,
        program: Path,
        root: Path,
        printer: Printer | None = None,
        *,
        credential: str | None = None,
        refusing: bool = False,
    ) -> None:
        """Bring one up under `root`, running the program at `program`.

        `printer` is the machine the supervisor reaches. Given none, a stand-in
        on a real socket is started; given the scripted `OctoPrint`, that is
        what the supervisor drives and the stand-in serves only the snapshot the
        alert below names.

        `credential` is the operator credential the supervisor is configured
        to admit, by its verifier. Given none, one is drawn from
        `CREDENTIAL_BYTES` random bytes; a
        journey names one to hold a client to a credential of a particular
        shape. `refusing` grants the operator nothing; see `_configuration`.
        """
        self.program = program
        self.root = root
        self.credential = credential or secrets.token_urlsafe(CREDENTIAL_BYTES)
        self.refusing = refusing
        self.machine = Machine()
        self.printer = printer or Printer(self.machine.url, "a-provisioned-key", scripted=False)
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
            _as_toml(
                json.loads(
                    _configuration(
                        self.state, self.printer, self.credential, refusing=self.refusing
                    )
                )
            ),
            encoding="utf-8",
        )
        self._supervisor = start(
            [str(self.program), "server", "--config", str(configuration)],
            cwd=self.root,
        )
        server = self._await_serving()
        print_id, image_id, event_id = self._open_a_print(server)
        return Running(
            server=server,
            credential=Credential(self.credential),
            print_id=print_id,
            image_id=image_id,
            event_id=event_id,
            state=self.state,
            file_name=HOLD_FILE,
        )

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

    def _await_serving(self) -> str:
        """The address the supervisor bound, as it says it is serving on it.

        Read off its standard error, by a thread of its own that goes on
        reading — so nothing it prints later can fill a pipe nobody empties.

        Raises:
            WorldError: If it stopped first, never said, or said something that
                is no `http://host:port` address.
        """
        supervisor = self._supervisor
        if supervisor is None or supervisor.stderr is None:
            msg = "the supervisor was not started with its standard error read"
            raise WorldError(msg)
        lines: queue.Queue[str | None] = queue.Queue()
        said: list[str] = []

        def pump(stream: IO[str]) -> None:
            for line in stream:
                lines.put(line)
            lines.put(None)

        threading.Thread(target=pump, args=(supervisor.stderr,), daemon=True).start()
        deadline = time.monotonic() + STARTUP_TIMEOUT_SECONDS
        while (remaining := deadline - time.monotonic()) > 0:
            try:
                line = lines.get(timeout=remaining)
            except queue.Empty:
                break
            if line is None:
                supervisor.wait(timeout=30)
                msg = "the supervisor stopped before it answered:\n" + "".join(said)
                raise WorldError(msg)
            said.append(line)
            if line.startswith(SERVING_ON):
                server = "http://" + line.removeprefix(SERVING_ON).strip()
                if not _addressable(server):
                    msg = (
                        f"the supervisor said it serves on no http://host:port address: {server!r}"
                    )
                    raise WorldError(msg)
                return server
        msg = f"the supervisor never said where it serves in {STARTUP_TIMEOUT_SECONDS}s"
        raise WorldError(msg)

    def _open_a_print(self, server: str) -> tuple[str, str, str]:
        """Open a print, an image and an event by posting one alert to the ingress.

        Raises:
            WorldError: If the ingress opened none.
        """
        import http.client

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
        # llmlint: ignore[async_typed_clients_at_boundaries] one post in a sequential proof
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
        msg = "the ingress accepted the alert but did not finish preparing its print and image"
        raise WorldError(msg)

    def _recorded(self) -> tuple[str, str, str] | None:
        """The print and image, once the ingress has finished observing the printer.

        Image storage precedes the asynchronous handler's printer read. Returning
        at that point lets a journey cancel before the handler samples the
        machine, causing it to close the print over that transient idle state.
        The turn's assessment or failure is recorded after the context read, so
        either is the readiness boundary before a client may change the printer.
        """
        store = self.state / STORE
        if not store.is_file():
            return None
        connection = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
        try:
            prints = connection.execute("SELECT id FROM prints").fetchall()
            images = connection.execute("SELECT id FROM images").fetchall()
            events = connection.execute(
                "SELECT id FROM events WHERE kind = 'obico_failure_alert'"
            ).fetchall()
            prepared = connection.execute(
                "SELECT id FROM events WHERE kind = 'agent_assessment' "
                "OR (kind = 'port_failure' "
                "AND json_extract(payload, '$.payload.site') = 'supervision_turn')"
            ).fetchall()
        except sqlite3.DatabaseError:
            return None
        finally:
            connection.close()
        if not prints or not images or not events or not prepared:
            return None
        return str(prints[0][0]), str(images[0][0]), str(events[0][0])


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
