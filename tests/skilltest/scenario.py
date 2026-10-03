"""One scenario, built into the workspace, the input and the stubs one skilltest run takes.

[`build`] lays a scenario down the way a supervision turn finds the world: the
installed skill as the working directory, the pictures as files in the
supervisor's state directory, named the way it names them, and the client
configuration beside them. It fills the committed turn template, or writes an
operator's start request, and stubs every command the program has, so nothing
the agent runs reaches a printer, a server, Obico or a camera.

Every answer is a document in the server's own answer shape, held in the
gate to the server's JSON Schemas and to the program's generated examples
rather than to a second, Python copy of those shapes.

Everything here is built without a model, which is what lets the gate's
deterministic tests build every scenario on disk and hold what they build to
the contracts it is read from.
"""

from __future__ import annotations

import base64
import hashlib
import json
import math
import re
import secrets
import shutil
import uuid
from dataclasses import dataclass, field, replace
from datetime import UTC, datetime, timedelta
from decimal import Decimal
from enum import StrEnum
from pathlib import Path
from typing import Any, NewType

from answers import from_labelled, labelled, machine
from real_prints import (
    ADJUSTABLE,
    DECISION,
    OBICO_SAMPLE,
    PROMPT_SLOTS_SOURCE,
    SCHEMAS,
    SKILL,
    TURN_PROMPT,
    Bound,
    EventId,
    Look,
    Scenario,
    Step,
    agent_turn,
    bounds,
    commands_in,
    conforming,
    history,
    read_case,
    required_steps,
    step_pattern,
)
from skilltest_pytest import TestCase, ToolMock, ToolSpy, called, not_called, spy, stub
from surface import (
    PROGRAM,
    asking_pattern,
    bare_program_pattern,
    event_kind,
    naming_pattern,
    program_pattern,
    surface,
    turn_tool_rules,
    usage,
    version,
)

PrintId = NewType("PrintId", str)
ImageId = NewType("ImageId", str)

# The detector's alert kind and the look's own, as their crates declare them.
ALERT = event_kind("ObicoFailureAlertPayload")
LOOK = event_kind("CameraLookPayload")

# What the printer reads when a case records no telemetry of its own: the PLA
# profile every one of these prints ran at, as both recorded contexts read it
# (`spaghetti-small-nest`'s and `spaghetti-debris-warning`'s agent-turn.json).
NOZZLE_C = 230.0
BED_C = 60.0
# A print the detector paused has had its nozzle heater turned off, as the
# recorded look after `spaghetti-small-nest`'s pausing alert reads it.
PAUSED_NOZZLE_C = 184.17
# An idle printer waiting for a print, heaters off.
IDLE_C = 24.0

# How long a look says it waited, which a fixed answer cannot take from the
# command it answers.
LOOK_WAITED_S = 30
# The markers of a JPEG comment segment and of the JFIF header.
JPEG_COMMENT = b"\xff\xfe"
JFIF_HEADER = b"\xff\xe0"
# How many later looks re-answer the scenario's last frame, each at its own
# instant, before skilltest repeats the last answer as it stands.
LATER_LOOKS = 4
# How many successive requests of one action are each answered by a record of
# their own before skilltest repeats the last.
RECORDS_PER_ACTION = 3

# The file a start request names. No case records one, and which file it is
# decides nothing about whether the bed is clear.
START_FILE = "calibration-box.gcode"

# The reason a fixed answer echoes. The answer's schema requires one, and a
# fixed answer cannot echo the one the agent gave.
ECHOED_REASON = "as requested"


def _instant(moment: datetime) -> str:
    return moment.strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def _id() -> str:
    return str(uuid.uuid7())


def _shell_quoted(word: str) -> str:
    """One word as a POSIX shell reads it back, as the server quotes the config path."""
    return "'" + word.replace("'", "'\\''") + "'"


def _schema_path(name: str) -> Path:
    """The generated schema of one type, under whichever crate checks it in."""
    return next(SCHEMAS.glob(f"*/{name}.json"))


def _schema(name: str) -> dict[str, Any]:
    return json.loads(_schema_path(name).read_text(encoding="utf-8"))


def dispositions() -> list[str]:
    """Every disposition an acknowledgement may carry, as its schema declares them."""
    return [variant["const"] for variant in _schema("AcknowledgementDisposition")["oneOf"]]


def declared_slots() -> list[str]:
    """Every slot `printobserver-oneharness`'s prompt module declares, in its own order."""
    source = PROMPT_SLOTS_SOURCE.read_text(encoding="utf-8")
    return re.findall(r'pub const \w+_SLOT: &str = "(\{\{\w+\}\})";', source)


def template_slots(template: str) -> list[str]:
    """Every slot the template writes, in the order it writes them."""
    return re.findall(r"\{\{\w+\}\}", template)


