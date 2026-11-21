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
from typing import Any, Literal, NewType, NotRequired, Protocol, TextIO, TypedDict, cast, get_args

import doubles
from cases import REPO, Case, Moment, Request, instant, read_case
from jsonschema import Draft202012Validator
from repo_checks import platforms
from repo_checks.checks_real_prints import pointer
from repo_checks.docs import skill_prose
from repo_checks.model import Repo
from repo_checks.shell import run, start
from shell_lines import is_compound, program_commands, subcommands

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

#: Set in the server's environment, its looks wait in real time rather than
#: passing their waits on the replay's clock.
REAL_WAITS = "PRINTOBSERVER_REPLAY_REAL_WAITS"
#: What may be asked for, as the server's own schema declares it.
PRINT_ACTION = Draft202012Validator(
    json.loads((REPO / "schemas/printobserver-core/PrintAction.json").read_text(encoding="utf-8"))
)
#: The kind an agent's request is recorded under in its print's history. A recovery is
#: the printer's rather than the print's, and is no outcome any decision point names.
REQUESTED = {"action_requested"}
#: The kinds a turn ending is recorded under.
ASSESSED = "agent_assessment"
FAILED = "port_failure"
FAILED_SITE = "supervision_turn"
CAMERA_LOOK = "camera_look"
#: The event kinds a replay entry is delivered under.
TRIGGERS = {"periodic_observation", "obico_failure_alert"}
#: The requests that make a check an acting one rather than a quiet one: those that
#: change what the machine is doing. Every `set-*` request is one as well.
ACTING = {"pause", "cancel", "resume"}
#: What the server writes down when an answer of `stop` stopped nothing.
STOP_UNACTED = "agent_stop_unacted"
#: The command a check takes a fresh look with, which it never runs after acting.
LOOK = "look"
#: The heading the turn prompt carries the print's facts under.
FACTS_HEADING = "## The print's facts"
#: The heading the turn prompt writes its situation under.
SITUATION_HEADING = "## The situation when this turn began"


def acts(operation: str) -> bool:
    """Whether a request or a command by this name changes what the machine is doing."""
    return operation in ACTING or operation.startswith("set-")


#: The option naming the print a command is about.
PRINT_OPTION = "--print-id"


def looks_in(commands: list[str]) -> list[list[str]]:
    """Every `look` a command trace runs, each as the words after the program."""
    return [
        words for command in commands for named, words in program_commands(command) if named == LOOK
    ]


def names_a_print(words: list[str]) -> bool:
    """Whether a command's words name the print it is about."""
    return any(word == PRINT_OPTION or word.startswith(f"{PRINT_OPTION}=") for word in words)


def looked_after_acting(commands: list[str]) -> str | None:
    """The first `look` a command trace runs after a command that acts, as `<action>: <look>`."""
    acted = None
    for command in commands:
        for subcommand in subcommands(command):
            if acted is not None and subcommand == LOOK:
                return f"{acted}: {command}"
            if acts(subcommand):
                acted = subcommand
    return None


class ReplayError(Exception):
    """A replay that could not put the agent in front of its events."""


def program(name: str) -> Path:
    """A built program of this workspace."""
    suffix = platforms.host(Repo(REPO)).program.removeprefix("printobserver")
    target = Path(os.environ.get("CARGO_TARGET_DIR") or REPO / "target")
    return target / "skilltest" / f"{name}{suffix}"


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
    state: Path,
    listen: str,
    octoprint: str,
    camera: str,
    obico: str,
    minted: Secrets,
    *,
    interval_s: int = OBSERVATION_INTERVAL_S,
    calibration: Path | None = None,
) -> str:
    """The installer's configuration, with what an operator fills in filled in.

    `calibration` names the camera calibration `[camera] calibration` takes, when
    the case names one; with none the line is left out, as the installer leaves it.
    """
    verifier = hashlib.sha256(minted.operator.encode()).hexdigest()
    text = (
        _installer_template()
        .replace("$RUNTIME_STATE", state.as_posix())
        .replace("$INTERVAL_LINE", f"observation_interval_s = {interval_s}")
        .replace('listen = "127.0.0.1:8420"', f'listen = "{listen}"')
        .replace('url = "http://127.0.0.1:5000"', f'url = "{octoprint}"')
        .replace('api_key = ""', f'api_key = "{minted.octoprint_key}"')
        .replace('shared_secret = ""', f'shared_secret = "{minted.ingress}"')
    )
    text = f'api.credential_verifier = "sha256:{verifier}"\n{text}'
    text += (
        f'\n[camera]\nsnapshot_url = "{camera}{SNAPSHOT_PATH}"\n'
        + (f"calibration = {json.dumps(calibration.as_posix())}\n" if calibration else "")
        + f'\n[obico]\nurl = "{obico}"\naccess_token = "{minted.obico_token}"\n'
        f'ml_api_url = "{obico}/ml/"\nprinter_id = {doubles.OBICO_PRINTER}\n'
    )
    tomllib.loads(text)
    return text


#: The extrusion types the G-code index counts as supports, which lay no part.
SUPPORT_TYPES = (b"Support material", b"Support material interface")
#: The checking interval of the server started only to decide a print's alignment
#: before the first event, so that no check is due while it runs.
DECIDING_INTERVAL_S = 3600
#: How long the replay stands where the job passed its first layer for the printer
#: watch to decide the print's alignment there: past one of its reads, the installer's
#: fifteen-second poll, and the frame the decision fetches.
PASSING_WAIT_S = 25


def first_layer_end(gcode: bytes) -> int | None:
    """Where a slice's first layer laying the part ends: the next `;LAYER_CHANGE`.

    The rule the server's G-code index decides the first object layer by: the first
    layer extruding under a declared `;TYPE:` other than supports, its skirt and brim
    included. Nothing for a slice that lays no such layer, or none after it.
    """
    offset = 0
    kind = b""
    relative = True
    last_e = 0.0
    in_layer = False
    laid = False
    for line in gcode.splitlines(keepends=True):
        text = line.split(b";", 1)[0].strip()
        if line.startswith(b";LAYER_CHANGE"):
            if laid:
                return offset
            in_layer = True
        elif line.startswith(b";TYPE:"):
            kind = line[len(b";TYPE:") :].strip()
        elif text in {b"M83", b"M82"}:
            relative = text == b"M83"
        elif text.startswith(b"G92"):
            found = re.search(rb"\bE(-?[\d.]+)", text)
            last_e = float(found.group(1)) if found else last_e
        elif text[:3] in {b"G1 ", b"G2 ", b"G3 "}:
            found = re.search(rb"\bE(-?[\d.]+)", text)
            if found:
                value = float(found.group(1))
                extruded = value if relative else value - last_e
                last_e = last_e if relative else value
                if in_layer and extruded > 0 and kind and kind not in SUPPORT_TYPES:
                    laid = True
        offset += len(line)
    return None


#: Where on the camera double's address its frames are fetched from.
SNAPSHOT_PATH = "/api/frame.jpeg"


