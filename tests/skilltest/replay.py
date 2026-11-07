"""One case's replay: the real server, fed the case's events in order, one turn each.

The server is the real `printobserver server`, configured as the installer
configures it, and its supervisor port runs the shipped harness with the shipped
turn prompt and skill. Everything about the agent's world is a neutral temporary
directory laid out as a per-user install: nothing it can read names this
repository, a test or a test runner.

Each `replay` entry waits for the turn before it to end. A `periodic_observation`
is the server's own scheduled check; an alert is posted to its real webhook, as
Obico's plugin posts one. Each decision point is then judged from the server's
own history of what the agent requested during that turn.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import secrets
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import tomllib
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import datetime
from http import HTTPStatus
from http.client import HTTPConnection
from pathlib import Path
from typing import Any, Literal, NewType, NotRequired, TextIO, TypedDict, cast, get_args

import doubles
from cases import REPO, Case, Moment, Request
from jsonschema import Draft202012Validator
from repo_checks import platforms
from repo_checks.checks_real_prints import pointer
from repo_checks.model import Repo
from repo_checks.shell import run, start

INSTALLER = REPO / "scripts" / "install.sh"
SKILL = REPO / "skills" / "printobserver"
#: What the installer's interval option writes: short, because each check waits on
#: this after the turn before it ends, and its value is what the event then says.
OBSERVATION_INTERVAL_S = 5
#: Longer than a turn's own timeout and the agent's looks together; a turn that
#: takes longer has hung, and the replay reports which.
TURN_DEADLINE_S = 1800
START_DEADLINE_S = 60
#: How long the server has to write down an alert it answered.
ALERT_DEADLINE_S = 30
CLIENT_DEADLINE_S = 120
#: The engine re-prompts a turn at most twice, so a whole replay spending more than
#: this many harness runs per entry has gone wrong.
RUN_ALLOWANCE_PER_ENTRY = 3
POLL_S = 0.5
#: Who may ask for something, as the server's own schema declares it.
ACTOR = Draft202012Validator(
    json.loads((REPO / "schemas/printobserver-core/Actor.json").read_text(encoding="utf-8"))
)
#: The kinds an agent's request is recorded under.
REQUESTED = {"action_requested", "recovery_requested"}
#: The kinds a turn ending is recorded under.
ASSESSED = "agent_assessment"
FAILED = "port_failure"
FAILED_SITE = "supervision_turn"
#: The event kinds a replay entry is delivered under.
TRIGGERS = {"periodic_observation", "obico_failure_alert"}


class ReplayError(Exception):
    """A replay that could not put the agent in front of its events."""


def program(name: str) -> Path:
    """A built program of this workspace."""
    suffix = platforms.host(Repo(REPO)).program.removeprefix("printobserver")
    return REPO / "target" / "skilltest" / f"{name}{suffix}"


def _installer_template() -> str:
    """The configuration the per-user installer writes, as the shell script holds it."""
    found = re.search(
        r"<<CONFIG \|\| die [^\n]*\n(?P<body>.*?)^CONFIG$",
        INSTALLER.read_text(encoding="utf-8"),
        re.DOTALL | re.MULTILINE,
    )
    if found is None:
        msg = f"{INSTALLER} no longer writes its configuration from a here-document"
        raise ReplayError(msg)
    return found["body"]


OctoPrintKey = NewType("OctoPrintKey", str)
IngressToken = NewType("IngressToken", str)
OperatorCredential = NewType("OperatorCredential", str)
ObicoToken = NewType("ObicoToken", str)
#: The print the server minted for this replay.
PrintId = NewType("PrintId", str)


@dataclass(frozen=True)
class Secrets:
    """What the doubles and the server authenticate each other with, minted per replay."""

    octoprint_key: OctoPrintKey = field(default_factory=lambda: OctoPrintKey(secrets.token_hex(16)))
    ingress: IngressToken = field(default_factory=lambda: IngressToken(secrets.token_hex(16)))
    operator: OperatorCredential = field(
        default_factory=lambda: OperatorCredential(secrets.token_hex(32))
    )
    obico_token: ObicoToken = field(default_factory=lambda: ObicoToken(secrets.token_hex(16)))


def service_config(
    state: Path, listen: str, octoprint: str, camera: str, obico: str, minted: Secrets
) -> str:
    """The installer's configuration, with what an operator fills in filled in."""
    verifier = hashlib.sha256(minted.operator.encode()).hexdigest()
    text = (
        _installer_template()
        .replace("$RUNTIME_STATE", state.as_posix())
        .replace("$INTERVAL_LINE", f"observation_interval_s = {OBSERVATION_INTERVAL_S}")
        .replace('listen = "127.0.0.1:8420"', f'listen = "{listen}"')
        .replace('url = "http://127.0.0.1:5000"', f'url = "{octoprint}"')
        .replace('api_key = ""', f'api_key = "{minted.octoprint_key}"')
        .replace('shared_secret = ""', f'shared_secret = "{minted.ingress}"')
    )
    text = f'api.credential_verifier = "sha256:{verifier}"\n{text}'
    text += (
        f'\n[camera]\nsnapshot_url = "{camera}/api/frame.jpeg"\n'
        f'\n[obico]\nurl = "{obico}"\naccess_token = "{minted.obico_token}"\n'
        f'ml_api_url = "{obico}/ml/"\n'
    )
    tomllib.loads(text)
    return text