def fill(template: str, values: dict[str, str]) -> str:
    """The template with each slot filled once, in one pass, as `prompt.rs` fills it.

    Text a filling carries is never read back as a slot.

    Raises:
        ValueError: If the template's slots are not exactly the ones given, once each.
    """
    written = template_slots(template)
    if sorted(written) != sorted(values):
        msg = f"the template writes the slots {written}, and the tier fills {sorted(values)}"
        raise ValueError(msg)
    pieces = re.split(r"(\{\{\w+\}\})", template)
    return "".join(values.get(piece, piece) for piece in pieces)


def harness_config() -> str:
    """The `.oneharness.toml` a run's harness reads: a production turn's permissions.

    A Claude Code turn runs in `default` mode — Claude Code's `dontAsk` — narrowed
    to `turn.rs`'s tools, with the read tools allowed and the shell allowed only
    for the program's own commands. A stubbed command passes because its stub
    answers it allowed; anything else the agent runs is refused, as it is there.
    """
    tools_flag, allowed_flag, tools, allowed = turn_tool_rules()
    arguments = [tools_flag, *tools, allowed_flag, *allowed]
    return f'mode = "default"\n\n[harness.claude-code]\nargs = {json.dumps(arguments)}\n'


@dataclass(frozen=True)
class Reading:
    """The printer and its job at one moment."""

    state: str
    completion: float | None
    nozzle_actual: float
    nozzle_target: float
    bed_actual: float
    bed_target: float
    print_time_s: int | None = None
    print_time_left_s: int | None = None


@dataclass
class Image:
    """One picture of the case, copied to where the supervisor keeps it."""

    name: str
    path: Path
    sha256: str
    id: ImageId


class Render(StrEnum):
    """How a stub renders what it answers."""

    LABELLED = "labelled"
    JSON = "json"
    TEXT = "text"


@dataclass(frozen=True)
class StubSpec:
    """One stub: the commands it answers, and what it answers each successive one with."""

    name: str
    command: str | None
    pattern: str
    documents: tuple[Any, ...]
    render: Render = Render.LABELLED
    exit_code: int = 0

    def outputs(self) -> list[dict[str, Any]]:
        """Each response, rendered as the command asked for, with the program's exit."""
        rendered = []
        for document in self.documents:
            match self.render:
                case Render.TEXT:
                    output = str(document)
                case Render.JSON:
                    output = machine(document)
                case Render.LABELLED:
                    output = labelled(document)
            rendered.append({"output": output, "exit_code": self.exit_code})
        return rendered

    def mock(self) -> ToolMock:
        """The skilltest stub."""
        return stub(tool="bash", pattern=self.pattern, responses=self.outputs(), name=self.name)


@dataclass(frozen=True)
class Adjustment:
    """One bounded adjustment: its command, what it adjusts, and the grid its stubs echo on."""

    command: str
    adjustable: str
    parameter: str
    resolution: Decimal
    nominal: float


# The printer's nominal reading of each adjustable, which a value off an
# adjustment's grid is answered at, by the adjustable's own spelling.
NOMINAL = {
    "feedrate": 1.0,
    "flowrate": 1.0,
    "fan": 100.0,
    "tool_target": NOZZLE_C,
    "bed_target": BED_C,
}
# The tool every tool target here is for: the one tool these prints' printer has.
TOOL = 0


def adjustments() -> tuple[Adjustment, ...]:
    """Every bounded adjustment, as the decision core pairs each action with what it adjusts.

    `decision.rs`'s `adjustment` names, per action, the adjustable it changes
    and the field carrying the value; `adjustable.rs` spells each adjustable as
    the bounds name it. Each value is answered on a grid: every value of its
    resolution inside the bounds the configuration allows, each by a stub of
    its own, so the answer echoes the value asked for — hundredths for a span a
    factor's size, whole units otherwise.

    Raises:
        ValueError: If the core no longer pairs actions with adjustables that way.
    """
    decision = DECISION.read_text(encoding="utf-8")
    spellings = dict(
        re.findall(
            r'Self::(\w+)(?: \{[^}]*\})? => formatter\.write_str\("(\w+)"\)',
            ADJUSTABLE.read_text(encoding="utf-8"),
        )
    )
    tool_target = re.search(r'const TOOL_TARGET: &str = "(\w+)";', ADJUSTABLE.read_text("utf-8"))
    pairs = re.findall(
        r"PrintAction::(\w+) \{[^}]*\} => \{?\s*Some\(\(Adjustable::(\w+)[^,]*, \*(\w+)\)\)",
        decision,
    )
    if not pairs or tool_target is None:
        msg = f"{DECISION} no longer pairs each adjustment with the adjustable it changes"
        raise ValueError(msg)
    found = []
    allowed = bounds()
    for action, variant, parameter in pairs:
        command = re.sub(r"(?<!^)(?=[A-Z])", "-", action).lower()
        if variant in spellings:
            adjustable, nominal = spellings[variant], NOMINAL[spellings[variant]]
        else:
            adjustable = f"{tool_target[1]}:{TOOL}"
            nominal = NOMINAL[tool_target[1]]
        span = allowed[adjustable].max - allowed[adjustable].min
        resolution = Decimal("0.01") if span <= FACTOR_SPAN else Decimal(1)
        found.append(Adjustment(command, adjustable, parameter, resolution, nominal))
    return tuple(found)