def calibration_copy(calibration: Path, camera: str, into: Path) -> Path:
    """A case's camera calibration, its stream the camera double's: where its frames come from.

    The calibration was fitted on the recording's own camera, which the double stands
    in for at an address of its own, so the copy names the stream the server's frames
    arrive on and is otherwise the committed file.
    """
    document = json.loads(calibration.read_text(encoding="utf-8"))
    if not isinstance(document, dict) or "camera_snapshot_url" not in document:
        msg = f"{calibration} is no camera calibration"
        raise ReplayError(msg)
    document["camera_snapshot_url"] = f"{camera}{SNAPSHOT_PATH}"
    copied = into / "camera-calibration.json"
    copied.write_text(json.dumps(document), encoding="utf-8")
    return copied


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


def keep_the_log(server: subprocess.Popen[str], log: Path) -> None:
    """Keep what the server prints, so neither pipe fills and it is there to read."""

    def kept(stream: TextIO | None) -> None:
        with log.open("a", encoding="utf-8") as written:
            for line in stream or ():
                written.write(line)
                written.flush()

    for stream in (server.stdout, server.stderr):
        threading.Thread(target=kept, args=(stream,), daemon=True).start()


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
        case "action_requested":
            return _is_requested_action(payload.get("action"))
        case "agent_assessment":
            assessment = payload.get("assessment")
            return assessment is not None and is_assessment(assessment)
        case _:
            return True


def _is_requested_action(action: object) -> bool:
    """Whether a request is one the server's committed `PrintAction` schema declares."""
    return PRINT_ACTION.is_valid(action)


