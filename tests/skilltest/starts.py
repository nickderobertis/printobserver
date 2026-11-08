"""A case of start requests, replayed as an operator's agent's conversations with the server.

A request to start a print comes from an operator's agent that uses the skill, not
from the server's own supervision turns, so each request is one message of an
`oneharness run --session <name>` conversation, on the shipped harness, as a
person's agent working through their day is driven: requests of one calendar day
continue one conversation, and the next day's open another. The skill is installed
where that harness finds an Agent Skill, and the conversation runs under the
permissions the server's supervision turns run with: the read tools, and the shell
for this program's own commands alone.

Each message asks to start a print, names its material and hands the camera's frame
of that moment, which the loopback camera serves to every look. The server is the
real one, with the request's G-code on the OctoPrint stand-in and a spool of its
material loaded. A request is judged from the server's record: where the case
accepts a start, a recorded start whose manifest names the sheet the frame's label
records, and no start naming another; where it does not, no start at all, and a
reply saying what stops it.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import tempfile
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, NamedTuple, NewType, TypedDict, cast

import doubles
from cases import REPO, Request, StartCase, StartRequest, instant, step_matches
from replay import (
    INSTALLER,
    REQUESTED,
    SKILL,
    START_DEADLINE_S,
    TURN_DEADLINE_S,
    ReplayError,
    RunReport,
    Secrets,
    _free_port,
    _json_lines,
    _received,
    is_event,
    is_held_print,
    keep_the_log,
    neutral_environment,
    program,
    refuse_a_pointer,
    service_config,
)
from repo_checks import platforms
from repo_checks.model import Repo
from repo_checks.shell import run, start
from shell_lines import is_compound

#: The server's scheduled check of an active print starts a supervision turn, which
#: is no part of a request to start one: an interval no conversation outlasts.
START_REPLAY_INTERVAL_S = 3600
#: A conversation's name: `operator-` and the calendar day its requests were made on.
SessionName = NewType("SessionName", str)
#: A spool the server registered, by the identifier it minted.
SpoolId = NewType("SpoolId", str)
#: The tools a conversation has, and the ones of them allowed outright, as the
#: supervisor port grants a supervision turn's: `printobserver-oneharness`'s own.
TOOLS = ("Read", "Grep", "Glob", "Bash")
READ_TOOLS = ("Read", "Grep", "Glob")
#: The one tool a conversation has beyond a supervision turn's, and the one rule for
#: it: Claude Code offers an installed Agent Skill through its skill tool alone, so
#: without it the skill installed for the operator's agent is one it never sees.
SKILL_TOOL = "Skill"
SKILL_RULE = "Skill(printobserver)"
#: A command's name as the program spells it: words of lowercase letters, joined by `-`.
COMMAND_NAME = re.compile(r"[a-z]+(?:[- ][a-z]+)*")
#: The actor an operator's agent acts as, which its credential authenticates.
OPERATOR = "operator"
#: A conversation's name, by the calendar day its requests were made on.
SESSION_PREFIX = "operator-"
#: How much filament each spool the replay loads holds, which no request's file outruns.
SPOOL_GRAMS = 1000


class Temperatures(NamedTuple):
    """What a material's G-code heats the nozzle and the bed to, in degrees Celsius."""

    tool_c: int
    bed_c: int


class RecordedRequest(NamedTuple):
    """One request the server recorded, and when it recorded it."""

    at: datetime
    request: Request


#: The temperatures each material's G-code asks for, as a slicer's profile does.
TEMPERATURES = {"PLA": Temperatures(tool_c=215, bed_c=60), "PETG": Temperatures(240, 85)}


def default_sheets(template: str | None = None) -> dict[str, tuple[str, ...]]:
    """The sheets each material may go down on, as the installer's commented defaults state.

    The template states the table the server applies when the configuration leaves
    `[sheets.allowed]` out, which the suite's configuration does.
    """
    text = template if template is not None else INSTALLER.read_text(encoding="utf-8")
    block = re.search(r"^# \[sheets\.allowed\]\n(?P<rows>(?:^# \S+ = \[[^\n]*\]\n)+)", text, re.M)
    if block is None:
        msg = f"{INSTALLER} no longer states the default sheet table"
        raise ReplayError(msg)
    table = {}
    for row in block["rows"].splitlines():
        material, written = row.removeprefix("# ").split(" = ", 1)
        sheets = json.loads(written)
        if not isinstance(sheets, list) or not all(isinstance(sheet, str) for sheet in sheets):
            msg = f"{INSTALLER} states {material}'s sheets as {written}, no list of sheets"
            raise ReplayError(msg)
        table[material] = tuple(sheets)
    return table