def _free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def neutral_environment(bin_directory: Path, home: Path) -> dict[str, str]:
    """A service's environment: its search path, its home, and the harness's credential alone.

    Built up rather than stripped down, so nothing this process was started with
    reaches the agent by being forgotten. No entry of the search path is inside
    this repository, and the first is the install's own.
    """
    kept = [
        entry
        for entry in os.environ.get("PATH", "").split(os.pathsep)
        if entry and not Path(entry).resolve().is_relative_to(REPO)
    ]
    environment = {
        "PATH": os.pathsep.join([str(bin_directory), *kept]),
        "HOME": str(home),
        "USERPROFILE": str(home),
        "LANG": "C.UTF-8",
    }
    for name in ("SystemRoot", "SYSTEMROOT", "TEMP", "TMP", "COMSPEC", "PATHEXT"):
        if name in os.environ:
            environment[name] = os.environ[name]
    token = harness_token()
    if token:
        environment["CLAUDE_CODE_OAUTH_TOKEN"] = token
    return environment


def harness_token() -> str | None:
    """What the shipped harness authenticates with, in the variable a service would be given.

    CI hands it `CLAUDE_CODE_OAUTH_TOKEN`. A signed-in host hands the access token
    of its own sign-in instead, read and never written back, because a copy of the
    sign-in would rotate its refresh token out from under the original.
    """
    token = os.environ.get("CLAUDE_CODE_OAUTH_TOKEN")
    if token:
        return token
    configured = os.environ.get("CLAUDE_CONFIG_DIR")
    directory = Path(configured) if configured else Path.home() / ".claude"
    try:
        document = json.loads((directory / ".credentials.json").read_text(encoding="utf-8"))
    except OSError, ValueError:
        return None
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
    sign_in = document.get("claudeAiOauth") if isinstance(document, dict) else None
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
    held = sign_in.get("accessToken") if isinstance(sign_in, dict) else None
    return held if isinstance(held, str) else None


def refuse_a_pointer(data: bytes, case: str, name: str) -> bytes:
    """A frame's bytes, unless they are the LFS pointer an unfetched checkout holds."""
    if pointer(data[:2048]) is not None:
        msg = f"{case}/{name} is an LFS pointer: run `just real-prints-fetch`"
        raise ReplayError(msg)
    return data


def _utc_instant(held: str) -> datetime:
    """A time the server wrote, its nanoseconds cut to what `datetime` holds."""
    return datetime.fromisoformat(
        re.sub(r"(\.\d{6})\d*Z$", r"\1+00:00", held).replace("Z", "+00:00")
    )


def _is_timestamp(value: object) -> bool:
    """Whether the server's answer is a time the replay can read."""
    if not isinstance(value, str):
        return False
    try:
        return _utc_instant(value).tzinfo is not None
    except ValueError:
        return False


class HeldPrint(TypedDict):
    """One print the server answers, as far as the replay reads one."""

    id: str
    job_started_at: NotRequired[str | None]


class EventImage(TypedDict):
    """The picture an event carries, as far as the replay reads one."""

    sha256: NotRequired[str]


class HistoryEvent(TypedDict):
    """One event of the print's history, as far as the replay reads one."""

    id: str
    kind: str
    received_at: str
    payload: dict[str, Any]
    image: NotRequired[EventImage | None]