# The widest span a factor's bounds have, past which a value is a whole number.
FACTOR_SPAN = 5.0
ADJUSTMENTS = adjustments()


@dataclass
class Built:
    """Everything one skilltest run of a scenario takes, and what its answers were composed from."""

    scenario: Scenario
    workspace: Path
    config: Path
    print_id: PrintId
    actor: str
    prompt: str
    images: dict[str, Image]
    event: dict[str, Any] | None
    situation: dict[str, Any] | None
    answers: dict[str, list[dict[str, Any]]]
    stubs: list[StubSpec]
    never: list[tuple[Step, ToolSpy]]
    required: list[tuple[Step, ToolSpy]]
    case: TestCase
    replayed: set[str] = field(default_factory=set)
    slots: dict[str, str] | None = None


def _frame(case: str, name: str) -> dict[str, Any]:
    """The frame entry a case's `case.json` gives one of its files, or none."""
    return next((f for f in read_case(case).get("frames", []) if f["file"] == name), {})


def _file_name(case: str) -> str:
    """The file a case's print ran, as its history or its sources name it."""
    for event in history(case):
        if event["kind"] == ALERT:
            return event["payload"]["file_name"]
    named = re.findall(
        r"[\w-]+(?:\.[\w-]+)*\.gcode", json.dumps(read_case(case).get("sources", {}))
    )
    printed = [name for name in named if not name.endswith(".clean.gcode")]
    if not printed:
        msg = f"{case} names no G-code file its print ran"
        raise ValueError(msg)
    return printed[0]


def _reading(scenario: Scenario, image: str, *, paused_by_detector: bool) -> Reading:
    """What the printer reads when a frame was taken, from the case's own record.

    A case that recorded the printer's telemetry is read from it: the last
    reading in the scenario's state that carries the job's progress and both
    heaters. Otherwise the frame's progress is the job's, at the profile these
    prints ran, and a progress the case does not record is not answered.
    """
    state = "paused" if paused_by_detector else scenario.printer_state
    if state == "operational":
        return Reading(state, None, IDLE_C, 0.0, IDLE_C, 0.0)
    telemetry = [
        row
        for row in read_case(scenario.case).get("telemetry", [])
        if row.get("state", "").lower() == scenario.printer_state
        and "completion_pct" in row
        and "tool0" in row
        and "bed" in row
    ]
    where = f"{scenario.case}'s case.json"
    if telemetry:
        row = telemetry[-1]
        return Reading(
            state=state,
            completion=_percent(row["completion_pct"], where) / 100,
            nozzle_actual=_measured(row["tool0"].get("actual_c"), where),
            nozzle_target=_measured(row["tool0"].get("target_c"), where),
            bed_actual=_measured(row["bed"].get("actual_c"), where),
            bed_target=_measured(row["bed"].get("target_c"), where),
            print_time_s=_seconds(row.get("printTime_s"), where),
            print_time_left_s=_seconds(row.get("printTimeLeft_s"), where),
        )
    progress = _frame(scenario.case, image).get("progress_pct")
    return Reading(
        state=state,
        completion=None if progress is None else _percent(progress, where) / 100,
        nozzle_actual=PAUSED_NOZZLE_C if paused_by_detector else NOZZLE_C,
        nozzle_target=0.0 if paused_by_detector else NOZZLE_C,
        bed_actual=BED_C,
        bed_target=BED_C,
    )


def _measured(value: object, where: str) -> float:
    """A recorded reading, once it is a finite number.

    Raises:
        ValueError: If it is not.
    """
    if isinstance(value, bool) or not isinstance(value, int | float) or not math.isfinite(value):
        msg = f"{where} records {value!r} where a reading belongs"
        raise ValueError(msg)
    return float(value)


def _percent(value: object, where: str) -> float:
    """A recorded progress, once it is a percentage.

    Raises:
        ValueError: If it is not a number from 0 to 100.
    """
    number = _measured(value, where)
    if not 0 <= number <= 100:
        msg = f"{where} records a progress of {number}, outside 0 to 100"
        raise ValueError(msg)
    return number