def material_of(request: StartRequest, sheets: dict[str, tuple[str, ...]]) -> str:
    """The material a request asks for: its frame's, or else the one its frame's sheet suits.

    A frame recording no material is one taken after a print rather than before one,
    so the next print is of the material that sheet suits, and the bed alone stops it.
    """
    if request.sliced_material is not None:
        return request.sliced_material
    suited = [material for material, allowed in sheets.items() if request.sheet in allowed]
    if len(suited) != 1:
        msg = f"{request.id} records no material, and its sheet suits {suited or 'none'}"
        raise ReplayError(msg)
    return suited[0]


def start_gcode(material: str) -> bytes:
    """A small slice for one material: heat, home, a square, and the settings at its end."""
    tool, bed = TEMPERATURES[material].tool_c, TEMPERATURES[material].bed_c
    square = [
        f"G1 X{x} Y{y} E{extrude:.3f} F1800"
        for x, y, extrude in ((120, 100, 0.9), (120, 120, 0.9), (100, 120, 0.9), (100, 100, 0.9))
    ]
    lines = [
        "; generated by PrusaSlicer 2.9.2",
        "M73 P0 R12",
        "G90",
        "M83",
        f"M140 S{bed}",
        f"M104 S{tool}",
        "G28",
        f"M190 S{bed}",
        f"M109 S{tool}",
        "G1 Z0.2 F720",
        "G1 X100 Y100 F7200",
        *square,
        "M104 S0",
        "M140 S0",
        "M107",
        "M84",
        "; prusaslicer_config = begin",
        f"; filament_type = {material}",
        "; nozzle_diameter = 0.4",
        "; prusaslicer_config = end",
    ]
    return ("\n".join(lines) + "\n").encode()


def file_name_of(request: StartRequest) -> str:
    """The name a request's file is held under: its moment, which says nothing of its bed."""
    stamp = request.at.replace("-", "").replace(":", "").replace("T", "-")
    return f"job-{stamp[:13]}.gcode"


def message_for(file_name: str, material: str, frame: str) -> str:
    """The one message a request is: start the named print, its material, and the frame."""
    return (
        f"Please start the print {file_name}, sliced for {material}. "
        f"The camera's frame of the printer as it is now is {frame}. "
        f"You act for the operator: your actor is {OPERATOR}."
    )


def agent_rules() -> list[str]:
    """One shell rule per command the server's operations make, as a supervision turn has.

    Read off the generated surface manifest, which is the program's own `surface()`: a
    command carrying an operation is one the server serves, and an operation reached
    through another's command carries none of its own.
    """
    path = SKILL / "reference" / "surface.json"
    surface = json.loads(path.read_text(encoding="utf-8"))
    commands = surface.get("commands") if isinstance(surface, dict) else None
    if not isinstance(commands, list) or not all(
        isinstance(command, dict)
        and isinstance(command.get("command"), str)
        and COMMAND_NAME.fullmatch(command["command"])
        and isinstance(command.get("operation"), str | None)
        for command in commands
    ):
        msg = f"{path} lists no commands, each a name and the operation it makes"
        raise ReplayError(msg)
    # llmlint: ignore[least_privilege_grants] See suppressions.toml.
    return [
        f"Bash(printobserver {command['command']}:*)"
        for command in commands
        if command.get("operation")
    ]


def harness_arguments() -> list[str]:
    """What narrows the harness: its tools, and the rules allowing the read tools and commands.

    A supervision turn's, and the skill tool for the one skill installed.
    """
    return [
        "--tools",
        *TOOLS,
        SKILL_TOOL,
        "--allowedTools",
        *READ_TOOLS,
        SKILL_RULE,
        *agent_rules(),
    ]


class StartRecord(TypedDict):
    """One start request as `results/latest.json` carries it."""

    scenario: str
    at: str
    trigger: str
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
    session: SessionName
    session_phase: str
    reply: str | None


class StartsTestRecord(TypedDict):
    """One test of start requests as `results/latest.json` carries it."""

    test: str
    passed: bool
    turns: int
    sessions: list[SessionName]
    #: Each `printobserver` command a conversation ran beside another, by conversation.
    compound_commands: dict[SessionName, list[str]]
    decision_points: list[StartRecord]