def is_held_print(held: object) -> bool:
    """Whether one of the server's prints has the fields the replay reads from it."""
    if not isinstance(held, dict) or not isinstance(held.get("id"), str):
        return False
    started = held.get("job_started_at")
    return started is None or _is_timestamp(started)


def is_event(event: object) -> bool:
    """Whether one history event has the fields and shapes the replay reads from it."""
    if (
        not isinstance(event, dict)
        or not isinstance(event.get("id"), str)
        or not isinstance(event.get("kind"), str)
        or not _is_timestamp(event.get("received_at"))
    ):
        return False
    payload = event.get("payload")
    image = event.get("image")
    if not isinstance(payload, dict) or not (
        image is None or (isinstance(image, dict) and isinstance(image.get("sha256", ""), str))
    ):
        return False
    match event["kind"]:
        case "port_failure":
            return (
                isinstance(payload.get("site"), str)
                and isinstance(payload.get("detail", ""), str)
                and (payload["site"] != FAILED_SITE or isinstance(payload.get("event_id"), str))
            )
        case "action_requested" | "recovery_requested":
            return _is_requested_action(payload.get("action"))
        case "agent_assessment":
            return is_assessment(payload.get("assessment"))
        case _:
            return True


def _is_requested_action(action: object) -> bool:
    """Whether a request says what was asked and who asked, which is how it is judged."""
    if not isinstance(action, dict) or not isinstance(action.get("action"), str):
        return False
    return _is_actor(action.get("actor"))


def _is_actor(actor: object) -> bool:
    """Whether it is an actor the server's committed `Actor` schema declares."""
    return ACTOR.is_valid(actor)


def is_assessment(assessment: object) -> bool:
    """Whether a turn's assessment has every field the record carries; a turn may have none."""
    if assessment is None:
        return True
    return (
        isinstance(assessment, dict)
        and Assessment.__annotations__.keys() <= assessment.keys()
        and assessment["confidence"] in get_args(Confidence)
        and all(isinstance(assessment[name], str) for name in ("summary", "did", "why"))
        and all(isinstance(assessment[name], bool) for name in ("should_continue", "escalating"))
    )


def kept_harness(root: Path = REPO) -> str | None:
    """The harness this run started with, kept where nothing on the host replaces it.

    A host's global install is replaced whole when anything on that host updates it,
    and for that moment no program is there to start; a run is also one release's
    evidence. So the program is copied once, under a name of its own in this tree's
    build directory, and every test of the run starts that copy.
    """
    found = shutil.which("claude")
    if found is None:
        return None
    source = Path(found).resolve()
    kept = root / "target" / "skilltest" / "harness" / f"{source.stat().st_size}-{source.name}"
    if not kept.is_file():
        kept.parent.mkdir(parents=True, exist_ok=True)
        partial = kept.with_suffix(f".{os.getpid()}")
        shutil.copy2(source, partial)
        partial.replace(kept)
    return str(kept)


def real_picture(case: Case) -> Callable[[str], bytes]:
    """A frame's bytes from the case's own directory."""

    def read(name: str) -> bytes:
        return refuse_a_pointer((case.directory / name).read_bytes(), case.name, name)

    return read


def delivered(status: int) -> bool:
    """Whether the ingress took an alert: it answers before it has handled one."""
    return status == HTTPStatus.ACCEPTED


Confidence = Literal["low", "medium", "high"]


class Assessment(TypedDict):
    """What the agent answered a turn with, as the server recorded it."""

    summary: str
    confidence: Confidence
    should_continue: bool
    did: str
    why: str
    escalating: bool


class DecisionRecord(TypedDict):
    """One decision point as `results/latest.json` carries it."""

    scenario: str
    at: str
    trigger: str
    passed: bool
    why_not: str | None
    requests: list[str]
    commands: list[str]
    opened: list[str]
    assessment: Assessment | None
    looks: int
    usage: dict[str, Any] | None
    models: list[str]
    compacted: bool


class TestRecord(TypedDict):
    """One test as `results/latest.json` carries it."""

    test: str
    passed: bool
    turns: int
    sessions: list[str]
    decision_points: list[DecisionRecord]


class LedgerSession(TypedDict):
    """One conversation of a print, as the supervisor port's ledger holds it."""

    session_name: str
    harness_identity: str


class LedgerTurn(TypedDict):
    """One turn of a print, as the supervisor port's ledger holds it."""

    session_name: str


class Ledger(TypedDict):
    """What the supervisor port wrote down of a print."""

    sessions: list[LedgerSession]
    turns: list[LedgerTurn]