def _seconds(value: object, where: str) -> int | None:
    """A recorded duration in whole seconds, when one was recorded.

    Raises:
        ValueError: If one was recorded and is not a non-negative whole number.
    """
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        msg = f"{where} records {value!r} where a number of seconds belongs"
        raise ValueError(msg)
    return value


def _reading_value(value: float) -> dict[str, Any]:
    return {"out_of_range": False, "value": value}


def _printer(reading: Reading, observed_at: str) -> dict[str, Any]:
    return {
        "bed": {
            "actual_c": _reading_value(reading.bed_actual),
            "target_c": _reading_value(reading.bed_target),
        },
        "connection": reading.state,
        "observed_at": observed_at,
        "tools": [
            {
                "actual_c": _reading_value(reading.nozzle_actual),
                "target_c": _reading_value(reading.nozzle_target),
            }
        ],
    }


def _job(reading: Reading, file_name: str) -> dict[str, Any]:
    """The job the printer reports, carrying only what the case records of it."""
    job: dict[str, Any] = {"file_name": file_name, "file_origin": "local", "state": reading.state}
    if reading.completion is not None:
        job["completion"] = _reading_value(reading.completion)
    if reading.print_time_s is not None:
        job["print_time_s"] = reading.print_time_s
    if reading.print_time_left_s is not None:
        job["print_time_left_s"] = reading.print_time_left_s
    return job


def _manifest(file_name: str, allowed: dict[str, Any]) -> dict[str, Any]:
    return {
        "allowed": allowed,
        "file_name": file_name,
        "material": "PLA",
        "metadata": {},
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": "0.20mm BALANCED",
    }


