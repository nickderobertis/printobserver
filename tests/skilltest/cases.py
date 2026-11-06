"""The real-print cases as a replay reads them, and the rule a decision point is judged by.

Every event, frame and expectation comes from a case's `case.json`, which
`tools/repo-checks`'s `checks_real_prints` holds to its schema, its files and
its history before anything here reads it. A case's `assertions.replay` is the
events its one test feeds; a case with none has no test.
"""

from __future__ import annotations

import base64
import json
import re
from dataclasses import dataclass, field
from datetime import datetime
from decimal import Decimal, InvalidOperation
from enum import StrEnum
from pathlib import Path
from typing import Any, NewType

from repo_checks.checks_real_prints import printed_gcode, stored_size

REPO = Path(__file__).resolve().parents[2]
CASES = REPO / "tests" / "real-prints"
#: The frames that are no camera's: enlarged crops, and Obico's own renderings.
NOT_FROM_THE_CAMERA = re.compile(r"^crop-|obico")
#: A frame named for the hour and minute it was taken at.
NAMED_FOR_ITS_MINUTE = re.compile(r"^after-(?P<hour>\d{2})(?P<minute>\d{2})")


def elapsed_seconds(start: str, end: str) -> int:
    """The whole seconds between two `at` times."""
    return int((datetime.fromisoformat(end) - datetime.fromisoformat(start)).total_seconds())


ScenarioId = NewType("ScenarioId", str)
EventId = NewType("EventId", str)


class Trigger(StrEnum):
    """What starts a scenario's turn, as `case.schema.json` names it."""

    RECORDED_ALERT = "recorded_alert"
    SYNTHETIC_ALERT = "synthetic_alert"
    PERIODIC_OBSERVATION = "periodic_observation"
    START_REQUEST = "start_request"


class MachineState(StrEnum):
    """What the machine reads as at a scenario, in the printer contract's spelling."""

    PRINTING = "printing"
    PAUSED = "paused"
    OPERATIONAL = "operational"
    OFFLINE = "offline"

    @property
    def answers(self) -> bool:
        """Whether a read of the machine answers at all.

        The server's scheduled check does not fire on a state a print ends in, so
        a scenario in one is replayed as a machine whose port has gone.
        """
        return self in {MachineState.PRINTING, MachineState.PAUSED}


@dataclass(frozen=True)
class Step:
    """One request a case names: its operation and the arguments it pins."""

    operation: str
    args: dict[str, str | float] = field(default_factory=dict)

    def describe(self) -> str:
        """The step as a reader of a failure would write it."""
        pinned = " ".join(f"{name}={value}" for name, value in self.args.items())
        return f"{self.operation} {pinned}".strip()


@dataclass(frozen=True)
class Request:
    """One request the server recorded the agent making."""

    operation: str
    arguments: dict[str, Any]

    def describe(self) -> str:
        """The request, its reason left out, as a reader of a failure would want it."""
        shown = " ".join(
            f"{name}={value}"
            for name, value in self.arguments.items()
            if name not in {"reason", "actor", "action", "event_id"}
        )
        return f"{self.operation} {shown}".strip()


def _same_value(expected: str | float, given: object) -> bool:
    if isinstance(expected, str):
        return given == expected
    try:
        return Decimal(str(given)) == Decimal(str(expected))
    except InvalidOperation:
        return False


def step_matches(step: Step, request: Request) -> bool:
    """Whether a request is the step's operation and names each of its args with that value."""
    return request.operation == step.operation and all(
        name in request.arguments and _same_value(expected, request.arguments[name])
        for name, expected in step.args.items()
    )


def _outcome_met(outcome: tuple[Step, ...], requests: list[Request]) -> bool:
    position = 0
    for request in requests:
        if position < len(outcome) and step_matches(outcome[position], request):
            position += 1
    return position == len(outcome)


@dataclass(frozen=True)
class Frame:
    """One camera frame of a case, and when it was captured."""

    file: str
    captured: str
    progress_pct: float | None


@dataclass(frozen=True)
class Moment:
    """One entry of a case's `replay`: an event, what it hands over and how it is judged."""

    id: ScenarioId
    at: str
    trigger: Trigger
    event_image: str
    event_id: EventId | None
    printer_state: MachineState
    file_position_bytes: int
    accept_any_of: tuple[tuple[Step, ...], ...]
    never: tuple[Step, ...]
    summary: str

    @property
    def is_alert(self) -> bool:
        """Whether the Obico double delivers this event, rather than the server's own schedule."""
        return self.trigger in {Trigger.RECORDED_ALERT, Trigger.SYNTHETIC_ALERT}

    def verdict(self, requests: list[Request]) -> str | None:
        """Why these requests fail this decision point, or nothing when they pass it."""
        broken = [
            f"it must never {step.describe()}"
            for step in self.never
            if any(step_matches(step, request) for request in requests)
        ]
        if not any(_outcome_met(outcome, requests) for outcome in self.accept_any_of):
            accepted = " | ".join(
                " then ".join(step.describe() for step in outcome) or "(nothing)"
                for outcome in self.accept_any_of
            )
            broken.append(f"it met no accepted outcome of {accepted}")
        return "; ".join(broken) or None


def _event_id(scenario: dict[str, Any]) -> EventId | None:
    return EventId(scenario["event_id"]) if "event_id" in scenario else None


