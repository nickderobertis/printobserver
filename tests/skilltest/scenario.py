"""One scenario, built into the workspace, the input and the stubs one skilltest run takes.

[`build`] lays a scenario down the way a supervision turn finds the world: the
installed skill as the working directory, the pictures as files in the
supervisor's state directory, named the way it names them, and the client
configuration beside them. It fills the committed turn template, or writes an
operator's start request, and stubs every operation the server serves, so
nothing the agent runs reaches a printer, a server, Obico or a camera.

Everything it builds is built without a model, which is what lets the gate's
deterministic tests build every scenario on disk and hold what they build to
the contracts it is read from.
"""

from __future__ import annotations

import base64
import hashlib
import json
import re
import secrets
import shutil
import uuid
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from decimal import Decimal
from pathlib import Path
from typing import Any

from answers import from_labelled, labelled, machine
from real_prints import (
    CASES,
    OBICO_SAMPLE,
    PROGRAM,
    PROMPT_SLOTS_SOURCE,
    SKILL,
    TURN_PROMPT,
    Scenario,
    Step,
    agent_turn,
    commands_in,
    help_pattern,
    history,
    operation_pattern,
    operations,
    program_pattern,
    read_case,
    reads,
    required_steps,
    service_config,
    step_pattern,
    usage,
    version,
    version_pattern,
)
from skilltest_pytest import TestCase, ToolMock, ToolSpy, called, not_called, spy, stub

# The detector's alert kind, and the look's own.
ALERT = "obico_failure_alert"
LOOK = "camera_look"

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

# The job's length, which no case records; only its proportions are read.
ESTIMATED_PRINT_TIME_S = 3600
FILE_SIZE_BYTES = 422889
# How long a look says it waited, which a fixed answer cannot take from the
# command it answers.
LOOK_WAITED_S = 30

# The exit the program refuses an invocation it cannot parse with.
USAGE_EXIT = 2

# The file a start request names. No case records one, and which file it is
# decides nothing about whether the bed is clear.
START_FILE = "calibration-box.gcode"


def _now() -> datetime:
    return datetime.now(UTC)


def _instant(moment: datetime) -> str:
    return moment.strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def _id() -> str:
    return str(uuid.uuid7())


def _shell_quoted(word: str) -> str:
    """One word as a POSIX shell reads it back, as the server quotes the config path."""
    return "'" + word.replace("'", "'\\''") + "'"


# ---------------------------------------------------------------------------
# The committed prompt template
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# The facts one scenario's answers are composed from
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Reading:
    """The printer and its job at one moment."""

    state: str
    completion: float
    nozzle_actual: float
    nozzle_target: float
    bed_actual: float
    bed_target: float
    print_time_s: int
    print_time_left_s: int


@dataclass
class Image:
    """One picture of the case, copied to where the supervisor keeps it."""

    name: str
    path: Path
    sha256: str
    id: str


@dataclass
class Built:
    """Everything one skilltest run of a scenario takes, and what its answers were composed from."""

    scenario: Scenario
    workspace: Path
    config: Path
    print_id: str
    actor: str
    prompt: str
    images: dict[str, Image]
    event: dict[str, Any] | None
    answers: dict[str, list[dict[str, Any]]]
    stubs: list[StubSpec]
    never: list[tuple[Step, ToolSpy]]
    required: list[tuple[Step, ToolSpy]]
    case: TestCase
    replayed: set[str] = field(default_factory=set)
    slots: dict[str, str] | None = None


def _frame(case: str, name: str) -> dict[str, Any]:
    """The frame entry a case's `case.json` gives one of its files, or none."""
    document = read_case(CASES / case)
    for frame in document.get("frames", []):
        if frame["file"] == name:
            return frame
    return {}


def _alerts(case: str) -> list[dict[str, Any]]:
    return [event for event in history(case) if event["kind"] == ALERT]