class _Composer:
    """Composes one scenario's world: its images, its event, and every answer."""

    def __init__(self, scenario: Scenario, root: Path) -> None:
        self.scenario = scenario
        self.now = datetime.now(UTC)
        self.state_dir = root / "printobserver"
        self.workspace = self.state_dir / "skills" / "printobserver"
        self.config = self.state_dir / "client.toml"
        self.images: dict[str, Image] = {}
        self.trigger = next(
            (event for event in history(scenario.case) if event["id"] == scenario.event_id), None
        )
        self.print_id = PrintId(self.trigger["print_id"] if self.trigger is not None else _id())
        self.actor = json.dumps(
            {"agent": {"session_name": f"print-{self.print_id}"}}, separators=(",", ":")
        )
        if scenario.trigger == "start_request":
            self.file_name = START_FILE
        else:
            source = _frame(scenario.case, scenario.event_image).get("source")
            self.file_name = _file_name(source or scenario.case)
        self.opened_at = _instant(self.now - timedelta(minutes=30))

    def lay_down(self) -> None:
        """Put the skill, the client configuration and every picture where a turn finds them."""
        shutil.copytree(SKILL, self.workspace, ignore=shutil.ignore_patterns("__pycache__"))
        self.config.write_text(
            f'[client]\nserver = "http://127.0.0.1:8420"\ncredential = "{secrets.token_hex(24)}"\n',
            encoding="utf-8",
        )
        for name in [self.scenario.event_image, *(look.image for look in self.scenario.looks)]:
            self.image(name)

    def image(self, name: str) -> Image:
        """A case file copied under the state directory, named by its digest as the server does."""
        if name not in self.images:
            self.images[name] = self._stored(name, (self.scenario.directory / name).read_bytes())
        return self.images[name]

    def capture(self, name: str, index: int) -> Image:
        """A later capture of a case frame: the same picture, as bytes of a capture of its own.

        A camera never hands two captures of an unchanged scene byte for byte,
        so a later look of the same frame carries the picture with a JPEG comment
        segment after its start marker, which leaves what it shows unchanged.
        """
        original = (self.scenario.directory / name).read_bytes()
        comment = f"capture {index}".encode()
        segment = JPEG_COMMENT + (len(comment) + 2).to_bytes(2, "big") + comment
        # After the JFIF header where there is one, which a reader expects first.
        at = 2
        if original[2:4] == JFIF_HEADER:
            at = 4 + int.from_bytes(original[4:6], "big")
        return self._stored(name, original[:at] + segment + original[at:])

    def _stored(self, name: str, content: bytes) -> Image:
        digest = hashlib.sha256(content).hexdigest()
        path = self.state_dir / "images" / digest[:2] / digest
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return Image(name=name, path=path, sha256=digest, id=ImageId(_id()))

    def event(self) -> dict[str, Any] | None:
        """The event the turn is about: the recorded alert, or one built in its shape."""
        if self.scenario.trigger == "start_request":
            return None
        image = self.image(self.scenario.event_image)
        if self.trigger is not None:
            return {**self.trigger, "image": {"id": image.id, "sha256": image.sha256}}
        return self._synthetic_alert(image)

    def _synthetic_alert(self, image: Image) -> dict[str, Any]:
        """An Obico alert in the recorded alerts' exact shape, for a print Obico never alerted on.

        Its `raw` is the webhook body Obico posts, in the committed sample's
        shape, carrying this alert's own values.
        """
        received = self.now - timedelta(seconds=20)
        started = self.now - timedelta(minutes=30)
        obico_print_id = 1
        body = json.loads(OBICO_SAMPLE.read_text(encoding="utf-8"))
        body["event"]["is_warning"] = self.scenario.detector_warned
        body["event"]["print_paused"] = self.scenario.detector_paused_the_print
        body["print"]["id"] = obico_print_id
        body["print"]["filename"] = self.file_name
        body["print"]["started_at"] = started.timestamp()
        body["printer"]["id"] = 1
        body["printer"]["name"] = "Prusa CORE One+"
        body["img_url"] = (
            f"http://127.0.0.1:3334/media/tsd-pics/snapshots/1/{received.timestamp()}.jpg"
        )
        payload = {
            "file_name": self.file_name,
            "is_warning": self.scenario.detector_warned,
            "obico_print_id": obico_print_id,
            "print_paused": self.scenario.detector_paused_the_print,
            "started_at": _instant(started),
        }
        return {
            "id": _id(),
            "image": {"id": image.id, "sha256": image.sha256},
            "kind": ALERT,
            "payload": conforming(payload, _schema_path("ObicoFailureAlertPayload"), "an alert"),
            "print_id": self.print_id,
            "raw": base64.b64encode(json.dumps(body).encode()).decode(),
            "received_at": _instant(received),
            "source": "obico",
        }

    def situation(self) -> dict[str, Any]:
        """What the supervisor knew when the turn began, in `TurnSituation`'s field order."""
        return {
            "printer_state": self.scenario.printer_state,
            "detector_warned": self.scenario.detector_warned,
            "detector_paused_the_print": self.scenario.detector_paused_the_print,
            "arrived_while_busy": [],
        }

    def earlier_events(self, event: dict[str, Any] | None) -> list[dict[str, Any]]:
        """The print's events up to the turn's, oldest first."""
        if event is None:
            return []
        if self.trigger is None:
            return [event]
        recorded = history(self.scenario.case)
        position = next(index for index, held in enumerate(recorded) if held["id"] == event["id"])
        return [*reversed(recorded[position + 1 :]), event]

    def print_record(self) -> dict[str, Any]:
        provider = (self.trigger or {}).get("payload", {}).get("obico_print_id", 1)
        return {
            "file_name": self.file_name,
            "id": self.print_id,
            "narrowings": [],
            "opened_at": self.opened_at,
            "provider_print_id": provider,
            "state": "printing",
        }

    def bounds(self) -> dict[str, Any]:
        """The configured bounds, as an answer's `allowed` carries them."""
        return {name: {"max": bound.max, "min": bound.min} for name, bound in bounds().items()}

    def context(self, event: dict[str, Any] | None, reading: Reading) -> dict[str, Any]:
        image = self.image(self.scenario.event_image)
        context: dict[str, Any] = {
            "bounds": {"allowed": self.bounds()},
            "interventions": [],
            "job": _job(reading, self.file_name),
            "latest_image": {"id": image.id, "sha256": image.sha256},
            "print": self.print_record(),
            "printer": _printer(reading, _instant(self.now - timedelta(seconds=19))),
            "recent_events": self.earlier_events(event),
        }
        return {"context": context, "image_path": str(image.path)}

    def recorded_context(self) -> dict[str, Any] | None:
        """The context the case's own turn was answered, with its picture repointed to the copy."""
        for recorded in agent_turn(self.scenario.case) or []:
            invoked = commands_in(recorded.command)
            if recorded.failed or not any(c.operation == "context" for c in invoked):
                continue
            document = conforming(
                from_labelled(recorded.answered),
                SCHEMAS / "printobserver-server" / "ContextAnswer.json",
                f"{self.scenario.case}'s recorded context",
            )
            document["image_path"] = str(self.image(self.scenario.event_image).path)
            return document
        return None

    def records(self, operation: str, action: dict[str, Any]) -> list[dict[str, Any]]:
        """The records successive requests of one action answer, each its own."""
        return [self.record(operation, action, sequence) for sequence in range(RECORDS_PER_ACTION)]

    def recorded_acknowledgements(self) -> dict[tuple[str, str], dict[str, Any]]:
        """Each acknowledgement the case's own turn was answered, by its event and disposition."""
        found: dict[tuple[str, str], dict[str, Any]] = {}
        for recorded in agent_turn(self.scenario.case) or []:
            if recorded.failed:
                continue
            for invoked in commands_in(recorded.command):
                if invoked.operation == "acknowledge-failure":
                    key = (
                        invoked.options.get("event_id", ""),
                        invoked.options.get("disposition", ""),
                    )
                    found[key] = conforming(
                        from_labelled(recorded.answered),
                        SCHEMAS / "printobserver-server" / "ActionAnswer.json",
                        f"{self.scenario.case}'s recorded acknowledgement",
                    )
        return found

    def look(
        self, index: int, look: Look | None, delivered: list[dict[str, Any]], paused: bool
    ) -> dict[str, Any]:
        """The answer of the look at `index`, answering a scenario look's frame."""
        name = look.image if look else self.scenario.event_image
        later = index >= max(len(self.scenario.looks), 1)
        frame = self.capture(name, index) if later else self.image(name)
        reading = _reading(self.scenario, frame.name, paused_by_detector=paused)
        if later:
            # The case records no progress for a look after its own frames.
            reading = replace(reading, completion=None, print_time_s=None, print_time_left_s=None)
        received = self.now + timedelta(seconds=LOOK_WAITED_S * (index + 1))
        answer: dict[str, Any] = {
            "detector_paused": paused,
            "event": {
                "id": _id(),
                "image": {"id": frame.id, "sha256": frame.sha256},
                "kind": LOOK,
                "payload": conforming(
                    {"delivered": [event["id"] for event in delivered], "waited_s": LOOK_WAITED_S},
                    _schema_path("CameraLookPayload"),
                    "a look",
                ),
                "print_id": self.print_id,
                "received_at": _instant(received),
                "source": "system",
            },
            "frame": {"id": frame.id, "sha256": frame.sha256},
            "image_path": str(frame.path),
            "job": _job(reading, self.file_name),
            "printer": _printer(reading, _instant(received)),
        }
        if delivered:
            answer["arrived"] = delivered
        return answer

    def looks(self) -> list[dict[str, Any]]:
        """Every look's answer, in order, then the last frame looked at again.

        The last look repeats for every look after it, as a later look of the
        same frame: a look the server takes later carries its own instant and
        its own record, and delivers nothing it already delivered.
        """
        recorded = {EventId(event["id"]): event for event in history(self.scenario.case)}
        paused = bool(self.scenario.detector_paused_the_print)
        looks: list[Look | None] = list(self.scenario.looks) or [None]
        answers = []
        for index in range(len(looks) + LATER_LOOKS):
            look = looks[min(index, len(looks) - 1)]
            arrived = look.arrived_event_ids if look and index < len(looks) else ()
            delivered = [recorded[event_id] for event_id in arrived]
            paused = paused or any(
                event["kind"] == ALERT and event["payload"].get("print_paused")
                for event in delivered
            )
            answers.append(self.look(index, look, delivered, paused))
        return answers

    def record(self, operation: str, action: dict[str, Any], sequence: int = 0) -> dict[str, Any]:
        """An accepted, executed action record, as the server answers one.

        It carries no intervention: an intervention's expiry is the duration the
        agent chose, which a fixed answer cannot know, and the answer's schema
        lets a record stand without one. The `sequence`-th request of the same
        action is a record of its own, a second later.
        """
        actor = json.loads(self.actor)
        # Stamped after the scenario's own looks and before any later one: an
        # agent looks, acts, then looks again.
        looked = LOOK_WAITED_S * (len(self.scenario.looks) + 0.5)
        requested = self.now + timedelta(seconds=looked + sequence)
        return {
            "record": {
                "decision": "accepted",
                "executed_at": _instant(requested + timedelta(milliseconds=12)),
                "id": _id(),
                "outcome": "succeeded",
                "print_id": self.print_id,
                "request": {
                    "action": {"action": operation.replace("-", "_"), "actor": actor, **action},
                    "actor": actor,
                    "requested_at": _instant(requested),
                },
            }
        }