def is_assessment(assessment: object) -> bool:
    """Whether a turn's assessment has every field the record carries; a turn may have none."""
    if assessment is None:
        return True
    return (
        isinstance(assessment, dict)
        and Assessment.__annotations__.keys() <= assessment.keys()
        and assessment["confidence"] in get_args(Confidence)
        and assessment["disposition"] in get_args(Disposition)
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
    digest = hashlib.sha256(source.read_bytes()).hexdigest()[:16]
    kept = root / "target" / "skilltest" / "harness" / f"{digest}-{source.name}"
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
Disposition = Literal["continue", "watch", "stop"]
#: Whether a check acted, which is what its cost is held to.
CheckClass = Literal["quiet", "acting"]


class CallUsage(TypedDict, total=False):
    """What one model call of a check read and wrote, as the supervisor port recorded it."""

    input_tokens: int
    output_tokens: int
    cache_read_tokens: int
    cache_write_tokens: int


class Assessment(TypedDict):
    """What the agent answered a turn with, as the server recorded it."""

    summary: str
    confidence: Confidence
    should_continue: bool
    did: str
    why: str
    escalating: bool
    disposition: Disposition


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
    #: Each model call the check made, in order, from the supervisor port's record.
    calls: list[CallUsage]
    check_class: CheckClass
    #: The bytes of the facts its prompt carried, when the transcript showed its prompt.
    facts_bytes: NotRequired[int | None]
    #: The image its prompt carried, when the transcript showed its prompt.
    image: NotRequired[CheckImage | None]
    #: How long its event waited for its turn, and what compaction ran meanwhile.
    wait: NotRequired[CheckWait | None]
    #: Whether an idle compaction of its session completed since the previous check.
    follows_compaction: NotRequired[bool]


class OutsideRecord(TypedDict):
    """One line an outside supervisor was handed, as `results/latest.json` carries it."""

    scenario: str
    at: str
    trigger: str
    milestone: str
    passed: bool
    why_not: str | None
    requests: list[str]
    commands: list[str]
    opened: list[str]
    assessment: None
    looks: int
    usage: dict[str, Any] | None
    models: list[str]
    compacted: bool
    session: str
    session_phase: str
    reply: str | None


#: How an idle compaction may end, in the port's words.
CompactionEnd = Literal["completed", "cancelled", "failed", "unsupported"]


class WaitedCompaction(TypedDict):
    """One idle compaction a check's event waited beside, as its record carries it."""

    ended: CompactionEnd
    started_at: str
    ended_at: str


class CheckWait(TypedDict):
    """From a check's event's arrival to its turn's start, on the server's clock."""

    arrived_at: str
    started_at: str | None
    wait_ms: int | None
    #: Each idle compaction of the session running between the two.
    compactions: list[WaitedCompaction]
    #: Whether its turn started only after one of them completed.
    delayed: bool
    #: How long, in real milliseconds, the scheduled check waited for the
    #: compaction just before its event to end, when it waited for one.
    waited_for_compaction_ms: int | None


#: Which image a check was shown.
ImageKind = Literal["composite", "whole_frame", "stored_frame"]

#: What the server's line names the three-panel check image by, and the whole frame by.
COMPOSITE_NOTE = "the three-panel check image"
WHOLE_FRAME_NOTE = "the whole camera frame"


class CheckImage(TypedDict):
    """Which image one check was shown, and why, as its prompt's situation said."""

    kind: ImageKind
    #: Why the whole frame rather than the three-panel image; none for the composite.
    why: str | None
    note: str


def check_image(note: str) -> CheckImage:
    """The image a prompt's situation line names: the composite, the whole frame and why."""
    if note.startswith(COMPOSITE_NOTE):
        return {"kind": "composite", "why": None, "note": note}
    if note.startswith(WHOLE_FRAME_NOTE):
        _, because, why = note.partition(", because ")
        return {"kind": "whole_frame", "why": why if because else None, "note": note}
    return {"kind": "stored_frame", "why": note, "note": note}


class Alignment(TypedDict):
    """One case's first-layer alignment verdict, as the server kept it."""

    verdict: str
    #: How far the printed first layer lay from its projection, when the verdict says.
    offset_px: float | None
    #: Why it failed, when it did for a reason other than its offset.
    failure: str | None


#: The schema the server keeps each print's alignment verdict to.
ALIGNMENT_SCHEMA = REPO / "schemas" / "printobserver-server" / "PrintAlignment.json"


def alignment_of(document: object) -> Alignment | None:
    """A verdict file's verdict, or nothing for a file that is no verdict.

    Held to the schema the server's own type writes, which its contract test keeps
    in step with the type, so a verdict the server writes differently is refused here
    rather than read as something it is not.
    """
    schema = json.loads(ALIGNMENT_SCHEMA.read_text(encoding="utf-8"))
    if not isinstance(document, dict) or not Draft202012Validator(schema).is_valid(document):
        return None
    reason = document.get("reason")
    offset = document.get("offset_px")
    if isinstance(reason, dict):
        offset = reason.get("offset_px", offset)
    return {
        "verdict": document["verdict"],
        "offset_px": float(offset) if isinstance(offset, int | float) else None,
        "failure": str(reason.get("failure")) if isinstance(reason, dict) else None,
    }


class TestRecord(TypedDict):
    """One test as `results/latest.json` carries it."""

    test: str
    passed: bool
    turns: int
    sessions: list[str]
    #: Each `printobserver` command a conversation ran beside another, by conversation.
    compound_commands: dict[str, list[str]]
    #: A conversation's point is an `OutsideRecord`, which its trigger tells apart.
    decision_points: list[DecisionRecord]
    #: Its print's first-layer alignment verdict, when the server decided one.
    alignment: NotRequired[Alignment | None]
    #: Every idle compaction of its print's sessions, from the port's ledger.
    compactions: NotRequired[list[LedgerCompaction]]
    #: The compaction settings its server was configured with.
    configured: NotRequired[Compaction | None]


class Compaction(TypedDict):
    """The two compaction settings a server is configured with, as its file has them."""

    idle_compact_tokens: int | None
    auto_compact_tokens: int | None


def installer_compaction() -> Compaction:
    """The compaction settings the installer's configuration template writes."""
    written = {
        name: int(value)
        for name, value in re.findall(
            r"^(idle_compact_tokens|auto_compact_tokens) = (\d+)$", _installer_template(), re.M
        )
    }
    return {
        "idle_compact_tokens": written.get("idle_compact_tokens"),
        "auto_compact_tokens": written.get("auto_compact_tokens"),
    }


def configured_compaction(config: Path) -> Compaction:
    """The compaction settings one server configuration file carries."""
    supervisor = tomllib.loads(config.read_text(encoding="utf-8")).get("supervisor", {})
    return {
        "idle_compact_tokens": supervisor.get("idle_compact_tokens"),
        "auto_compact_tokens": supervisor.get("auto_compact_tokens"),
    }


class LedgerSession(TypedDict):
    """One conversation of a print, as the supervisor port's ledger holds it."""

    session_name: str
    harness_identity: str


class LedgerTurn(TypedDict):
    """One turn of a print, as the supervisor port's ledger holds it."""

    session_name: str
    calls: NotRequired[list[CallUsage]]
    #: When it ended and when it started, on the server's clock.
    ran_at: NotRequired[str]
    started_at: NotRequired[str]


class LedgerCompaction(TypedDict):
    """One idle compaction of a print's session, as the supervisor port's ledger holds it."""

    session_name: str
    started_at: str
    ended_at: str
    ended: CompactionEnd
    size_before: int
    reported_before: NotRequired[int]
    reported_after: NotRequired[int]
    usage: CallUsage
    error: NotRequired[str]
    scheduled_check_waited_ms: NotRequired[int]


class Ledger(TypedDict):
    """What the supervisor port wrote down of a print."""

    sessions: list[LedgerSession]
    turns: list[LedgerTurn]
    compactions: list[LedgerCompaction]


#: Every end an idle compaction may have.
COMPACTION_ENDS = set(get_args(CompactionEnd))


def _is_compaction(entry: object) -> bool:
    """Whether one ledger entry is an idle compaction as the port writes one."""
    return (
        isinstance(entry, dict)
        and isinstance(entry.get("session_name"), str)
        and _is_timestamp(entry.get("started_at"))
        and _is_timestamp(entry.get("ended_at"))
        and entry.get("ended") in COMPACTION_ENDS
        and type(entry.get("size_before")) is int
        and all(
            type(entry[name]) is int and entry[name] >= 0
            for name in ("reported_before", "reported_after", "scheduled_check_waited_ms")
            if name in entry
        )
        and isinstance(entry.get("usage", {}), dict)
        and all(type(count) is int and count >= 0 for count in entry.get("usage", {}).values())
    )


def check_wait(
    arrived_at: str,
    turn: LedgerTurn | None,
    previous_end: str | None,
    compactions: list[LedgerCompaction],
) -> tuple[CheckWait, bool]:
    """How one check's event waited for its turn, and whether it follows a compaction.

    The turn starts once any compaction it cancelled has exited; a compaction
    running between the event's arrival and that start, which then completed, is
    one the check waited on. A scheduled check waits for a running compaction
    before its event is written down, so the wait it took is the one the port
    recorded on the compaction that ended just before its event arrived.
    """
    arrived = _utc_instant(arrived_at)
    started_at = turn.get("started_at") if turn is not None else None
    started = _utc_instant(started_at) if started_at is not None else None
    since = _utc_instant(previous_end) if previous_end is not None else None
    meanwhile: list[WaitedCompaction] = [
        {
            "ended": compaction["ended"],
            "started_at": compaction["started_at"],
            "ended_at": compaction["ended_at"],
        }
        for compaction in compactions
        if started is not None
        and _utc_instant(compaction["started_at"]) < started
        and _utc_instant(compaction["ended_at"]) > arrived
    ]
    before = [
        compaction
        for compaction in compactions
        if (since is None or _utc_instant(compaction["ended_at"]) > since)
        and _utc_instant(compaction["ended_at"]) <= (started or arrived)
    ]
    waited = [
        compaction["scheduled_check_waited_ms"]
        for compaction in before
        if "scheduled_check_waited_ms" in compaction
        and _utc_instant(compaction["ended_at"]) <= arrived
    ]
    wait: CheckWait = {
        "arrived_at": arrived_at,
        "started_at": started_at,
        "wait_ms": None
        if started is None
        else max(0, round((started - arrived).total_seconds() * 1000)),
        "compactions": meanwhile,
        "delayed": any(compaction["ended"] == "completed" for compaction in meanwhile),
        "waited_for_compaction_ms": waited[-1] if waited else None,
    }
    return wait, any(compaction["ended"] == "completed" for compaction in before)


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
    calls: list[CallUsage] = field(default_factory=list)
    facts_bytes: int | None = None
    image_note: str | None = None
    #: Whether the server wrote down that the turn answered `stop` and stopped nothing.
    stopped_nothing: bool = False
    #: When the server kept its event.
    arrived_at: str | None = None
    wait: CheckWait | None = None
    follows_compaction: bool = False

    @property
    def check_class(self) -> CheckClass:
        """Acting when the check asked for a pause, a cancel, a resume or any setting."""
        return "acting" if any(acts(request.operation) for request in self.requests) else "quiet"

    @property
    def why_not(self) -> str | None:
        """Why this decision point failed, or nothing when it passed."""
        if self.failure:
            return f"the turn failed: {self.failure}"
        reasons = [self.moment.verdict(self.requests)]
        if self.stopped_nothing:
            reasons.append("it answered stop and asked for no pause, so nothing stopped the print")
        looked = looked_after_acting(self.commands)
        if looked is not None:
            reasons.append(f"it looked again after acting ({looked})")
        return "; ".join(reason for reason in reasons if reason) or None

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
            "calls": self.calls,
            "check_class": self.check_class,
            "facts_bytes": self.facts_bytes,
            "image": check_image(self.image_note) if self.image_note is not None else None,
            "wait": self.wait,
            "follows_compaction": self.follows_compaction,
        }