@dataclass
class Run:
    """One harness run: the replay entry it served, and where what it said is kept."""

    #: Its output, which is the provider's one result document.
    record: Path
    moment_index: int
    #: How far each session transcript had grown when it started.
    transcript_sizes: dict[Path, int]
    #: When it started, on the clock the server's record is kept by, and when it ended.
    started_at: datetime
    #: What its session transcripts gained while it ran.
    transcript: list[dict[str, Any]] = field(default_factory=list)
    ended_at: datetime | None = None


@dataclass
class DecisionPoint:
    """One entry of the replay, and what became of it."""

    moment: Moment
    requests: list[Request]
    commands: list[str]
    opened: list[str]
    assessment: Assessment | None
    usage: dict[str, Any] | None
    models: list[str]
    compacted: bool
    failure: str | None
    event_kind: str | None
    image_sha256: str | None
    looks: int

    @property
    def why_not(self) -> str | None:
        """Why this decision point failed, or nothing when it passed."""
        if self.failure:
            return f"the turn failed: {self.failure}"
        return self.moment.verdict(self.requests)

    def as_record(self) -> DecisionRecord:
        """The decision point as `results/latest.json` carries it."""
        return {
            "scenario": self.moment.id,
            "at": self.moment.at,
            "trigger": self.moment.trigger,
            "passed": self.why_not is None,
            "why_not": self.why_not,
            "requests": [request.describe() for request in self.requests],
            "commands": self.commands,
            "opened": self.opened,
            "assessment": self.assessment,
            "looks": self.looks,
            "usage": self.usage,
            "models": self.models,
            "compacted": self.compacted,
        }


@dataclass
class Replayed:
    """A finished replay: its decision points, and what the port wrote down."""

    case: Case
    points: list[DecisionPoint]
    ledger: Ledger
    fetches: list[str]
    state: Path
    transcript: list[dict[str, Any]] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        """Whether every decision point passed, which is what passing the test is."""
        return all(point.why_not is None for point in self.points)

    def failures(self) -> str:
        """Each decision point that failed, for a test's message."""
        return "\n".join(
            f"{point.moment.id} ({point.moment.summary}): {point.why_not}\n"
            f"  it requested: {[request.describe() for request in point.requests]}"
            for point in self.points
            if point.why_not
        )

    def as_record(self) -> TestRecord:
        """The replay as `results/latest.json` carries it."""
        return {
            "test": self.case.name,
            "passed": self.passed,
            "turns": len(self.points),
            "sessions": [session["session_name"] for session in self.ledger["sessions"]],
            "decision_points": [point.as_record() for point in self.points],
        }


def _ledger_of(document: object, path: Path) -> Ledger:
    """A ledger read from the port's file, once it holds what is read from one."""
    sessions = document.get("sessions") if isinstance(document, dict) else None
    turns = document.get("turns") if isinstance(document, dict) else None
    if (
        not isinstance(sessions, list)
        or not isinstance(turns, list)
        or not all(
            isinstance(entry, dict) and isinstance(entry.get("session_name"), str)
            for entry in [*sessions, *turns]
        )
        or not all(isinstance(entry.get("harness_identity"), str) for entry in sessions)
    ):
        msg = f"{path} is no ledger of sessions and turns"
        raise ReplayError(msg)
    return {"sessions": sessions, "turns": turns}


def _received(event: HistoryEvent) -> datetime:
    """When the server kept one event."""
    return _utc_instant(event["received_at"])


def _json_lines(text: str) -> list[dict[str, Any]]:
    lines = []
    for line in text.splitlines():
        try:
            parsed = json.loads(line)
        except ValueError:
            continue
        if isinstance(parsed, dict):
            lines.append(parsed)
    return lines


def _lines_of(harness_run: Run) -> list[dict[str, Any]]:
    """Everything one harness run wrote: its transcript, then its result document."""
    record = harness_run.record
    answered = _json_lines(record.read_text("utf-8", "replace")) if record.is_file() else []
    return [*harness_run.transcript, *answered]