def not_an_option(option: str, command: str) -> str:
    """What the program prints refusing an option a command does not take (`parse.rs`)."""
    return f"{PROGRAM}: `{option}` is not an option of `{command}`.\n\n{usage()}\n"


def _rendered(
    name: str, command: str, pattern: str, documents: list[dict[str, Any]]
) -> list[StubSpec]:
    """A command's stubs: under `--json` the document, otherwise its lines.

    A command asking for `--json` is matched first, since the first matching
    stub answers.
    """
    json_pattern = pattern + r"""(?:[^"\\]|\\.)*?--json(?:[^\w-]|$)"""
    return [
        StubSpec(f"{name}-json", command, json_pattern, tuple(documents), Render.JSON),
        StubSpec(name, command, pattern, tuple(documents)),
    ]


def _grid(bound: Bound, step: Decimal) -> list[Decimal]:
    values = []
    value = Decimal(str(bound.min))
    while value <= Decimal(str(bound.max)):
        values.append(value.quantize(step))
        value += step
    return values


def _status_states(scenario: Scenario, before: str) -> list[str]:
    """The printer state successive `status` reads answer.

    Where every acceptable outcome pauses or resumes the print, the case fixes
    the sequence, so a status read after that action reads the state it
    produced; otherwise every read answers the state the turn began in.
    """
    for operation, after in (("pause", "paused"), ("resume", "printing")):
        if all(
            any(step.operation == operation for step in outcome)
            for outcome in scenario.accept_any_of
        ):
            return [before, after]
    return [before]