@dataclass
class OutsidePoint:
    """One watch line an outside supervisor was handed, and what it did with it.

    It passes when the supervisor took a look of its own at the printer, naming no
    print, opened the frame the server stored for it, took no look naming the print
    — which would wait on what printobserver's agent is owed — and asked for nothing
    that changes what the machine is doing.
    """

    moment: Moment
    session: str
    #: `create` for the line opening the conversation, `continue` for one carrying it on.
    phase: str
    report: RunReport
    reply: str | None
    failure: str | None
    #: What the operator asked of the print while the conversation ran.
    requests: list[Request]
    #: The name of each frame the server stored while the conversation ran.
    stored: list[str]
    #: How many frames the camera answered while it ran.
    looks: int

    @property
    def why_not(self) -> str | None:
        """Why this line's decision failed, or nothing when it passed."""
        if self.failure:
            return f"the conversation failed: {self.failure}"
        reasons = [self.moment.verdict(self.requests)]
        reasons.extend(
            f"it asked for {request.describe()}, which changes what the machine is doing"
            for request in self.requests
            if acts(request.operation)
        )
        looked = looks_in(self.report.commands)
        if not any(not names_a_print(words) for words in looked):
            reasons.append("it took no look of its own at the printer, with no print named")
        named = [" ".join(words) for words in looked if names_a_print(words)]
        if named:
            reasons.append(
                f"it looked with the print named ({named[0]}), which waits on and takes what "
                "printobserver's agent is owed"
            )
        if looked and not set(self.report.opened) & set(self.stored):
            reasons.append(f"it opened none of the frames its looks named ({self.stored})")
        return "; ".join(reason for reason in reasons if reason) or None

    def as_record(self) -> OutsideRecord:
        """The line as `results/latest.json` carries it."""
        return {
            "scenario": self.moment.id,
            "at": self.moment.at,
            "trigger": self.moment.trigger,
            "milestone": str(self.moment.milestone),
            "passed": self.why_not is None,
            "why_not": self.why_not,
            "requests": [request.describe() for request in self.requests],
            "commands": self.report.commands,
            "opened": self.report.opened,
            "assessment": None,
            "looks": self.looks,
            "usage": self.report.usage,
            "models": self.report.models,
            "compacted": self.report.compacted,
            "session": self.session,
            "session_phase": self.phase,
            "reply": self.reply,
        }


class OutsideSupervisor(Protocol):
    """An outside supervisor's one conversation, beside printobserver's agent.

    It started the print and follows `printobserver watch`; each milestone hands it
    the watch's latest line, as the next message of the same conversation.
    """

    @property
    def session(self) -> str:
        """The conversation's name."""
        ...

    def supervise(self, moment: Moment, print_id: PrintId) -> OutsidePoint:
        """Hand it the watch's line at one milestone, and judge what it did."""
        ...

    def transcript(self) -> list[dict[str, Any]]:
        """Everything its harness runs wrote."""
        ...

    def stop(self) -> None:
        """Stop whatever it started."""
        ...


@dataclass
class Replayed:
    """A finished replay: its decision points, and what the port wrote down."""

    case: Case
    points: list[DecisionPoint]
    ledger: Ledger
    fetches: list[str]
    state: Path
    transcript: list[dict[str, Any]] = field(default_factory=list)
    alignment: Alignment | None = None
    #: The compaction settings the server was configured with.
    configured: Compaction | None = None
    #: Each watch line an outside supervisor was handed, in order.
    outside: list[OutsidePoint] = field(default_factory=list)
    #: The outside supervisor's conversation, when the case hands it any line.
    outside_session: str | None = None

    @property
    def passed(self) -> bool:
        """Whether every decision point passed, which is what passing the test is."""
        return all(point.why_not is None for point in [*self.points, *self.outside])

    def failures(self) -> str:
        """Each decision point that failed, for a test's message."""
        return "\n".join(
            [
                *(
                    f"{point.moment.id} ({point.moment.summary}): {point.why_not}\n"
                    f"  it requested: {[request.describe() for request in point.requests]}"
                    for point in self.points
                    if point.why_not
                ),
                *(
                    f"{point.moment.id} ({point.moment.summary}): {point.why_not}\n"
                    f"  it ran: {point.report.commands}\n  it said: {point.reply}"
                    for point in self.outside
                    if point.why_not
                ),
            ]
        )

    def compound_commands(self) -> dict[str, list[str]]:
        """Each `printobserver` command a conversation ran beside another, by conversation.

        A turn is its ledger entry's session's; with no entry to read, the print's last.
        """
        sessions = [session["session_name"] for session in self.ledger["sessions"]]
        found: dict[str, list[str]] = {session: [] for session in sessions}
        turns = self.ledger["turns"]
        for number, point in enumerate(self.points):
            session = (
                turns[number]["session_name"]
                if len(turns) == len(self.points)
                else (sessions[-1] if sessions else "")
            )
            found.setdefault(session, []).extend(
                command for command in point.commands if is_compound(command)
            )
        if self.outside_session is not None:
            found[self.outside_session] = [
                command
                for point in self.outside
                for command in point.report.commands
                if is_compound(command)
            ]
        return found

    def sessions(self) -> list[str]:
        """Each conversation of the replay: the print's, then the outside supervisor's."""
        found = [session["session_name"] for session in self.ledger["sessions"]]
        return [*found, self.outside_session] if self.outside_session is not None else found

    def decision_records(self) -> list[DecisionRecord]:
        """Every decision point as the record carries it, in the case's own replay order.

        A watch line's is an `OutsideRecord`, carrying no check's calls or class, which
        the record tells apart by its trigger as it does a start request's.
        """
        order = {moment.id: index for index, moment in enumerate(self.case.moments)}
        ranked: list[tuple[int, DecisionRecord]] = [
            *((order[point.moment.id], point.as_record()) for point in self.points),
            *(
                (order[point.moment.id], cast("DecisionRecord", point.as_record()))
                for point in self.outside
            ),
        ]
        return [record for _, record in sorted(ranked, key=lambda ranked: ranked[0])]

    def as_record(self) -> TestRecord:
        """The replay as `results/latest.json` carries it."""
        return {
            "test": self.case.name,
            "passed": self.passed,
            "turns": len(self.points) + len(self.outside),
            "sessions": self.sessions(),
            "compound_commands": self.compound_commands(),
            "decision_points": self.decision_records(),
            "alignment": self.alignment,
            "compactions": self.ledger["compactions"],
            "configured": self.configured,
        }


def _ledger_of(document: object, path: Path) -> Ledger:
    """A ledger read from the port's file, once it holds what is read from one."""
    sessions = document.get("sessions") if isinstance(document, dict) else None
    turns = document.get("turns") if isinstance(document, dict) else None
    compactions = document.get("compactions", []) if isinstance(document, dict) else None
    if (
        not isinstance(compactions, list)
        or not all(_is_compaction(entry) for entry in compactions)
        or not all(
            _is_timestamp(entry[name])
            for entry in turns or []
            if isinstance(entry, dict)
            for name in ("ran_at", "started_at")
            if name in entry
        )
    ):
        msg = f"{path} carries compactions or turn times the port does not write"
        raise ReplayError(msg)
    if (
        not isinstance(sessions, list)
        or not isinstance(turns, list)
        or not all(
            isinstance(entry, dict) and isinstance(entry.get("session_name"), str)
            for entry in [*sessions, *turns]
        )
        or not all(isinstance(entry.get("harness_identity"), str) for entry in sessions)
        or not all(_calls_are_counts(entry.get("calls", [])) for entry in turns)
    ):
        msg = f"{path} is no ledger of sessions and turns"
        raise ReplayError(msg)
    return {"sessions": sessions, "turns": turns, "compactions": compactions}