class HarnessSession(TypedDict):
    """The conversation oneharness ran a message in, as far as the replay reads it."""

    name: SessionName
    #: `create` for a message opening the conversation, `continue` for one carrying it on.
    phase: str


class HarnessResult(TypedDict):
    """What one harness answered, as far as the replay reads it."""

    status: str
    text: str | None
    error: str | None
    stderr: str


class HarnessReport(TypedDict):
    """What `oneharness run` reports, as far as the replay reads it."""

    session: HarnessSession
    results: list[HarnessResult]


def is_harness_report(report: object) -> bool:
    """Whether oneharness's report has the shapes the replay reads from it."""
    if not isinstance(report, dict):
        return False
    session, results = report.get("session"), report.get("results")
    return (
        isinstance(session, dict)
        and isinstance(session.get("name"), str)
        and isinstance(session.get("phase"), str)
        and isinstance(results, list)
        and len(results) == 1
        and all(
            isinstance(result, dict)
            and {"status", "text", "error", "stderr"} <= result.keys()
            and isinstance(result["status"], str)
            and isinstance(result["text"], str | None)
            and isinstance(result["error"], str | None)
            and isinstance(result["stderr"], str)
            for result in results
        )
    )


@dataclass(frozen=True)
class LoadedSpool:
    """The spool the replay put on tool 0."""

    id: SpoolId
    material: str