def _answers(
    composer: _Composer, event: dict[str, Any] | None
) -> tuple[dict[str, list[dict[str, Any]]], set[str]]:
    """What every command answers, and which of those are a case's own recorded answers."""
    scenario = composer.scenario
    first = _reading(
        scenario, scenario.event_image, paused_by_detector=bool(scenario.detector_paused_the_print)
    )
    replayed: set[str] = set()
    context = composer.recorded_context()
    if context is None:
        context = composer.context(event, first)
    else:
        replayed.add("context")
    image = composer.image(scenario.event_image)
    alert = event["id"] if event is not None else _id()
    answers: dict[str, list[dict[str, Any]]] = {
        "prints": [{"active": composer.print_id, "prints": [composer.print_record()]}],
        "status": [
            {
                "interventions": [],
                "job": {
                    **context["context"].get("job", _job(first, composer.file_name)),
                    "state": state,
                },
                "print": composer.print_record(),
                "printer": {**_printer(first, _instant(composer.now)), "connection": state},
            }
            for state in _status_states(scenario, first.state)
        ],
        "context": [context],
        "image": [
            {
                "path": str(image.path),
                "record": {
                    "byte_len": image.path.stat().st_size,
                    "content_type": "image/jpeg",
                    "event_id": alert,
                    "fetched_at": _instant(composer.now),
                    "id": image.id,
                    "print_id": composer.print_id,
                    "relative_path": image.path.relative_to(composer.state_dir).as_posix(),
                    "sha256": image.sha256,
                    "source_url": "http://127.0.0.1:1984/api/frame.jpeg?src=buddy",
                },
            }
        ],
        "history": [{"events": list(reversed(composer.earlier_events(event)))}],
        "manifest-get": [{"narrowings": []}],
        "manifest-set": [
            {"manifest": _manifest(composer.file_name, composer.bounds()), "narrowings": []}
        ],
        "look": composer.looks(),
        "start-print": composer.records(
            "start-print",
            {
                "file_name": composer.file_name,
                "manifest": _manifest(composer.file_name, {}),
                "reason": ECHOED_REASON,
            },
        ),
        "acknowledge-failure": composer.records(
            "acknowledge-failure",
            {"disposition": dispositions()[0], "event_id": alert, "reason": ECHOED_REASON},
        ),
    }
    for operation in ("pause", "resume", "cancel"):
        answers[operation] = composer.records(operation, {"reason": ECHOED_REASON})
    for adjustment in ADJUSTMENTS:
        answers[adjustment.command] = [_adjusted(composer, adjustment, adjustment.nominal)]
    return answers, replayed


def _adjusted(composer: _Composer, adjustment: Adjustment, value: float) -> dict[str, Any]:
    action: dict[str, Any] = {adjustment.parameter: value, "reason": ECHOED_REASON}
    if adjustment.adjustable.endswith(f":{TOOL}"):
        action["tool"] = TOOL
    return composer.record(adjustment.command, action)


def _stubs(
    composer: _Composer, answers: dict[str, list[dict[str, Any]]], alerts: list[str]
) -> list[StubSpec]:
    """Every stub, in the order the hook tries them; the first that matches answers."""
    exits = surface().exits
    specs = [
        StubSpec("help", None, asking_pattern("--help"), (usage(),), Render.TEXT),
        StubSpec("version", None, asking_pattern("--version"), (version(),), Render.TEXT),
    ]
    # An actor given to a command that takes none is refused before anything is
    # sent, as the turn's prompt warns: the program's own refusal, for each form.
    actor_forms = next(
        field.forms
        for command in surface().commands
        for field in command.fields
        if field.name == "actor"
    )
    for command in surface().operations:
        if any(field.name == "actor" for field in command.fields):
            continue
        for option in actor_forms:
            specs.append(
                # llmlint: ignore[cli_output_contract] suppressions.toml has the reason.
                StubSpec(
                    f"{command.name}-refuses-{option.lstrip('-')}",
                    command.name,
                    naming_pattern(command.name, option),
                    (not_an_option(option, command.name),),
                    Render.TEXT,
                    exits["usage"],
                )
            )
    # An acknowledgement echoes what the agent decided: one stub per disposition
    # of each alert it was handed, the case's own recorded answer where it has one.
    recorded = composer.recorded_acknowledgements()
    for event_id in alerts:
        for disposition in dispositions():
            step = Step("acknowledge-failure", {"disposition": disposition, "event_id": event_id})
            documents = composer.records(step.operation, {**step.args, "reason": ECHOED_REASON})
            if (event_id, disposition) in recorded:
                documents[0] = recorded[(event_id, disposition)]
            name = f"acknowledge-{disposition}-{event_id}"
            specs.extend(_rendered(name, step.operation, step_pattern(step), documents))
    allowed = bounds()
    for adjustment in ADJUSTMENTS:
        for value in _grid(allowed[adjustment.adjustable], adjustment.resolution):
            number = float(value) if adjustment.resolution < 1 else int(value)
            step = Step(adjustment.command, {adjustment.parameter: number})
            document = _adjusted(composer, adjustment, float(value))
            name = f"{adjustment.command}-{value}"
            specs.extend(_rendered(name, step.operation, step_pattern(step), [document]))
    for command in surface().operations:
        pattern = step_pattern(Step(command.name, {}))
        specs.extend(_rendered(command.name, command.name, pattern, answers[command.name]))
    refusal = f"{PROGRAM}: this invocation is not one this program can carry out.\n\n{usage()}\n"
    specs += [
        StubSpec("usage", None, bare_program_pattern(), (usage(),), Render.TEXT, exits["success"]),
        # llmlint: ignore[cli_output_contract] suppressions.toml has the reason.
        StubSpec("refused", None, program_pattern(), (refusal,), Render.TEXT, exits["usage"]),
    ]
    return specs