def _calls_are_counts(calls: object) -> bool:
    """Whether a ledger turn's calls are each a usage of whole, non-negative counts.

    Each reports its input and output, as the port writes every call the harness
    reported both of.
    """
    return isinstance(calls, list) and all(
        isinstance(call, dict)
        and all(name in call for name in REPORTED_COUNTS)
        and call.keys() <= CallUsage.__annotations__.keys()
        and all(type(count) is int and count >= 0 for count in call.values())
        for call in calls
    )


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


#: How much of what a failed turn's harness runs printed its record keeps.
HARNESS_SAID_CHARS = 1500

#: How much of a prints answer the replay could not read its refusal quotes.
PRINTS_SAID_CHARS = 2000


def _harness_said(runs: list[Run]) -> str:
    """The end of what a turn's harness runs printed, which is where a run says why it failed.

    The engine's own refusal of an answer names no cause when the harness printed
    no answer at all, so a failed turn's record carries the harness's own words.
    """
    said = " | ".join(
        harness_run.record.read_text("utf-8", "replace").strip() or "(nothing)"
        if harness_run.record.is_file()
        else "(no output kept)"
        for harness_run in runs
    )
    return said[-HARNESS_SAID_CHARS:]


def _lines_of(harness_run: Run) -> list[dict[str, Any]]:
    """Everything one harness run wrote: its transcript, then its result document."""
    record = harness_run.record
    answered = _json_lines(record.read_text("utf-8", "replace")) if record.is_file() else []
    return [*harness_run.transcript, *answered]


def _block_under(text: str, heading: str) -> str | None:
    """The JSON block a prompt carries under one heading, as written."""
    _, found, after = text.partition(heading)
    _, fence, rest = after.partition("```json\n")
    block, closed, _ = rest.partition("\n```")
    return block if found and fence and closed else None