@dataclass
class RunReport:
    """What the runs of one turn said of themselves.

    A harness run answers one result document, carrying the run's usage and the
    models it used. What it did and whether it compacted are only in the session
    transcript the harness keeps under its configuration directory, which the
    harness's sign-in directory is.
    """

    commands: list[str] = field(default_factory=list)
    opened: list[str] = field(default_factory=list)
    seen_tool_uses: set[str] = field(default_factory=set)
    usage: dict[str, Any] | None = None
    models: list[str] = field(default_factory=list)
    compacted: bool = False

    def take(self, lines: list[dict[str, Any]]) -> None:
        """Read one run's output and transcript into this turn's report."""
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        for line in lines:
            match line:
                # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
                case {"type": "system", "subtype": "compact_boundary"}:
                    self.compacted = True
                case {"type": "result", "usage": dict(usage), **rest}:
                    self._add_usage(usage)
                    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
                    models = rest.get("modelUsage")
                    for model in models if isinstance(models, dict) else ():
                        self._saw(str(model))
                case {"message": {"model": str(model), "content": list(content)}}:
                    self._saw(model)
                    for block in content:
                        self._used(block)
                case _:
                    pass

    def _used(self, block: object) -> None:
        """One tool call of the transcript, which writes a call once per streamed message."""
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        match block:
            case {"type": "tool_use", "id": str(called), "name": str(name), "input": dict(given)}:
                if called in self.seen_tool_uses:
                    return
                self.seen_tool_uses.add(called)
                match name, given:
                    case "Bash", {"command": str(command)}:
                        self.commands.append(command)
                    case "Read", {"file_path": str(path)}:
                        self.opened.append(Path(path).name)
                    case _:
                        pass
            case _:
                pass

    def _add_usage(self, usage: dict[str, Any]) -> None:
        """A turn the engine re-prompted is one turn of several runs, so its usage adds up."""
        if self.usage is None:
            self.usage = dict(usage)
            return
        for name, value in usage.items():
            held = self.usage.get(name)
            if isinstance(value, int) and isinstance(held, int):
                self.usage[name] = held + value
            else:
                self.usage.setdefault(name, value)

    def _saw(self, model: str) -> None:
        # `<synthetic>` is the harness's own bookkeeping message, which no model wrote.
        if not model.startswith("<") and model not in self.models:
            self.models.append(model)