def build(scenario: Scenario, root: Path) -> Built:
    """Lay one scenario down under `root` and compose its run, with no model involved."""
    composer = _Composer(scenario, root)
    composer.lay_down()
    event = composer.event()
    answers, replayed = _answers(composer, event)
    alerts = [event["id"]] if event is not None else []
    alerts += [
        arrived["id"]
        for answer in answers["look"]
        for arrived in answer.get("arrived", [])
        if arrived["kind"] == ALERT
    ]
    specs = _stubs(composer, answers, alerts)

    never = [(step, spy(tool="bash", pattern=step_pattern(step))) for step in scenario.never]
    required = [
        (step, spy(tool="bash", pattern=step_pattern(step))) for step in required_steps(scenario)
    ]
    evals = [
        *(not_called(watcher, name=f"never {step.describe()}") for step, watcher in never),
        *(called(watcher, name=f"required {step.describe()}") for step, watcher in required),
    ]
    if not evals:
        msg = f"{scenario.test_id} asks for nothing an eval can hold: no never or required step"
        raise ValueError(msg)

    situation = composer.situation() if event is not None else None
    slots = slot_values(composer, event, situation) if event is not None and situation else None
    prompt = (
        fill(TURN_PROMPT.read_text(encoding="utf-8"), slots) if slots else _start_request(composer)
    )
    case = TestCase(
        name=scenario.test_id,
        skill=str(composer.workspace),
        input=prompt,
        mocks=[
            *(spec.mock() for spec in specs),
            *(watcher for _, watcher in never),
            *(watcher for _, watcher in required),
        ],
        evals=evals,
    )
    return Built(
        scenario=scenario,
        workspace=composer.workspace,
        config=composer.config,
        print_id=composer.print_id,
        actor=composer.actor,
        prompt=prompt,
        images=composer.images,
        event=event,
        situation=situation,
        answers=answers,
        stubs=specs,
        never=never,
        required=required,
        case=case,
        replayed=replayed,
        slots=slots,
    )


def slot_values(
    composer: _Composer, event: dict[str, Any], situation: dict[str, Any]
) -> dict[str, str]:
    """What each of the template's slots is filled with for one turn, as `turn.rs` fills them."""
    return {
        "{{event}}": json.dumps(event, indent=2, ensure_ascii=False),
        "{{situation}}": json.dumps(situation, indent=2, ensure_ascii=False),
        "{{image_path}}": str(composer.image(composer.scenario.event_image).path),
        "{{context_command}}": (
            f"{PROGRAM} context --config {_shell_quoted(str(composer.config))} "
            f"--print-id {composer.print_id}"
        ),
        "{{actor}}": composer.actor,
    }


def _start_request(composer: _Composer) -> str:
    """An operator's request to start a named print, with the camera's current frame."""
    frame = composer.image(composer.scenario.event_image)
    return (
        f"Please start the next print on the printer: `{composer.file_name}`. It is "
        "Prusament PLA on the 0.4 mm nozzle, sliced with the 0.20mm BALANCED profile, "
        f"and its print record is {composer.print_id}.\n\n"
        f"This is what the printer's camera shows right now:\n\n{frame.path}\n\n"
        f"Give every {PROGRAM} command `--config {_shell_quoted(str(composer.config))}`, "
        f"and each one whose usage lists `--actor` this actor, quoted as written:\n\n"
        f"--actor '{composer.actor}'\n"
    )