def _image_line(situation: str | None) -> str | None:
    """The situation's line saying which image its prompt carried, when it says one."""
    if situation is None:
        return None
    try:
        document = json.loads(situation)
    except json.JSONDecodeError:
        return None
    line = document.get("image") if isinstance(document, dict) else None
    return line if isinstance(line, str) else None


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
    #: The bytes of the facts the turn's prompt carried, read off the prompt itself.
    facts_bytes: int | None = None
    #: The situation's line saying which image the turn's prompt carried.
    image_note: str | None = None

    def take(self, lines: list[dict[str, Any]]) -> None:
        """Read one run's output and transcript into this turn's report."""
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        for line in lines:
            match line:
                # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
                case {"type": "system", "subtype": "compact_boundary"}:
                    self.compacted = True
                # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
                case {"type": "result", "usage": dict(usage), **rest}:
                    self._add_usage(usage)
                    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
                    models = rest.get("modelUsage")
                    for model in models if isinstance(models, dict) else ():
                        self._saw(str(model))
                # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
                case {"message": {"model": str(model), "content": list(content)}}:
                    self._saw(model)
                    for block in content:
                        self._used(block)
                # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
                case {"type": "user", "message": {"content": list(content)}}:
                    for block in content:
                        self._prompted(block)
                case _:
                    pass

    def _used(self, block: object) -> None:
        """One tool call of the transcript, which writes a call once per streamed message."""
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        match block:
            # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
            case {"type": "tool_use", "id": str(called), "name": str(name), "input": dict(given)}:
                if called in self.seen_tool_uses:
                    return
                self.seen_tool_uses.add(called)
                match name, given:
                    case "Bash", {"command": str(command)}:
                        self.commands.append(command)
                    case "Read", {"file_path": str(path)}:
                        self.opened.append(Path(path).name)
                    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
                    case "Skill", {"skill": str(skill)}:
                        self.opened.append(f"skill:{skill}")
                    case _:
                        pass
            case _:
                pass

    def _prompted(self, block: object) -> None:
        """The facts and the image line one prompt carries, the first time it is seen."""
        match block:
            case {"type": "text", "text": str(text)} if self.facts_bytes is None:
                facts = _block_under(text, FACTS_HEADING)
                if facts is not None:
                    self.facts_bytes = len(facts.encode("utf-8"))
                    situation = _block_under(text, SITUATION_HEADING)
                    self.image_note = _image_line(situation)
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
        *,
        real_waits: bool = False,
        outside: Callable[[Replay], OutsideSupervisor] | None = None,
    ) -> None:
        """Lay the install out and start the doubles; nothing runs until [`Replay.run`].

        A look's wait passes on the replay's clock at once; `real_waits` has the
        server wait it out in real time instead, which is what a jumped replay is
        compared against. `outside` makes the outside supervisor a case's watch lines
        are handed to, once the install is laid out.
        """
        self.case = case
        self.make_outside = outside
        self.outside: OutsideSupervisor | None = None
        self.outside_points: list[OutsidePoint] = []
        self.real_waits = real_waits
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
        #: Each idle compaction's harness run, which serves no replay entry.
        self.compaction_runs: list[Run] = []
        self.errors: list[str] = []
        self.servers = [
            doubles.octoprint(self.timeline, self.minted.octoprint_key),
            doubles.camera(self.timeline),
            doubles.obico(self.timeline),
            doubles.control(
                self._harness_started,
                self._harness_exited,
                self.errors.append,
                compaction=(self._compaction_started, self._compaction_exited),
            ),
        ]
        self.port = _free_port()
        self.server: subprocess.Popen[str] | None = None
        self.print_id: PrintId | None = None
        #: The print's alignment verdict as last read, which the server deletes once
        #: the print closes.
        self.alignment: Alignment | None = None
        #: Where the slice's first layer laying the part ends, when it has one.
        self.first_layer_end = first_layer_end(gcode) if gcode is not None else None
        #: The case's calibration as the server is configured with it, once laid out.
        self.calibration: Path | None = None

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
        camera = doubles.address(self.servers[1])
        calibration = (
            calibration_copy(self.case.camera_calibration, camera, self.root)
            if self.case.camera_calibration is not None
            else None
        )
        self.calibration = calibration
        config = self.root / "config.toml"
        config.write_text(self._config_text(OBSERVATION_INTERVAL_S), encoding="utf-8")
        return config

    def _config_text(self, interval_s: int) -> str:
        """The server's configuration for this replay, checking every `interval_s` seconds."""
        return service_config(
            self.state,
            f"127.0.0.1:{self.port}",
            doubles.address(self.servers[0]),
            doubles.address(self.servers[1]),
            doubles.address(self.servers[2]),
            self.minted,
            interval_s=interval_s,
            calibration=self.calibration,
        )

    def _agents_before(self, index: int) -> int:
        """How many of the replay's entries before one are printobserver's agent's turns."""
        return sum(1 for moment in self.case.moments[:index] if not moment.is_outside)

    def _agent_turns(self) -> int:
        """How many of the replay's entries are printobserver's agent's turns."""
        return self._agents_before(len(self.case.moments))

    def _harness_started(self) -> str:
        """Note one harness run, and say where its output is to be kept."""
        if self._trigger_count() <= self._agents_before(self.timeline.index):
            self.timeline.harness_started()
        else:
            self.timeline.moved_at_exit = False
        record = self.records / f"{len(self.runs):04}.json"
        self.runs.append(
            Run(record, self.timeline.index, self._transcript_sizes(), self.timeline.now())
        )
        return str(record)

    def _compaction_started(self) -> str:
        """Note one compaction's harness run, which moves the replay nowhere."""
        record = self.records / f"compaction-{len(self.compaction_runs):04}.json"
        self.compaction_runs.append(
            Run(record, self.timeline.index, self._transcript_sizes(), self.timeline.now())
        )
        return str(record)

    def _compaction_exited(self) -> None:
        """Note that the compaction's harness exited; its end is the port's to record."""
        self.compaction_runs[-1].ended_at = self.timeline.now()

    def _transcripts(self) -> list[Path]:
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        return sorted(self.state.glob("harness/claude-code/projects/*/*.jsonl"))

    def _transcript_sizes(self) -> dict[Path, int]:
        return {path: path.stat().st_size for path in self._transcripts()}

    def _harness_exited(self) -> None:
        """Stand at the next event before the server can let its scheduled check fire."""
        run = self.runs[-1]
        run.ended_at = self.timeline.now()
        self._read_alignment()
        for path in self._transcripts():
            with path.open("rb") as transcript:
                transcript.seek(run.transcript_sizes.get(path, 0))
                run.transcript.extend(_json_lines(transcript.read().decode("utf-8", "replace")))
        try:
            self._next_event()
        except (ReplayError, OSError, subprocess.SubprocessError) as error:
            self.errors.append(str(error))

    def _first_layer_frame(self) -> doubles.Passing | None:
        """The frame the printer watch decided the print's alignment on, live.

        A live watch reads the job every poll and decides once the first layer laying
        the part is down, on the frame it takes then: the first frame the recording's
        supervisor took on its own schedule — one carrying the progress the printer
        reported — whose progress puts the job past that layer. Nothing for a case
        naming no calibration, or a slice laying no such layer.
        """
        ends = self.first_layer_end
        if self.case.camera_calibration is None or ends is None:
            return None
        for index, (frame, when) in enumerate(
            zip(self.case.frames, self.timeline.frame_times, strict=True)
        ):
            if frame.progress_pct is None:
                continue
            position = int(frame.progress_pct / 100 * self.case.gcode_size)
            if position > ends:
                return doubles.Passing(position, index, when)
        return None

    def _stand_passing(self, passing: doubles.Passing, short_of: Moment) -> None:
        """Stand at the passing frame, short of an event's position, until the server decides.

        The job stands no further than the event it comes before, and past the first
        layer, as it did: the server decides on its next read, or [`PASSING_WAIT_S`]
        passes.
        """
        ends = self.first_layer_end or 0
        position = min(passing.position, short_of.file_position_bytes)
        with self.timeline.lock:
            self.timeline.pass_at(passing._replace(position=max(position, ends + 1)))
        deadline = time.monotonic() + PASSING_WAIT_S
        while time.monotonic() < deadline and not any((self.state / "alignment").glob("*.json")):
            time.sleep(0.25)
        self._read_alignment()

    def _pass_the_first_layer(self, upcoming: Moment) -> None:
        """Let the printer watch see the job pass its first layer between two events.

        The replay moves the job from one event to the next; when the frame the live
        watch decided on falls after the event that ended and no later than the next,
        the replay first stands there — while the finished turn still holds the print,
        so no scheduled check can fire — and the next event then stands as it always
        does.
        """
        passing = self._first_layer_frame()
        if passing is None or self.alignment is not None:
            return
        if instant(self.timeline.moment.at) < passing.at <= instant(upcoming.at):
            self._stand_passing(passing, upcoming)

    def _decide_before_the_first_event(self) -> None:
        """Decide the alignment first when the first layer went down before the first event.

        The recording's watch decided it before the replay's first event, which no
        replayed turn stands at, so the server is started once with no check due, at
        the passing frame, until it decides; then stopped, its state kept, to start
        again at the first event. A print stays open, and its verdict with it, through
        a restart.
        """
        first = self.case.moments[0]
        passing = self._first_layer_frame()
        if passing is None or passing.at > instant(first.at):
            return
        config = self.root / "deciding.toml"
        config.write_text(self._config_text(DECIDING_INTERVAL_S), encoding="utf-8")
        self.server = self._started(config)
        try:
            self._wait_for_server()
            self._stand_passing(passing, first)
        finally:
            self.stop_server()
        with self.timeline.lock:
            self.timeline.stand_at(0)

    def _started(self, config: Path) -> subprocess.Popen[str]:
        """The server, started on one configuration, its output kept."""
        suffix = platforms.host(Repo(REPO)).program.removeprefix("printobserver")
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        server = start(
            [str(self.bin / f"printobserver{suffix}"), "server", "--config", str(config)],
            cwd=self.state,
            env={
                **neutral_environment(self.bin, self.home),
                **({REAL_WAITS: "1"} if self.real_waits else {}),
            },
        )
        self._keep_the_log(server)
        return server

    def _read_alignment(self) -> None:
        """Keep the print's alignment verdict while the server keeps it."""
        for path in sorted((self.state / "alignment").glob("*.json")):
            try:
                found = alignment_of(json.loads(path.read_text(encoding="utf-8")))
            except OSError, json.JSONDecodeError:
                continue
            if found is not None:
                self.alignment = found

    def _next_event(self) -> None:
        """Stand at the next turn's event, handing the outside supervisor each line before it.

        A line comes between two of printobserver's turns, so it is handed while the
        turn that ended still holds the print, and no scheduled check can fire before
        the conversation has answered it.
        """
        timeline = self.timeline
        exited_from = timeline.index
        following = timeline.index + 1
        while following < len(self.case.moments) and self.case.moments[following].is_outside:
            self._pass_the_first_layer(self.case.moments[following])
            with timeline.lock:
                timeline.stand_at(following)
            self._hand_the_line(self.case.moments[following])
            following += 1
        if following >= len(self.case.moments):
            with timeline.lock:
                timeline.finished = True
            return
        upcoming = self.case.moments[following]
        self._pass_the_first_layer(upcoming)
        if timeline.paused and upcoming.printer_state == "printing":
            self._operator_resumes()
        with timeline.lock:
            timeline.stand_at(following)
            timeline.moved_at_exit = True
            timeline.exited_from = exited_from
        if upcoming.is_alert:
            self._deliver_alert(upcoming)

    def _hand_the_line(self, moment: Moment) -> None:
        """Hand the outside supervisor the watch's line at one milestone, and keep its point."""
        if self.outside is None:
            msg = f"{moment.id} is a watch line, and this replay has no outside supervisor"
            raise ReplayError(msg)
        self.outside_points.append(self.outside.supervise(moment, self._print_id()))

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
        said = self._client("prints")
        answered = json.loads(said)
        prints = answered.get("prints") if isinstance(answered, dict) else None
        if not isinstance(prints, list) or not all(map(is_held_print, prints)):
            msg = f"the server's prints answered no list of prints: {said[:PRINTS_SAID_CHARS]}"
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

    def history(self) -> list[HistoryEvent]:
        """The print's whole history, oldest first, as the operator reads it."""
        return self._events()

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
        keep_the_log(server, self.root / "server.log")

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
        wanted = self._agent_turns()
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
            # llmlint: ignore[changed_behavior_has_e2e] See suppressions.toml.
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
        if self.case.moments[0].is_outside:
            msg = f"{self.case.moments[0].id} is a watch line before printobserver's first verdict"
            raise ReplayError(msg)
        config = self._install()
        if any(moment.is_outside for moment in self.case.moments):
            if self.make_outside is None:
                msg = f"{self.case.name} hands watch lines, and this replay supervises from nowhere"
                raise ReplayError(msg)
            self.outside = self.make_outside(self)
        self._decide_before_the_first_event()
        self.server = self._started(config)
        try:
            self._wait_for_server()
            first = self.case.moments[0]
            if first.is_alert:
                self._post_alert(first)
            self._wait_for_every_turn()
            self._read_alignment()
            events = self._events()
            points = self._judged(events)
            ledger = self._ledger()
            if len(ledger["turns"]) == len(points):
                previous_end: str | None = None
                for point, turn in zip(points, ledger["turns"], strict=True):
                    point.calls = list(turn.get("calls", []))
                    if point.arrived_at is not None:
                        point.wait, point.follows_compaction = check_wait(
                            point.arrived_at, turn, previous_end, ledger["compactions"]
                        )
                    previous_end = turn.get("ran_at")
            return Replayed(
                case=self.case,
                points=points,
                ledger=ledger,
                fetches=list(self.timeline.fetches),
                state=self.state,
                transcript=[
                    *(line for harness_run in self.runs for line in _lines_of(harness_run)),
                    *(self.outside.transcript() if self.outside is not None else []),
                ],
                alignment=self.alignment,
                configured=configured_compaction(config),
                outside=list(self.outside_points),
                outside_session=self.outside.session if self.outside is not None else None,
            )
        finally:
            self.stop()

    def _ledger(self) -> Ledger:
        path = self.state / "supervisor-sessions" / f"{self._print_id()}.json"
        if not path.is_file():
            return {"sessions": [], "turns": [], "compactions": []}
        return _ledger_of(json.loads(path.read_text(encoding="utf-8")), path)

    def _judged(self, events: list[HistoryEvent]) -> list[DecisionPoint]:
        """Judge each entry by what the server recorded while that entry's harness runs ran.

        The harness's own start and exit bound a turn, because the record's order
        does not: an alert posted as a turn ends is kept before its assessment, and
        a failed turn's failure is kept beside the event it answers.
        """
        triggers = [event for event in events if event["kind"] in TRIGGERS]
        points = []
        turns = [
            (index, moment)
            for index, moment in enumerate(self.case.moments)
            if not moment.is_outside
        ]
        for number, (index, moment) in enumerate(turns):
            runs = [harness_run for harness_run in self.runs if harness_run.moment_index == index]
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
                    if harness_run.moment_index > index
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
        # A turn that acknowledged nothing by command is acknowledged from its
        # answer once its harness has exited, before the next turn can start.
        answered = [
            event
            for event in events
            if begun <= _received(event) and (until is None or _received(event) < until)
        ]
        requests = []
        for event in answered:
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
            failure=f"{failures[0]}; the harness answered: {_harness_said(runs)}"
            if failures
            else None,
            event_kind=trigger["kind"],
            image_sha256=image.get("sha256"),
            looks=sum(1 for event in during if event["kind"] == CAMERA_LOOK),
            facts_bytes=report.facts_bytes,
            image_note=report.image_note,
            stopped_nothing=any(event["kind"] == STOP_UNACTED for event in answered),
            arrived_at=trigger["received_at"],
        )

    def stop(self) -> None:
        """Stop the server, the outside supervisor and every double."""
        self.timeline.stopped.set()
        self.stop_server()
        if self.outside is not None:
            self.outside.stop()
        for server in self.servers:
            server.shutdown()
            server.server_close()

    def stop_server(self) -> None:
        """Stop the server, leaving the doubles and the state as they are."""
        if self.server is not None and self.server.poll() is None:
            self.server.terminate()
            try:
                self.server.wait(timeout=30)
            except subprocess.TimeoutExpired:
                # llmlint: ignore[changed_behavior_has_e2e] See suppressions.toml.
                self.server.kill()
                self.server.wait()

    def clean_up(self) -> None:
        """Remove the install, once what a test reads from it has been read."""
        shutil.rmtree(self.root, ignore_errors=True)