def _file_name(case: str) -> str:
    """The file a case's print ran, as its history or its sources name it."""
    for alert in _alerts(case):
        return alert["payload"]["file_name"]
    document = read_case(CASES / case)
    named = re.findall(r"[\w-]+(?:\.[\w-]+)*\.gcode", json.dumps(document.get("sources", {})))
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
    prints ran.
    """
    document = read_case(scenario.directory)
    state = "paused" if paused_by_detector else scenario.printer_state
    if state == "operational":
        return Reading(state, 0.0, IDLE_C, 0.0, IDLE_C, 0.0, 0, 0)
    telemetry = [
        row
        for row in document.get("telemetry", [])
        if row.get("state", "").lower() == scenario.printer_state
        and "completion_pct" in row
        and "tool0" in row
        and "bed" in row
    ]
    if telemetry:
        row = telemetry[-1]
        return Reading(
            state=state,
            completion=row["completion_pct"] / 100,
            nozzle_actual=float(row["tool0"]["actual_c"]),
            nozzle_target=float(row["tool0"]["target_c"]),
            bed_actual=float(row["bed"]["actual_c"]),
            bed_target=float(row["bed"]["target_c"]),
            print_time_s=int(row.get("printTime_s", 0)),
            print_time_left_s=int(row.get("printTimeLeft_s", 0)),
        )
    completion = _frame(scenario.case, image).get("progress_pct", 50.0) / 100
    elapsed = round(ESTIMATED_PRINT_TIME_S * completion)
    return Reading(
        state=state,
        completion=completion,
        nozzle_actual=PAUSED_NOZZLE_C if paused_by_detector else NOZZLE_C,
        nozzle_target=0.0 if paused_by_detector else NOZZLE_C,
        bed_actual=BED_C,
        bed_target=BED_C,
        print_time_s=elapsed,
        print_time_left_s=ESTIMATED_PRINT_TIME_S - elapsed,
    )


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
    return {
        "completion": _reading_value(reading.completion),
        "estimated_print_time_s": ESTIMATED_PRINT_TIME_S,
        "file_name": file_name,
        "file_origin": "local",
        "print_time_left_s": reading.print_time_left_s,
        "print_time_s": reading.print_time_s,
        "size_bytes": FILE_SIZE_BYTES,
        "state": reading.state,
    }


# ---------------------------------------------------------------------------
# Building one scenario
# ---------------------------------------------------------------------------


class _Composer:
    """Composes one scenario's world: its images, its event, and every answer."""

    def __init__(self, scenario: Scenario, root: Path) -> None:
        self.scenario = scenario
        self.now = _now()
        self.state_dir = root / "printobserver"
        self.workspace = self.state_dir / "skills" / "printobserver"
        self.config = self.state_dir / "client.toml"
        self.images: dict[str, Image] = {}
        self.trigger = self._trigger_event()
        self.print_id = self.trigger["print_id"] if self.trigger is not None else _id()
        self.actor = json.dumps(
            {"agent": {"session_name": f"print-{self.print_id}"}}, separators=(",", ":")
        )
        self.file_name = START_FILE if scenario.trigger == "start_request" else self._file()
        self.opened_at = _instant(self.now - timedelta(minutes=30))

    # -- the world on disk ---------------------------------------------------

    def lay_down(self) -> None:
        shutil.copytree(SKILL, self.workspace, ignore=shutil.ignore_patterns("__pycache__"))
        self.config.write_text(
            f'[client]\nserver = "http://127.0.0.1:8420"\ncredential = "{secrets.token_hex(24)}"\n',
            encoding="utf-8",
        )
        for name in [self.scenario.event_image, *(look.image for look in self.scenario.looks)]:
            self.image(name)

    def image(self, name: str) -> Image:
        """A case file copied under the state directory, named by its digest as the server does."""
        if name in self.images:
            return self.images[name]
        source = self.scenario.directory / name
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        path = self.state_dir / "images" / digest[:2] / digest
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, path)
        copied = Image(name=name, path=path, sha256=digest, id=_id())
        self.images[name] = copied
        return copied

    # -- the event ------------------------------------------------------------

    def _trigger_event(self) -> dict[str, Any] | None:
        if self.scenario.event_id is None:
            return None
        return next(
            event for event in history(self.scenario.case) if event["id"] == self.scenario.event_id
        )

    def _file(self) -> str:
        source = _frame(self.scenario.case, self.scenario.event_image).get("source")
        return _file_name(source or self.scenario.case)

    def event(self) -> dict[str, Any] | None:
        """The event the turn is about: the recorded alert, or one built in its shape."""
        if self.scenario.trigger == "start_request":
            return None
        image = self.image(self.scenario.event_image)
        if self.trigger is not None:
            recorded = dict(self.trigger)
            recorded["image"] = {"id": image.id, "sha256": image.sha256}
            return recorded
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
        return {
            "id": _id(),
            "image": {"id": image.id, "sha256": image.sha256},
            "kind": ALERT,
            "payload": {
                "file_name": self.file_name,
                "is_warning": self.scenario.detector_warned,
                "obico_print_id": obico_print_id,
                "print_paused": self.scenario.detector_paused_the_print,
                "started_at": _instant(started),
            },
            "print_id": self.print_id,
            "raw": base64.b64encode(json.dumps(body).encode()).decode(),
            "received_at": _instant(received),
            "source": "obico",
        }

    def earlier_events(self, event: dict[str, Any] | None) -> list[dict[str, Any]]:
        """The print's events up to the turn's, oldest first."""
        if event is None:
            return []
        if self.trigger is None:
            return [event]
        recorded = history(self.scenario.case)
        position = next(index for index, held in enumerate(recorded) if held["id"] == event["id"])
        older = list(reversed(recorded[position + 1 :]))
        return [*older, event]

    # -- the answers ----------------------------------------------------------

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
        return {
            name: {"max": float(bound["max"]), "min": float(bound["min"])}
            for name, bound in service_config()["safety"]["allowed"].items()
        }

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
        turn = agent_turn(self.scenario.case)
        if turn is None:
            return None
        for step in turn["steps"]:
            command = step.get("input", {}).get("command", "")
            if step.get("tool") == "Bash" and re.search(r"printobserver context\b", command):
                if step["result"]["is_error"]:
                    continue
                document = from_labelled(step["result"]["content"])
                document["image_path"] = str(self.image(self.scenario.event_image).path)
                return document
        return None

    def look(self, index: int, delivered: list[dict[str, Any]], paused: bool) -> dict[str, Any]:
        """The answer of the scenario's look at `index`."""
        look = self.scenario.looks[index] if self.scenario.looks else None
        frame = self.image(look.image if look else self.scenario.event_image)
        reading = _reading(self.scenario, frame.name, paused_by_detector=paused)
        received = self.now + timedelta(seconds=LOOK_WAITED_S * (index + 1))
        answer: dict[str, Any] = {
            "detector_paused": paused,
            "event": {
                "id": _id(),
                "image": {"id": frame.id, "sha256": frame.sha256},
                "kind": LOOK,
                "payload": {
                    "delivered": [event["id"] for event in delivered],
                    "waited_s": LOOK_WAITED_S,
                },
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
        """Every look's answer, in order; the last repeats for every look after it."""
        recorded = {event["id"]: event for event in history(self.scenario.case)}
        paused = bool(self.scenario.detector_paused_the_print)
        answers = []
        for index, look in enumerate(self.scenario.looks or [None]):
            delivered = [
                recorded[event_id] for event_id in (look.arrived_event_ids if look else ())
            ]
            paused = paused or any(
                event["kind"] == ALERT and event["payload"].get("print_paused")
                for event in delivered
            )
            answers.append(self.look(index, delivered, paused))
        return answers

    def record(self, operation: str, action: dict[str, Any]) -> dict[str, Any]:
        """An accepted, executed action record, as the server answers one.

        It carries no intervention: an intervention's expiry is the duration the
        agent chose, which a fixed answer cannot know, and the answer's schema
        lets a record stand without one.
        """
        actor = json.loads(self.actor)
        requested = self.now + timedelta(minutes=2)
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

    def recorded_acknowledgements(self) -> dict[tuple[str, str], dict[str, Any]]:
        """Each acknowledgement the case's own turn was answered, by its event and disposition."""
        turn = agent_turn(self.scenario.case)
        found: dict[tuple[str, str], dict[str, Any]] = {}
        for step in (turn or {}).get("steps", []):
            command = step.get("input", {}).get("command", "")
            if step.get("tool") != "Bash" or step["result"]["is_error"]:
                continue
            for invoked in commands_in(command):
                if invoked.operation != "acknowledge-failure":
                    continue
                key = (invoked.options.get("event_id", ""), invoked.options.get("disposition", ""))
                found[key] = from_labelled(step["result"]["content"])
        return found


# The reason a fixed answer echoes. The answer's schema requires one, and a
# fixed answer cannot echo the one the agent gave.
_REASON = "as requested"

# Each adjustment, the one value it asks for, and the grid its stubs answer on:
# every value of that resolution inside the bounds the configuration allows,
# each answered by a stub of its own, so an answer echoes the value asked for.
_ADJUSTMENTS: dict[str, tuple[str, str, Decimal]] = {
    "set-feedrate-factor": ("feedrate", "factor", Decimal("0.01")),
    "set-flowrate-factor": ("flowrate", "factor", Decimal("0.01")),
    "set-fan-percent": ("fan", "percent", Decimal(1)),
    "set-tool-target-c": ("tool_target:0", "target_c", Decimal(1)),
    "set-bed-target-c": ("bed_target", "target_c", Decimal(1)),
}
# Where a value off that grid is answered: the printer's own nominal reading.
_NOMINAL = {
    "feedrate": 1.0,
    "flowrate": 1.0,
    "fan": 100.0,
    "tool_target:0": NOZZLE_C,
    "bed_target": BED_C,
}


def _manifest(file_name: str) -> dict[str, Any]:
    return {
        "allowed": {},
        "file_name": file_name,
        "material": "PLA",
        "metadata": {},
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": "0.20mm BALANCED",
    }


@dataclass(frozen=True)
class StubSpec:
    """One stub: the commands it answers, and what it answers each successive one with."""

    name: str
    operation: str
    pattern: str
    documents: tuple[dict[str, Any], ...]
    render: str = "labelled"

    def outputs(self) -> list[dict[str, Any]]:
        """Each response, in the rendering the command asked for, with the program's exit."""
        if self.render == "text":
            return [{"output": document["text"], "exit_code": 0} for document in self.documents]
        if self.render == "refusal":
            return [
                {"output": f"{document['usage']}\n\n{usage()}", "exit_code": USAGE_EXIT}
                for document in self.documents
            ]
        render = machine if self.render == "json" else labelled
        return [{"output": render(document), "exit_code": 0} for document in self.documents]

    def mock(self) -> ToolMock:
        """The skilltest stub."""
        return stub(tool="bash", pattern=self.pattern, responses=self.outputs(), name=self.name)


def _rendered(
    name: str, operation: str, pattern: str, documents: list[dict[str, Any]]
) -> list[StubSpec]:
    """A command's stubs: under `--json` the document, otherwise its lines.

    A command asking for `--json` is matched first, since the first matching
    stub answers.
    """
    json_pattern = pattern + r"""(?:[^"\\]|\\.)*?--json(?:[^\w-]|$)"""
    return [
        StubSpec(f"{name}-json", operation, json_pattern, tuple(documents), "json"),
        StubSpec(name, operation, pattern, tuple(documents)),
    ]


def _grid(bound: dict[str, Any], step: Decimal) -> list[Decimal]:
    low = Decimal(str(bound["min"]))
    high = Decimal(str(bound["max"]))
    values = []
    value = low
    while value <= high:
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


def build(scenario: Scenario, root: Path) -> Built:
    """Lay one scenario down under `root` and compose its run, with no model involved."""
    composer = _Composer(scenario, root)
    composer.lay_down()
    event = composer.event()
    first_reading = _reading(
        scenario, scenario.event_image, paused_by_detector=bool(scenario.detector_paused_the_print)
    )

    replayed: set[str] = set()
    context = composer.recorded_context()
    if context is not None:
        replayed.add("context")
    else:
        context = composer.context(event, first_reading)

    looks = composer.looks()
    alert_ids = [event["id"]] if event is not None else []
    alert_ids += [
        arrived["id"]
        for answer in looks
        for arrived in answer.get("arrived", [])
        if arrived["kind"] == ALERT
    ]
    image = composer.image(scenario.event_image)
    statuses = [
        {
            "interventions": [],
            "job": {
                **(context["context"].get("job") or _job(first_reading, composer.file_name)),
                "state": state,
            },
            "print": composer.print_record(),
            "printer": {**_printer(first_reading, _instant(composer.now)), "connection": state},
        }
        for state in _status_states(scenario, first_reading.state)
    ]
    answers: dict[str, list[dict[str, Any]]] = {
        "prints": [{"active": composer.print_id, "prints": [composer.print_record()]}],
        "status": statuses,
        "context": [context],
        "image": [
            {
                "path": str(image.path),
                "record": {
                    "byte_len": image.path.stat().st_size,
                    "content_type": "image/jpeg",
                    "event_id": event["id"] if event is not None else _id(),
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
            {
                "manifest": {**_manifest(composer.file_name), "allowed": composer.bounds()},
                "narrowings": [],
            }
        ],
        "look": looks,
        "pause": [composer.record("pause", {"reason": _REASON})],
        "resume": [composer.record("resume", {"reason": _REASON})],
        "cancel": [composer.record("cancel", {"reason": _REASON})],
        "start-print": [
            composer.record(
                "start-print",
                {
                    "file_name": composer.file_name,
                    "manifest": _manifest(composer.file_name),
                    "reason": _REASON,
                },
            )
        ],
        "acknowledge-failure": [
            composer.record(
                "acknowledge-failure",
                {
                    "disposition": "watch",
                    "event_id": alert_ids[0] if alert_ids else _id(),
                    "reason": _REASON,
                },
            )
        ],
    }
    for operation, (adjustable, parameter, _) in _ADJUSTMENTS.items():
        action: dict[str, Any] = {parameter: _NOMINAL[adjustable], "reason": _REASON}
        if operation == "set-tool-target-c":
            action["tool"] = 0
        answers[operation] = [composer.record(operation, action)]

    specs: list[StubSpec] = [
        # Asking for the usage or the version prints it, whatever else the
        # command names, so these answer before anything else does.
        StubSpec("help", "", help_pattern(), ({"text": usage()},), "text"),
        StubSpec("version", "", version_pattern(), ({"text": f"{PROGRAM} {version()}\n"},), "text"),
    ]
    # The acknowledgement names what the agent decided, so each disposition of
    # each alert it was handed answers with exactly that — the case's own
    # recorded answer where its turn was given one.
    recorded = composer.recorded_acknowledgements()
    for event_id in alert_ids:
        for disposition in ("continue", "watch", "stop"):
            step = Step("acknowledge-failure", {"disposition": disposition, "event_id": event_id})
            document = recorded.get((event_id, disposition)) or composer.record(
                "acknowledge-failure",
                {"disposition": disposition, "event_id": event_id, "reason": _REASON},
            )
            specs.extend(
                _rendered(
                    f"acknowledge-{disposition}-{event_id}",
                    step.operation,
                    step_pattern(step),
                    [document],
                )
            )
    # Each adjustment echoes the value it was asked for, on its grid.
    bounds = composer.bounds()
    for operation, (adjustable, parameter, resolution) in _ADJUSTMENTS.items():
        for value in _grid(bounds[adjustable], resolution):
            number = float(value) if resolution < 1 else int(value)
            step = Step(operation, {parameter: number})
            action = {parameter: float(value), "reason": _REASON}
            if operation == "set-tool-target-c":
                action["tool"] = 0
            specs.append(
                StubSpec(
                    f"{operation}-{value}",
                    operation,
                    step_pattern(step),
                    (composer.record(operation, action),),
                )
            )
    changing = set(operations()) - reads()
    for operation in operations():
        # An operation that changes something is carried out only with a reason.
        pattern = step_pattern(Step(operation, {}))
        specs.extend(_rendered(operation, operation, pattern, answers[operation]))
    # One without a reason is refused before it reaches a server, as the
    # program refuses a required option it was not given.
    for operation in sorted(changing):
        refusal = {
            "usage": f"{PROGRAM}: `{operation}` needs `--reason`, and this invocation carries none."
        }
        specs.append(
            StubSpec(
                f"{operation}-without-reason",
                operation,
                operation_pattern(operation),
                (refusal,),
                "refusal",
            )
        )

    # Anything else the program is asked prints its usage.
    specs.append(StubSpec("usage", "", program_pattern(), ({"text": usage()},), "text"))

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

    slots = slot_values(composer, event) if event is not None else None
    if slots is None:
        prompt = _start_request(composer)
    else:
        prompt = fill(TURN_PROMPT.read_text(encoding="utf-8"), slots)
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
        answers=answers,
        stubs=specs,
        never=never,
        required=required,
        case=case,
        replayed=replayed,
        slots=slots,
    )


def slot_values(composer: _Composer, event: dict[str, Any]) -> dict[str, str]:
    """What each of the template's slots is filled with for one turn, as `turn.rs` fills them."""
    scenario = composer.scenario
    situation = {
        "printer_state": scenario.printer_state,
        "detector_warned": scenario.detector_warned,
        "detector_paused_the_print": scenario.detector_paused_the_print,
        "arrived_while_busy": [],
    }
    return {
        "{{event}}": json.dumps(event, indent=2, ensure_ascii=False),
        "{{situation}}": json.dumps(situation, indent=2, ensure_ascii=False),
        "{{image_path}}": str(composer.image(scenario.event_image).path),
        "{{context_command}}": (
            f"printobserver context --config {_shell_quoted(str(composer.config))} "
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
        f"Give every printobserver command `--config {_shell_quoted(str(composer.config))}`, "
        f"and each one whose usage lists `--actor` this actor, quoted as written:\n\n"
        f"--actor '{composer.actor}'\n"
    )