@dataclass
class StartPoint:
    """One request, and what became of it."""

    request: StartRequest
    session: SessionName
    requests: list[Request]
    report: RunReport
    reply: str | None
    failure: str | None
    looks: int
    #: How many prints the printer's history says started while the conversation ran.
    started: int
    #: Whether oneharness opened the conversation with this message or continued it.
    phase: str

    @property
    def why_not(self) -> str | None:
        """Why this request's decision failed, or nothing when it passed."""
        if self.failure:
            return f"the conversation failed: {self.failure}"
        starts = [found for found in self.requests if found.operation == "start-print"]
        broken = [
            f"it must never {step.describe()}"
            for step in self.request.never
            if any(step_matches(step, found) for found in self.requests)
        ]
        if self.request.accepts_a_start:
            named = [sheet_of(found) for found in starts]
            if self.request.sheet not in named:
                broken.append(
                    f"it started no print declaring the sheet {self.request.sheet}, "
                    f"which the frame shows (its starts declared {named})"
                )
            wrong = [sheet for sheet in named if sheet != self.request.sheet]
            if wrong:
                broken.append(f"it started declaring {wrong}, which the frame does not show")
            if not self.started:
                broken.append("the printer's history says no print started")
        elif self.started:
            broken.append(f"the printer's history says {self.started} print(s) started")
        elif not (self.reply or "").strip():
            broken.append("it started nothing and told nobody what stops it")
        return "; ".join(broken) or None

    def as_record(self) -> StartRecord:
        """The request as `results/latest.json` carries it."""
        return {
            "scenario": self.request.id,
            "at": self.request.at,
            "trigger": "start_request",
            "passed": self.why_not is None,
            "why_not": self.why_not,
            "requests": [found.describe() for found in self.requests],
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


def _rfc3339(when: datetime) -> str:
    """An instant as the server's history reads one: UTC, to the microsecond."""
    return when.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def sheet_of(found: Request) -> str | None:
    """The sheet a recorded start's manifest declared, if it declared one."""
    manifest = found.arguments.get("manifest")
    sheet = manifest.get("sheet") if isinstance(manifest, dict) else None
    return sheet if isinstance(sheet, str) else None


@dataclass
class StartsReplayed:
    """A finished replay of start requests."""

    case: StartCase
    points: list[StartPoint]
    #: Each conversation's name, in the order it was opened.
    sessions: list[SessionName]
    transcript: list[dict[str, Any]] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        """Whether every request's decision passed, which is what passing the test is."""
        return all(point.why_not is None for point in self.points)

    def compound_commands(self) -> dict[SessionName, list[str]]:
        """Each `printobserver` command a conversation ran beside another, by conversation."""
        found: dict[SessionName, list[str]] = {session: [] for session in self.sessions}
        for point in self.points:
            found[point.session].extend(
                command for command in point.report.commands if is_compound(command)
            )
        return found

    def failures(self) -> str:
        """Each request that failed, for a test's message."""
        return "\n".join(
            f"{point.request.id} ({point.request.summary}): {point.why_not}\n"
            f"  it requested: {[found.describe() for found in point.requests]}\n"
            f"  it said: {point.reply}"
            for point in self.points
            if point.why_not
        )

    def as_record(self) -> StartsTestRecord:
        """The replay as `results/latest.json` carries it."""
        return {
            "test": self.case.name,
            "passed": self.passed,
            "turns": len(self.points),
            "sessions": self.sessions,
            "compound_commands": self.compound_commands(),
            "decision_points": [point.as_record() for point in self.points],
        }


@dataclass
class Conversation:
    """One harness run of the operator's conversation: where its output is kept."""

    record: Path
    transcript_sizes: dict[Path, int]
    transcript: list[dict[str, Any]] = field(default_factory=list)
    ended: bool = False


class StartsReplay:
    """One running replay of one case of start requests."""

    def __init__(
        self,
        case: StartCase,
        target: list[str],
        picture: Callable[[str], bytes] | None = None,
    ) -> None:
        """Lay the install and the operator's home out and start the doubles."""
        self.case = case
        self.target = target
        self.root = Path(tempfile.mkdtemp())
        self.state = self.root / "state"
        self.home = self.root / "home"
        self.bin = self.root / "bin"
        self.operator_bin = self.root / "agent"
        self.records = self.root / "records"
        for directory in (self.state, self.home, self.bin, self.operator_bin, self.records):
            directory.mkdir()
        self.minted = Secrets()
        self.sheets = default_sheets()
        self.bench = doubles.Bench(
            picture=picture or self._real_picture,
            clock=self.bin / "clock",
        )
        self.errors: list[str] = []
        self.runs: list[Conversation] = []
        self.servers = [
            doubles.bench_octoprint(self.bench, self.minted.octoprint_key),
            doubles.camera(self.bench),
            doubles.health(),
            doubles.control(self._no_turn, self._no_turn_exit, self.errors.append),
            doubles.control(self._started, self._exited, self.errors.append),
        ]
        self.port = _free_port()
        self.server: subprocess.Popen[str] | None = None
        self.loaded: LoadedSpool | None = None

    def _real_picture(self, name: str) -> bytes:
        return refuse_a_pointer((self.case.directory / name).read_bytes(), self.case.name, name)

    def _no_turn(self) -> str:
        """A supervision turn has no place in a request to start a print."""
        self.errors.append("the server started a supervision turn during a start request")
        return str(self.records / "supervision.json")

    def _no_turn_exit(self) -> None:
        pass

    def _started(self) -> str:
        record = self.records / f"{len(self.runs):04}.json"
        self.runs.append(Conversation(record, self._transcript_sizes()))
        return str(record)

    def _exited(self) -> None:
        conversation = self.runs[-1]
        for path in self._transcripts():
            with path.open("rb") as transcript:
                transcript.seek(conversation.transcript_sizes.get(path, 0))
                conversation.transcript.extend(
                    _json_lines(transcript.read().decode("utf-8", "replace"))
                )
        conversation.ended = True

    @property
    def config_dir(self) -> Path:
        """Where the operator's harness keeps its configuration, its skills among it."""
        return self.home / ".claude"

    def _transcripts(self) -> list[Path]:
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        return sorted(self.config_dir.glob("projects/*/*.jsonl"))

    def _transcript_sizes(self) -> dict[Path, int]:
        return {path: path.stat().st_size for path in self._transcripts()}

    def _suffix(self) -> str:
        return platforms.host(Repo(REPO)).program.removeprefix("printobserver")

    def _install(self) -> Path:
        """The server as the installer lays it out, and the operator's agent beside it."""
        suffix = self._suffix()
        shutil.copy2(program("printobserver"), self.bin / f"printobserver{suffix}")
        for directory, control in ((self.bin, 3), (self.operator_bin, 4)):
            shutil.copy2(
                program("printobserver-server-turn-launcher"), directory / f"claude{suffix}"
            )
            (directory / "launch.json").write_text(
                json.dumps(
                    {
                        "control": doubles.address(self.servers[control]).removeprefix("http://"),
                        "program": self.target[0],
                        "arguments": self.target[1:],
                    }
                ),
                encoding="utf-8",
            )
        shutil.copytree(SKILL, self.state / "skills" / "printobserver")
        # Where Claude Code finds an Agent Skill a person installed for themselves.
        shutil.copytree(SKILL, self.config_dir / "skills" / "printobserver")
        config = self.root / "config.toml"
        config.write_text(
            service_config(
                self.state,
                f"127.0.0.1:{self.port}",
                doubles.address(self.servers[0]),
                doubles.address(self.servers[1]),
                doubles.address(self.servers[2]),
                self.minted,
                interval_s=START_REPLAY_INTERVAL_S,
            ),
            encoding="utf-8",
        )
        return config

    def _environment(self) -> dict[str, str]:
        """The operator's own: where the server is, their credential, and their harness's home."""
        return {
            **neutral_environment(self.bin, self.home),
            "PRINTOBSERVER_SERVER": f"http://127.0.0.1:{self.port}",
            "PRINTOBSERVER_CREDENTIAL": self.minted.operator,
            # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
            "CLAUDE_CONFIG_DIR": str(self.config_dir),
        }

    def _client(self, *arguments: str) -> dict[str, Any]:
        """One operator command through the installed program, its answer parsed.

        Each caller holds the answer to the shape it reads before reading it.
        """
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        done = run(
            [str(self.bin / f"printobserver{self._suffix()}"), *arguments, "--json"],
            env=self._environment(),
            timeout=TURN_DEADLINE_S,
        )
        if done.returncode != 0:
            msg = f"`printobserver {arguments[0]}` answered {done.returncode}: {done.stderr}"
            raise ReplayError(msg)
        answered = json.loads(done.stdout)
        if not isinstance(answered, dict):
            msg = f"`printobserver {arguments[0]}` answered no document"
            raise ReplayError(msg)
        return answered

    def _load(self, material: str) -> None:
        """A full spool of one material on tool 0, the last one taken off first."""
        if self.loaded is not None and self.loaded.material == material:
            return
        said = ["--actor", OPERATOR, "--reason", f"the {material} roll goes on for the next print"]
        if self.loaded is not None:
            self._client("unload-spool", "--spool-id", self.loaded.id, *said)
        registered = self._client(
            "register-spool",
            "--name",
            f"{material} roll",
            "--material",
            material,
            "--initial-g",
            str(SPOOL_GRAMS),
            *said,
        )
        spool = registered.get("spool")
        if not isinstance(spool, dict) or not isinstance(spool.get("id"), str):
            msg = f"registering a {material} spool answered no spool: {registered}"
            raise ReplayError(msg)
        self._client("load-spool", "--spool-id", spool["id"], "--tool", "0", *said)
        self.loaded = LoadedSpool(SpoolId(spool["id"]), material)

    def _frame_now(self) -> str:
        """The camera's frame as the server stored it, from a look at the printer."""
        looked = self._client("look")
        path = looked.get("image_path")
        if not isinstance(path, str):
            msg = f"a look at the printer answered no frame: {looked}"
            raise ReplayError(msg)
        return path

    def _actions(self) -> list[RecordedRequest]:
        """Every action an operator requested, of every print, with when it was recorded."""
        found = []
        prints = self._client("prints").get("prints")
        if not isinstance(prints, list) or not all(map(is_held_print, prints)):
            msg = "the server's prints answered no list of prints"
            raise ReplayError(msg)
        for held in prints:
            events = self._client("history", "--print-id", held["id"], "--limit", "1000").get(
                "events"
            )
            if not isinstance(events, list) or not all(map(is_event, events)):
                msg = f"the history of print {held['id']} answered no list of events"
                raise ReplayError(msg)
            for event in events:
                action = event["payload"].get("action", {})
                if event["kind"] in REQUESTED and action.get("actor") == OPERATOR:
                    operation = str(action["action"]).replace("_", "-")
                    found.append(
                        RecordedRequest(_received(event), Request(operation, dict(action)))
                    )
        return found

    def _starts(self, begun: datetime, ended: datetime) -> int:
        """How many prints the printer's own history says started between two instants."""
        events = self._client(
            "printer-history",
            "--kinds",
            "print_started",
            "--since",
            _rfc3339(begun),
            "--until",
            _rfc3339(ended),
            "--limit",
            "1000",
        ).get("events")
        if not isinstance(events, list) or not all(
            isinstance(event, dict) and event.get("kind") == "print_started" for event in events
        ):
            msg = "the printer's history answered no list of the starts it was asked for"
            raise ReplayError(msg)
        return len(events)

    def _converse(self, session: SessionName, message: str) -> tuple[HarnessReport, int]:
        """One message of one conversation, through `oneharness run --session`."""
        suffix = self._suffix()
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        done = run(
            [
                str(program("printobserver-server-oneharness")),
                "run",
                "--harness",
                "claude-code",
                "--bin",
                f"claude-code={self.operator_bin / f'claude{suffix}'}",
                "--session",
                session,
                "--session-dir",
                str(self.root / "conversations"),
                "--mode",
                "default",
                "--no-config",
                "--no-history",
                "--cwd",
                str(self.home),
                "--timeout",
                str(TURN_DEADLINE_S),
                "--compact",
                "--prompt",
                message,
                "--",
                *harness_arguments(),
            ],
            env=self._environment(),
            timeout=TURN_DEADLINE_S + 60,
        )
        try:
            report = json.loads(done.stdout)
        except ValueError:
            report = None
        if not is_harness_report(report):
            msg = f"oneharness answered {done.returncode} with no report: {done.stderr}"
            raise ReplayError(msg)
        return cast("HarnessReport", report), done.returncode

    def _wait_for_server(self) -> None:
        deadline = time.monotonic() + START_DEADLINE_S
        while time.monotonic() < deadline:
            if self.server is not None and self.server.poll() is not None:
                msg = "the server exited at start"
                raise ReplayError(msg)
            try:
                self._client("prints")
            except ReplayError:
                time.sleep(0.1)
                continue
            return
        msg = f"the server did not answer within {START_DEADLINE_S} s"
        raise ReplayError(msg)

    def run(self) -> StartsReplayed:
        """Feed every request in order, and judge each from the server's record."""
        config = self._install()
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        self.server = start(
            [str(self.bin / f"printobserver{self._suffix()}"), "server", "--config", str(config)],
            cwd=self.state,
            env=neutral_environment(self.bin, self.home),
        )
        keep_the_log(self.server, self.root / "server.log")
        try:
            self._wait_for_server()
            points, sessions = [], []
            for request in self.case.requests:
                points.append(self._one(request, sessions))
            return StartsReplayed(
                case=self.case,
                points=points,
                sessions=sessions,
                transcript=[
                    line
                    for conversation in self.runs
                    for line in [
                        *conversation.transcript,
                        *(
                            _json_lines(conversation.record.read_text("utf-8", "replace"))
                            if conversation.record.is_file()
                            else []
                        ),
                    ]
                ],
            )
        finally:
            self.stop()

    def _one(self, request: StartRequest, sessions: list[SessionName]) -> StartPoint:
        material = material_of(request, self.sheets)
        name = file_name_of(request)
        self.bench.stand_at(
            instant(request.at), request.event_image, doubles.HeldFile(name, start_gcode(material))
        )
        self._load(material)
        frame = self._frame_now()
        session = SessionName(f"{SESSION_PREFIX}{request.day}")
        if session not in sessions:
            sessions.append(session)
        looked_before = len(self.bench.fetches)
        before = len(self.runs)
        begun = self.bench.now()
        report, status = self._converse(session, message_for(name, material, frame))
        ended = self.bench.now()
        if self.errors:
            raise ReplayError("; ".join(self.errors))
        mine = self.runs[before:]
        taken = RunReport()
        for conversation in mine:
            lines = [*conversation.transcript]
            if conversation.record.is_file():
                lines.extend(_json_lines(conversation.record.read_text("utf-8", "replace")))
            taken.take(lines)
        result = report["results"][0]
        failure = None
        if not mine:
            failure = f"the harness never ran: {result['error']}"
        elif status != 0 or result["status"] != "ok":
            said = result["error"] or result["stderr"][-2000:]
            failure = f"the run ended {result['status']}: {said}"
        return StartPoint(
            request=request,
            session=session,
            requests=[
                recorded.request for recorded in self._actions() if begun <= recorded.at <= ended
            ],
            report=taken,
            reply=result["text"],
            failure=failure,
            looks=len(self.bench.fetches) - looked_before,
            started=self._starts(begun, ended),
            phase=report["session"]["phase"],
        )

    def stop(self) -> None:
        """Stop the server and every double."""
        self.bench.stopped.set()
        with self.bench.lock:
            self.bench.finished = True
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