#: What the probe of the shipped supervisor configuration is prompted with.
PROBE_PROMPT = "Answer with the assessment for a print that is printing normally."
PROBE_DEADLINE_S = 600
#: The file a run's probe record is kept in, beside its tests' own.
PROBE_RECORD = "probe.json"


class ProbeRecord(TypedDict):
    """One probe turn as `results/latest.json` carries it."""

    prompt: str
    #: Each model call the probe made, in order, as the engine reported them.
    calls: list[CallUsage]
    #: The bytes of the skill's prose, which every call carries as its system prompt.
    skill_bytes: int


def skill_prose_bytes() -> int:
    """The bytes of the committed skill after its frontmatter: what a turn's system prompt is.

    Split by `repo_checks.docs.skill_prose`, which is the adapter's own split.
    """
    prose = skill_prose((SKILL / "SKILL.md").read_text(encoding="utf-8")).prose
    return len(prose.encode("utf-8"))


#: The counts a call's usage must report: what it read outside the cache and what it wrote.
REPORTED_COUNTS = ("input_tokens", "output_tokens")


def _call_usage(usage: object) -> CallUsage:
    """One call's usage as the engine reports it, its token counts alone.

    A count it reports must be a whole, non-negative number, and it must report
    [`REPORTED_COUNTS`]: a call that says less is no measurement of what it read.
    """
    if not isinstance(usage, dict) or not all(
        type(usage.get(name)) is int for name in REPORTED_COUNTS
    ):
        msg = f"the probe reported a call with no input and output counts: {usage!r}"
        raise ReplayError(msg)
    counted: CallUsage = {}
    for name in ("input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens"):
        value = usage.get(name)
        if value is None:
            continue
        if type(value) is not int or value < 0:
            msg = f"the probe reported {name} as {value!r}, which is no count"
            raise ReplayError(msg)
        counted[name] = value
    return counted