def _step(document: dict[str, Any]) -> Step:
    return Step(document["operation"], dict(document.get("args", {})))


def _captured(document: dict[str, Any], frame: dict[str, Any]) -> str:
    """When a frame was captured, as text sorting in time, or its progress if it has only that."""
    time = frame.get("time")
    day = re.match(r"\d{4}-\d{2}-\d{2}", str(document.get("printed_on", "")))
    if isinstance(time, str):
        return time.replace(" ", "T") if len(time) > 10 else f"{day[0] if day else ''}T{time}"
    named = NAMED_FOR_ITS_MINUTE.match(frame["file"])
    if named and isinstance(document.get("frames"), list):
        dated = next(entry["time"] for entry in document["frames"] if "time" in entry)
        return f"{dated[:10]}T{named['hour']}:{named['minute']}:00"
    return f"{frame.get('progress_pct', 100.0):08.3f}"


@dataclass(frozen=True)
class Case:
    """One case with a `replay`, read for the test that feeds it."""

    name: str
    directory: Path
    frames: tuple[Frame, ...]
    moments: tuple[Moment, ...]
    gcode: Path
    gcode_size: int
    history: tuple[dict[str, Any], ...]

    @property
    def test_id(self) -> str:
        """The id this case's one test is collected under."""
        return self.name

    @property
    def has_frame_times(self) -> bool:
        """Whether its frames carry when they were captured, rather than only their progress."""
        return bool(self.frames) and "T" in self.frames[0].captured

    def frame_at_or_before(self, at: str, image: str) -> int:
        """The index of the frame a moment stands at: its own image, or the latest before `at`."""
        for index, frame in enumerate(self.frames):
            if frame.file == image:
                return index
        earlier = [index for index, frame in enumerate(self.frames) if frame.captured <= at]
        return earlier[-1] if earlier else -1

    def frame_positions(self) -> list[int]:
        """Where the job was when each frame was captured.

        A frame's own progress where it has one; otherwise a straight line through
        the events and frames either side of it, in capture time.
        """
        known: list[tuple[str, int]] = sorted(
            [(moment.at, moment.file_position_bytes) for moment in self.moments]
            + [
                (frame.captured, int(frame.progress_pct / 100 * self.gcode_size))
                for frame in self.frames
                if frame.progress_pct is not None
            ]
        )
        positions = []
        for frame in self.frames:
            if frame.progress_pct is not None:
                positions.append(int(frame.progress_pct / 100 * self.gcode_size))
                continue
            before = [point for point in known if point[0] <= frame.captured]
            after = [point for point in known if point[0] > frame.captured]
            if not before or not after:
                positions.append((before or after)[-1 if before else 0][1])
                continue
            start, end = before[-1], after[0]
            span = elapsed_seconds(start[0], end[0])
            share = elapsed_seconds(start[0], frame.captured) / span if span else 0
            positions.append(int(start[1] + (end[1] - start[1]) * share))
        return positions

    def recorded_alert(self, event_id: str) -> dict[str, Any]:
        """The body Obico posted for a recorded alert, as the history keeps it."""
        event = next(event for event in self.history if event["id"] == event_id)
        return json.loads(base64.b64decode(event["raw"]))


def read_case(name: str) -> Case:
    """One case of `tests/real-prints`, by its directory's name."""
    directory = CASES / name
    document = json.loads((directory / "case.json").read_text(encoding="utf-8"))
    scenarios = {scenario["id"]: scenario for scenario in document["assertions"]["scenarios"]}
    frames = sorted(
        (
            Frame(
                file=frame["file"],
                captured=_captured(document, frame),
                progress_pct=frame.get("progress_pct"),
            )
            for frame in document["frames"]
            if not NOT_FROM_THE_CAMERA.search(frame["file"])
            and frame.get("kind") != "obico_failure_alert"
        ),
        key=lambda frame: frame.captured,
    )
    moments = tuple(
        Moment(
            id=ScenarioId(entry),
            at=scenarios[entry]["at"],
            trigger=Trigger(scenarios[entry]["trigger"]),
            event_image=scenarios[entry]["event_image"],
            event_id=_event_id(scenarios[entry]),
            printer_state=MachineState(scenarios[entry]["printer_state"]),
            file_position_bytes=scenarios[entry]["file_position_bytes"],
            accept_any_of=tuple(
                tuple(_step(step) for step in outcome)
                for outcome in scenarios[entry]["accept_any_of"]
            ),
            never=tuple(_step(step) for step in scenarios[entry]["never"]),
            summary=scenarios[entry]["summary"],
        )
        for entry in document["assertions"].get("replay", ())
    )
    gcode = printed_gcode(directory)
    if gcode is None:
        msg = f"{name} has a replay and carries no G-code"
        raise ValueError(msg)
    history_path = directory / "printobserver-history.json"
    history = (
        json.loads(history_path.read_text(encoding="utf-8"))["events"]
        if history_path.is_file()
        else []
    )
    return Case(
        name=name,
        directory=directory,
        frames=tuple(frames),
        moments=moments,
        gcode=gcode,
        gcode_size=stored_size(gcode),
        history=tuple(history),
    )


def replayed_case_names() -> list[str]:
    """Every case on disk with a `replay`, so a case gaining one is one test with no edit here."""
    return sorted(
        path.parent.name
        for path in CASES.glob("*/case.json")
        if json.loads(path.read_text(encoding="utf-8"))["assertions"].get("replay")
    )