class Replay:
    """One running replay of one case."""

    def __init__(
        self,
        case: Case,
        target: list[str],
        picture: Callable[[str], bytes] | None = None,
        gcode: bytes | None = None,
    ) -> None:
        """Lay the install out and start the doubles; nothing runs until [`Replay.run`]."""
        self.case = case
        self.target = target
        self.root = Path(tempfile.mkdtemp())
        self.state = self.root / "state"
        self.home = self.root / "home"
        self.bin = self.root / "bin"
        self.records = self.root / "records"
        for directory in (self.state, self.home, self.bin, self.records):
            directory.mkdir()
        self.minted = Secrets()
        self.timeline = doubles.Timeline(
            case=case,
            picture=picture or real_picture(case),
            clock=self.bin / "clock",
            gcode=gcode,
        )
        self.runs: list[Run] = []
        self.errors: list[str] = []
        self.servers = [
            doubles.octoprint(self.timeline, self.minted.octoprint_key),
            doubles.camera(self.timeline),
            doubles.obico(self.timeline),
            doubles.control(self._harness_started, self._harness_exited, self.errors.append),
        ]
        self.port = _free_port()
        self.server: subprocess.Popen[str] | None = None
        self.print_id: PrintId | None = None

    def _install(self) -> Path:
        """Put the programs, the skill and the configuration where the installer puts them."""
        suffix = platforms.host(Repo(REPO)).program.removeprefix("printobserver")
        shutil.copy2(program("printobserver"), self.bin / f"printobserver{suffix}")
        shutil.copy2(program("printobserver-server-turn-launcher"), self.bin / f"claude{suffix}")
        (self.bin / "launch.json").write_text(
            json.dumps(
                {
                    "control": doubles.address(self.servers[3]).removeprefix("http://"),
                    "program": self.target[0],
                    "arguments": self.target[1:],
                }
            ),
            encoding="utf-8",
        )
        shutil.copytree(SKILL, self.state / "skills" / "printobserver")
        config = self.root / "config.toml"
        config.write_text(
            service_config(
                self.state,
                f"127.0.0.1:{self.port}",
                doubles.address(self.servers[0]),
                doubles.address(self.servers[1]),
                doubles.address(self.servers[2]),
                self.minted,
            ),
            encoding="utf-8",
        )
        return config

    def _harness_started(self) -> str:
        """Note one harness run, and say where its output is to be kept."""
        if self._trigger_count() <= self.timeline.index:
            self.timeline.harness_started()
        else:
            self.timeline.moved_at_exit = False
        record = self.records / f"{len(self.runs):04}.json"
        self.runs.append(
            Run(record, self.timeline.index, self._transcript_sizes(), self.timeline.now())
        )
        return str(record)

    def _transcripts(self) -> list[Path]:
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        return sorted(self.state.glob("harness/claude-code/projects/*/*.jsonl"))

    def _transcript_sizes(self) -> dict[Path, int]:
        return {path: path.stat().st_size for path in self._transcripts()}

    def _harness_exited(self) -> None:
        """Stand at the next event before the server can let its scheduled check fire."""
        run = self.runs[-1]
        run.ended_at = self.timeline.now()
        for path in self._transcripts():
            with path.open("rb") as transcript:
                transcript.seek(run.transcript_sizes.get(path, 0))
                run.transcript.extend(_json_lines(transcript.read().decode("utf-8", "replace")))
        try:
            self._next_event()
        except (ReplayError, OSError, subprocess.SubprocessError) as error:
            self.errors.append(str(error))

    def _next_event(self) -> None:
        timeline = self.timeline
        following = timeline.index + 1
        if following >= len(self.case.moments):
            with timeline.lock:
                timeline.finished = True
            return
        upcoming = self.case.moments[following]
        if timeline.paused and upcoming.printer_state == "printing":
            self._operator_resumes()
        with timeline.lock:
            timeline.stand_at(following)
            timeline.moved_at_exit = True
        if upcoming.is_alert:
            self._deliver_alert(upcoming)

    def _operator_resumes(self) -> None:
        """The person who was told takes it from there, as the record of the real print has it."""
        print_id = self._print_id()
        self._client(
            "resume",
            "--print-id",
            print_id,
            "--actor",
            '"operator"',
            "--reason",
            "Looked at the print, cleared what was wrong and resumed it.",
        )

    def _post_alert(self, moment: Moment) -> None:
        """Post a recorded alert to the real webhook, as Obico's plugin posts one.

        Its body is what the history recorded, with two things this run answers
        differently: where its picture is, and when the job started, which is how
        the server knows the alert is about the print it already has.
        """
        if moment.event_id is None:
            msg = f"{moment.id} is an alert with no recorded event to deliver"
            raise ReplayError(msg)
        body = self.case.recorded_alert(moment.event_id)
        body["img_url"] = f"{doubles.address(self.servers[2])}/media/{moment.event_image}"
        body["print"]["started_at"] = self._job_started_epoch()
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        connection = HTTPConnection("127.0.0.1", self.port, timeout=ALERT_DEADLINE_S)
        connection.request(
            "POST",
            f"/obico/webhook?token={self.minted.ingress}",
            body=json.dumps(body),
            headers={"Content-Type": "application/json"},
        )
        answered = connection.getresponse()
        answered.read()
        connection.close()
        if not delivered(answered.status):
            msg = f"the ingress answered {moment.id}'s alert with {answered.status}"
            raise ReplayError(msg)

    def _job_started_epoch(self) -> float:
        """When the print the server holds says its job started, or when this replay's did."""
        prints = self._prints()
        started = prints[0].get("job_started_at") if prints else None
        return _utc_instant(started).timestamp() if started else self.case.job_started().timestamp()

    def _deliver_alert(self, moment: Moment) -> None:
        """Post an alert while the last turn still holds the print, and wait until it is kept.

        The server hands what arrived during a turn to the next one when it ends,
        so no scheduled check can come between the two.
        """
        before = self._trigger_count()
        self._post_alert(moment)
        deadline = time.monotonic() + ALERT_DEADLINE_S
        while self._trigger_count() <= before:
            if time.monotonic() > deadline:
                msg = f"the server never recorded the alert of {moment.id}"
                raise ReplayError(msg)
            time.sleep(0.05)

    def _client_environment(self) -> dict[str, str]:
        return {
            **neutral_environment(self.bin, self.home),
            "PRINTOBSERVER_SERVER": f"http://127.0.0.1:{self.port}",
            "PRINTOBSERVER_CREDENTIAL": self.minted.operator,
        }

    def _client(self, *arguments: str) -> str:
        """One operator command, through the installed program."""
        suffix = platforms.host(Repo(REPO)).program.removeprefix("printobserver")
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        done = run(
            [str(self.bin / f"printobserver{suffix}"), *arguments, "--json"],
            env=self._client_environment(),
            timeout=CLIENT_DEADLINE_S,
        )
        if done.returncode != 0:
            msg = f"`printobserver {arguments[0]}` answered {done.returncode}: {done.stderr}"
            raise ReplayError(msg)
        return done.stdout

    def _prints(self) -> list[HeldPrint]:
        """The prints the server holds, once they have what is read from them."""
        answered = json.loads(self._client("prints"))
        prints = answered.get("prints") if isinstance(answered, dict) else None
        if not isinstance(prints, list) or not all(map(is_held_print, prints)):
            msg = "the server's prints answered no list of prints"
            raise ReplayError(msg)
        return cast("list[HeldPrint]", prints)

    def _print_id(self) -> PrintId:
        if self.print_id is None:
            prints = self._prints()
            if not prints:
                msg = "the server holds no print"
                raise ReplayError(msg)
            self.print_id = PrintId(prints[0]["id"])
        return self.print_id

    def _events(self) -> list[HistoryEvent]:
        """The print's whole history, oldest first; none before the server holds a print."""
        if self.print_id is None and not self._prints():
            return []
        answered = self._client("history", "--print-id", self._print_id(), "--limit", "1000")
        document = json.loads(answered)
        events = document.get("events") if isinstance(document, dict) else None
        if not isinstance(events, list) or not all(map(is_event, events)):
            msg = "the server's history answered no list of events"
            raise ReplayError(msg)
        return cast("list[HistoryEvent]", list(reversed(events)))

    def _trigger_count(self) -> int:
        return sum(1 for event in self._events() if event["kind"] in TRIGGERS)

    def _turns_ended(self, events: list[HistoryEvent]) -> int:
        ended = 0
        for event in events:
            if event["kind"] == ASSESSED or (
                event["kind"] == FAILED and event["payload"].get("site") == FAILED_SITE
            ):
                ended += 1
        return ended

    def _keep_the_log(self, server: subprocess.Popen[str]) -> None:
        """Keep what the server prints, so neither pipe fills and it is there to read."""

        def kept(stream: TextIO | None) -> None:
            with (self.root / "server.log").open("a", encoding="utf-8") as log:
                for line in stream or ():
                    log.write(line)
                    log.flush()

        for stream in (server.stdout, server.stderr):
            threading.Thread(target=kept, args=(stream,), daemon=True).start()

    def _log_tail(self) -> str:
        """What the server recorded of the failed turn."""
        failures = [
            event["payload"].get("detail", "")
            for event in self._events()
            if event["kind"] == FAILED
        ]
        return "; ".join(failures[-1:])

    def _wait_for_server(self) -> None:
        deadline = time.monotonic() + START_DEADLINE_S
        while time.monotonic() < deadline:
            if self.server is not None and self.server.poll() is not None:
                log = (self.root / "server.log").read_text(encoding="utf-8", errors="replace")
                msg = f"the server exited at start: {log}"
                raise ReplayError(msg)
            # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
            with socket.socket() as probe:
                probe.settimeout(0.2)
                if probe.connect_ex(("127.0.0.1", self.port)) == 0:
                    return
            time.sleep(0.05)
        msg = f"the server did not answer within {START_DEADLINE_S} s"
        raise ReplayError(msg)

    def _wait_for_every_turn(self) -> None:
        deadline = time.monotonic() + TURN_DEADLINE_S * len(self.case.moments)
        wanted = len(self.case.moments)
        while True:
            said = self.bin / "launcher.log"
            if said.is_file():
                self.errors.append(f"the launcher said {said.read_text('utf-8', 'replace')}")
            if self.errors:
                raise ReplayError("; ".join(self.errors))
            if len(self.runs) > RUN_ALLOWANCE_PER_ENTRY * wanted:
                msg = f"{len(self.runs)} harness runs for {wanted} events: stopping the spend"
                raise ReplayError(msg)
            ended = self._turns_ended(self._events())
            if self.timeline.finished and ended >= wanted:
                return
            harness_exits = sum(1 for harness_run in self.runs if harness_run.ended_at)
            if ended > harness_exits:
                said = self.bin / "launcher.log"
                told = said.read_text("utf-8", "replace") if said.is_file() else "nothing"
                msg = (
                    f"a turn ended with no harness run to show for it: {self._log_tail()}; "
                    f"the launcher said {told}"
                )
                raise ReplayError(msg)
            if time.monotonic() > deadline:
                msg = (
                    f"{self._turns_ended(self._events())} of {wanted} turns ended in "
                    f"{TURN_DEADLINE_S * wanted} s"
                )
                raise ReplayError(msg)
            time.sleep(POLL_S)

    def run(self) -> Replayed:
        """Feed every event in order, and judge each decision point from the server's record."""
        config = self._install()
        suffix = platforms.host(Repo(REPO)).program.removeprefix("printobserver")
        self.server = start(
            [str(self.bin / f"printobserver{suffix}"), "server", "--config", str(config)],
            cwd=self.state,
            env=neutral_environment(self.bin, self.home),
        )
        self._keep_the_log(self.server)
        try:
            self._wait_for_server()
            first = self.case.moments[0]
            if first.is_alert:
                self._post_alert(first)
            self._wait_for_every_turn()
            events = self._events()
            return Replayed(
                case=self.case,
                points=self._judged(events),
                ledger=self._ledger(),
                fetches=list(self.timeline.fetches),
                state=self.state,
                transcript=[line for harness_run in self.runs for line in _lines_of(harness_run)],
            )
        finally:
            self.stop()

    def _ledger(self) -> Ledger:
        path = self.state / "supervisor-sessions" / f"{self._print_id()}.json"
        if not path.is_file():
            return {"sessions": [], "turns": []}
        return _ledger_of(json.loads(path.read_text(encoding="utf-8")), path)

    def _judged(self, events: list[HistoryEvent]) -> list[DecisionPoint]:
        """Judge each entry by what the server recorded while that entry's harness runs ran.

        The harness's own start and exit bound a turn, because the record's order
        does not: an alert posted as a turn ends is kept before its assessment, and
        a failed turn's failure is kept beside the event it answers.
        """
        triggers = [event for event in events if event["kind"] in TRIGGERS]
        points = []
        for number, moment in enumerate(self.case.moments):
            runs = [harness_run for harness_run in self.runs if harness_run.moment_index == number]
            if number >= len(triggers) or not runs:
                msg = f"the server recorded no event, or the harness no run, for {moment.id}"
                raise ReplayError(msg)
            report = RunReport()
            for harness_run in runs:
                report.take(_lines_of(harness_run))
            until = next(
                (
                    harness_run.started_at
                    for harness_run in self.runs
                    if harness_run.moment_index > number
                ),
                None,
            )
            points.append(self._point(moment, triggers[number], events, runs, report, until))
        return points

    @staticmethod
    def _point(
        moment: Moment,
        trigger: HistoryEvent,
        events: list[HistoryEvent],
        runs: list[Run],
        report: RunReport,
        until: datetime | None,
    ) -> DecisionPoint:
        begun, ended = runs[0].started_at, runs[-1].ended_at or runs[-1].started_at
        during = [event for event in events if begun <= _received(event) <= ended]
        requests = []
        for event in during:
            if event["kind"] not in REQUESTED:
                continue
            action = event["payload"].get("action", {})
            if "agent" not in (action.get("actor") or {}):
                continue
            requests.append(Request(str(action.get("action", "")).replace("_", "-"), dict(action)))
        failures = [
            event["payload"].get("detail", "")
            for event in events
            if event["kind"] == FAILED
            and event["payload"].get("site") == FAILED_SITE
            and event["payload"].get("event_id") == trigger["id"]
        ]
        image = trigger.get("image") or {}
        return DecisionPoint(
            moment=moment,
            requests=requests,
            commands=report.commands,
            opened=report.opened,
            assessment=next(
                (
                    event["payload"].get("assessment")
                    for event in events
                    if event["kind"] == ASSESSED
                    and _received(event) >= ended
                    and (until is None or _received(event) < until)
                ),
                None,
            ),
            usage=report.usage,
            models=report.models,
            compacted=report.compacted,
            failure=failures[0] if failures else None,
            event_kind=trigger["kind"],
            image_sha256=image.get("sha256"),
            looks=sum(1 for event in during if event["kind"] == "camera_look"),
        )

    def stop(self) -> None:
        """Stop the server and every double."""
        self.timeline.stopped.set()
        if self.server is not None and self.server.poll() is None:
            self.server.terminate()
            try:
                self.server.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.server.kill()
                self.server.wait()
        for server in self.servers:
            server.shutdown()
            server.server_close()

    def clean_up(self) -> None:
        """Remove the install, once what a test reads from it has been read."""
        shutil.rmtree(self.root, ignore_errors=True)