def _probe_turn(harness: str, root: Path, image: Path | None) -> list[CallUsage]:
    """One probe turn through the shipped configuration laid out under `root`.

    The installer's configuration, the installed skill and the shipped harness, as a
    replay lays them out, and the server's own composition of its supervisor port —
    handed the program's usage lines as `printobserver server` hands them — prompted
    with [`PROBE_PROMPT`], with `image` attached as a check's image is, or none.
    """
    state, home, bin_directory = root / "state", root / "home", root / "bin"
    for directory in (state, home, bin_directory):
        directory.mkdir(exist_ok=True)
    suffix = platforms.host(Repo(REPO)).program.removeprefix("printobserver")
    shutil.copy2(harness, bin_directory / f"claude{suffix}")
    shutil.copytree(SKILL, state / "skills" / "printobserver", dirs_exist_ok=True)
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run([str(program("printobserver")), "--help"], timeout=CLIENT_DEADLINE_S)
    usage = root / "usage.txt"
    usage.write_text(
        "".join(
            f"{line.strip()}\n"
            for line in done.stdout.splitlines()
            if line.strip().startswith("printobserver ")
        ),
        encoding="utf-8",
    )
    config = root / "config.toml"
    config.write_text(
        service_config(state, "127.0.0.1:0", UNUSED, UNUSED, UNUSED, Secrets()),
        encoding="utf-8",
    )
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run(
        [
            str(program("printobserver-server-probe")),
            "--config",
            str(config),
            "--session",
            secrets.token_hex(8),
            "--prompt",
            PROBE_PROMPT,
            "--usage",
            str(usage),
            *(["--image", str(image)] if image is not None else []),
        ],
        cwd=state,
        env=neutral_environment(bin_directory, home),
        timeout=PROBE_DEADLINE_S,
    )
    if done.returncode != 0:
        msg = f"the probe answered {done.returncode}: {done.stderr}"
        raise ReplayError(msg)
    result = json.loads(done.stdout)
    calls = result.get("calls") if isinstance(result, dict) else None
    if not isinstance(calls, list) or not calls:
        msg = f"the probe reported no calls: {done.stdout[:PRINTS_SAID_CHARS]}"
        raise ReplayError(msg)
    return [_call_usage(call) for call in calls]


#: An address no double answers, for a probe that reaches no printer, camera or Obico.
UNUSED = "http://127.0.0.1:1"


def probe(harness: str) -> ProbeRecord:
    """One turn through the shipped supervisor configuration alone, in a fresh session.

    Prompted with [`PROBE_PROMPT`] and no image: what every call of a check costs
    before the check adds to it.
    """
    root = Path(tempfile.mkdtemp())
    try:
        return {
            "prompt": PROBE_PROMPT,
            "calls": _probe_turn(harness, root, None),
            "skill_bytes": skill_prose_bytes(),
        }
    finally:
        shutil.rmtree(root, ignore_errors=True)


#: The file a run's image probe record is kept in, beside its tests' own.
IMAGE_PROBE_RECORD = "image-probe.json"
#: The case and decision point whose frame the image probe composes its check image of.
IMAGE_PROBE_CASE = "jar-top-petg-healthy"
IMAGE_PROBE_SCENARIO = "carry-on-031-other"


class ImageProbeRecord(TypedDict):
    """The image probe as `results/latest.json` carries it."""

    prompt: str
    calls: list[CallUsage]
    case: str
    scenario: str
    #: The check image it attached: its size and its bytes.
    width: int
    height: int
    bytes: int


class Composed(TypedDict):
    """What `printobserver-server-compose` said of the check image it wrote."""

    width: int
    height: int
    bytes: int
    #: The line a check shown it carries in its situation.
    note: str


def composed_check_image(
    gcode: bytes, frame: Path, position: int, calibration: Path, out: Path
) -> Composed:
    """The check image a print's first check at `position` is shown, written to `out`.

    Composed by `printobserver-server-compose` — the server's own composition — of
    `frame` at that file position in `gcode`, through `calibration`, configured as the
    installer configures the server.
    """
    root = out.parent / f"{out.stem}-compose"
    root.mkdir()
    printed = root / "print.gcode"
    printed.write_bytes(gcode)
    copied = calibration_copy(calibration, UNUSED, root)
    # The configuration is loaded as the server loads it, which reads the skill.
    state = root / "state"
    shutil.copytree(SKILL, state / "skills" / "printobserver")
    config = root / "compose.toml"
    config.write_text(
        service_config(state, "127.0.0.1:0", UNUSED, UNUSED, UNUSED, Secrets(), calibration=copied),
        encoding="utf-8",
    )
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run(
        [
            str(program("printobserver-server-compose")),
            "--config",
            str(config),
            "--gcode",
            str(printed),
            "--frame",
            str(frame),
            "--position",
            str(position),
            "--out",
            str(out),
        ],
        timeout=CLIENT_DEADLINE_S,
    )
    shutil.rmtree(root, ignore_errors=True)
    if done.returncode != 0:
        msg = f"the check image could not be composed: {done.stderr}"
        raise ReplayError(msg)
    said = json.loads(done.stdout)
    if (
        not isinstance(said, dict)
        or not all(type(said.get(name)) is int for name in ("width", "height", "bytes"))
        or not isinstance(said.get("note"), str)
    ):
        msg = f"the composer said no size: {done.stdout[:PRINTS_SAID_CHARS]}"
        raise ReplayError(msg)
    return said


def image_probe(harness: str) -> ImageProbeRecord:
    """The text-only probe's turn with one case's three-panel check image attached.

    The image is composed by `printobserver-server-compose` — the server's own
    composition — of [`IMAGE_PROBE_CASE`]'s frame at [`IMAGE_PROBE_SCENARIO`], at its
    file position in the case's G-code, through the case's camera calibration, at the
    size every check is shown it. What one check image adds to a call is this probe's
    first-call input less the text-only probe's.
    """
    case = read_case(IMAGE_PROBE_CASE)
    moment = next(moment for moment in case.moments if moment.id == IMAGE_PROBE_SCENARIO)
    if case.camera_calibration is None:
        msg = f"{case.name} names no camera calibration to compose its check image through"
        raise ReplayError(msg)
    root = Path(tempfile.mkdtemp())
    try:
        composite = root / "check-image.jpg"
        size = composed_check_image(
            refuse_a_pointer(case.gcode.read_bytes(), case.name, case.gcode.name),
            case.directory / moment.event_image,
            moment.file_position_bytes,
            case.camera_calibration,
            composite,
        )
        return {
            "prompt": PROBE_PROMPT,
            "calls": _probe_turn(harness, root, composite),
            "case": case.name,
            "scenario": moment.id,
            "width": size["width"],
            "height": size["height"],
            "bytes": size["bytes"],
        }
    finally:
        shutil.rmtree(root, ignore_errors=True)
