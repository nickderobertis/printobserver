#!/usr/bin/env python3
"""The real-printer smoke test: this stack, end to end, against the actual machine.

Everything else in this repository is proven against a virtual printer, and a
virtual printer cannot tell you that a real Prusa refuses a flow adjustment
mid-print or that a restore lands on a machine that has already moved on. This
is the one test that says the whole stack works on the machine — and it is the
one place here where a bug damages hardware, so it prefers refusing to run over
proceeding on a precondition it is unsure of.

    just test-printer-smoke --run

**Nothing selects it automatically, and two things together select it.** The
`--run` flag on that recipe, and `PRINTOBSERVER_SMOKE_DEVICE` naming the serial
device the printer is on. One alone does not select it; absent either, this says
so and which was missing rather than skipping silently or failing. The ordinary
gate, continuous integration and every scheduled tier leave it unselected: a
print is hours of filament, and an unattended test that starts one is a test
that ruins a print nobody was watching.

**Every precondition fails closed.** Each is checked before anything is asked of
the machine, an unmet one stops the run naming it, and refusing is a *pass* for
this program's own purposes rather than a failure to investigate: exit zero,
having sent the printer no command at all.

**What it drives is the installed program.** `printobserver` as a subprocess
against an already-running supervisor, which is exactly the operator's own
surface — so nothing here reaches a printer except through the supervisor, the
policy and the safety envelope this system exists to put in between.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
import time
import tomllib
from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from http.client import HTTPException
from pathlib import Path

from octoprint_env import DEFAULT_STATE_DIR as OCTOPRINT_STATE_DIR
from octoprint_env import call as octoprint_call
from repo_checks.shell import run

# ~~ what selects this test, and what configures it

#: The flag the recipe must be given. Two inputs select this test and this is one.
FLAG = "--run"

#: The variable naming the serial device the printer is on. The other one.
DEVICE_ENV = "PRINTOBSERVER_SMOKE_DEVICE"

#: The configuration this smoke reads its safety envelope out of, and points
#: every command it runs at. On the machine beside the printer this is the
#: server's own file, which is what the service installer wrote.
CONFIG_ENV = "PRINTOBSERVER_SMOKE_CONFIG"
DEFAULT_CONFIG = "/etc/printobserver/config.toml"

#: The print this smoke acts on. A print record is minted by the supervisor
#: rather than by a caller, so the one to act on is named rather than guessed.
PRINT_ENV = "PRINTOBSERVER_SMOKE_PRINT_ID"

#: The variable that points a client at a supervisor, which wins over the
#: configuration file. This smoke reads it so that it can refuse one naming a
#: supervisor other than the one attached to the printer it verified.
SERVER_ENV = "PRINTOBSERVER_SERVER"

#: The program to drive. The installed `printobserver`, unless a host names another.
PROGRAM_ENV = "PRINTOBSERVER_SMOKE_PROGRAM"
PROGRAM = "printobserver"

#: How long the machine is given to report what it was asked for. A socket
#: answers at once and a printer takes its own time, which is what this is for.
SETTLE_ENV = "PRINTOBSERVER_SMOKE_SETTLE_S"
DEFAULT_SETTLE_S = 60.0

#: How long the bounded intervention stands for before it expires.
DURATION_ENV = "PRINTOBSERVER_SMOKE_DURATION_S"
DEFAULT_DURATION_S = 20

#: How long any one command is given to answer.
COMMAND_TIMEOUT_S = 120.0

#: How often the machine is asked what it is reporting.
POLL_PAUSE_S = 1.0

#: The payload this smoke prints, and the name it is uploaded under.
GCODE = Path(__file__).resolve().parent / "gcode" / "smoke.gcode"
FILE_NAME = "smoke.gcode"

#: `OctoPrint`'s own connection read, and the port a virtual printer reports.
#: `tools/octoprint-env` is the one script here that knows what an `OctoPrint`
#: is, and this reads through its own client rather than opening a connection.
CONNECTION_PATH = "/api/connection"

#: Who this smoke acts as, and why every action it takes says it is acting.
ACTOR = "operator"
REASON = "the real-printer smoke test is proving this stack against the machine"

# ~~ the safety envelope this test requires of the host it runs on

#: The envelope the configuration must carry for this smoke to run at all.
#:
#: **It is conservative by construction rather than by promise.** No range here
#: is wider than the ceiling `AGENTS.md`'s "The real-printer smoke test" names,
#: and `tests/test_envelope.py` holds it to that ceiling — because a test that
#: shipped a permissive envelope and then faithfully required it would satisfy
#: its own precondition while missing the point of having one. These bounds
#: accommodate PLA comfortably and exclude every material needing temperatures
#: nobody has reviewed, so a smoke run cannot be pointed at a filament this
#: ceiling was not written for.
CONSERVATIVE_ENVELOPE: dict[str, tuple[float, float]] = {
    "feedrate": (0.5, 1.2),
    "flowrate": (0.9, 1.1),
    "tool_target:0": (0.0, 230.0),
    "bed_target": (0.0, 70.0),
    "fan": (0.0, 100.0),
}


@dataclass(frozen=True, slots=True)
class Knob:
    """One adjustable, and how this smoke asks for it and reads it back."""

    adjustable: str
    command: str
    option: str
    step: float
    reported_at: str
    also: tuple[str, ...] = ()

    def asking(self, value: float) -> list[str]:
        """The options one request for `value` carries."""
        return [*self.also, self.option, f"{value:g}"]


#: Every adjustable this smoke drives, in the order it drives them.
KNOBS: tuple[Knob, ...] = (
    Knob("feedrate", "set-feedrate-factor", "--factor", 0.05, "/printer/feedrate_factor/value"),
    Knob("flowrate", "set-flowrate-factor", "--factor", 0.02, "/printer/flowrate_factor/value"),
    Knob(
        "tool_target:0",
        "set-tool-target-c",
        "--target-c",
        5.0,
        "/printer/tools/0/target_c/value",
        also=("--tool", "0"),
    ),
    Knob("bed_target", "set-bed-target-c", "--target-c", 2.0, "/printer/bed/target_c/value"),
    Knob("fan", "set-fan-percent", "--percent", 5.0, "/printer/fan_percent/value"),
)

# ~~ the exits the client program earns, as this smoke reads them

CLIENT_SUCCESS = 0
CLIENT_REJECTED = 5

# ~~ the sequence this smoke verifies, named so a failure says where it was

STEP_CONTEXT = "context"
STEP_START = "start-print"
STEP_ADJUST_INSIDE = "adjust-inside"
STEP_ADJUST_OUTSIDE = "adjust-outside"
STEP_INTERVENTION = "bounded-intervention"
STEP_PAUSE_RESUME = "pause-resume"
STEP_CANCEL = "cancel"
STEP_HISTORY = "history"
STEP_CLEANUP = "cleanup"

#: The order the verification runs in, and there is no other.
SEQUENCE: tuple[str, ...] = (
    STEP_CONTEXT,
    STEP_START,
    STEP_ADJUST_INSIDE,
    STEP_ADJUST_OUTSIDE,
    STEP_INTERVENTION,
    STEP_PAUSE_RESUME,
    STEP_CANCEL,
    STEP_HISTORY,
)

#: The states a printer reports while it has a print to act on.
RUNNING_STATES = ("printing", "paused")
OPERATIONAL = "operational"


def asked(value: float) -> float:
    """One value as this smoke asks for it, and so as it must be read back.

    A bound stepped inside is arithmetic on floats, and `1.2 - 2 * 0.05` is not
    `1.1`: the value that reaches the machine is the one this program prints,
    so the value it then requires the machine to report is that one and not the
    one the subtraction produced.
    """
    return round(value, 4)


def address_of(named: str) -> str:
    """One supervisor address, spelled the one way two of them can be compared."""
    return named.strip().removeprefix("http://").removeprefix("https://").rstrip("/")


def say(message: str) -> None:
    """Report one line of what this smoke is doing, or refusing to do."""
    print(f"printer-smoke: {message}", flush=True)


def at(document: object, pointer: str) -> object:
    """The value one slash-separated path names in a decoded document."""
    found = document
    for segment in pointer.strip("/").split("/"):
        if isinstance(found, dict):
            found = found.get(segment)
        elif isinstance(found, list) and segment.isdigit() and int(segment) < len(found):
            found = found[int(segment)]
        else:
            return None
    return found


def number_at(document: object, pointer: str) -> float | None:
    """The number one path names, when it names one."""
    found = at(document, pointer)
    return float(found) if isinstance(found, (int, float)) and not isinstance(found, bool) else None


class VerificationError(Exception):
    """One verification point the machine's own answer did not satisfy."""

    def __init__(self, step: str, detail: str) -> None:
        """Name the step it happened at and what the answer was.

        Args:
            step: One of `SEQUENCE`.
            detail: What was required and what came back.
        """
        self.step = step
        self.detail = detail
        super().__init__(f"{step}: {detail}")


@dataclass(frozen=True, slots=True)
class Selection:
    """Whether the two inputs together selected this test, and what was missing."""

    selected: bool
    device: str
    missing: tuple[str, ...]


def select(argv: Sequence[str], environ: dict[str, str]) -> Selection:
    """Read the two inputs that select this test, naming whichever is absent.

    Args:
        argv: The arguments the recipe passed on.
        environ: The environment it ran under.

    Returns:
        The selection, carrying every missing input rather than the first.
    """
    missing: list[str] = []
    if FLAG not in argv:
        missing.append(f"the `{FLAG}` flag was not given to the recipe")
    device = environ.get(DEVICE_ENV, "").strip()
    if not device:
        missing.append(f"{DEVICE_ENV} names no serial device")
    return Selection(selected=not missing, device=device, missing=tuple(missing))


@dataclass(frozen=True, slots=True)
class Answer:
    """What one command of the client program answered."""

    exit: int
    document: object
    said: str


@dataclass
class Smoke:
    """One run against one machine: what it was given, and what it changed."""

    device: str
    config: Path
    print_id: str
    program: str
    state_dir: Path
    settle_s: float
    duration_s: int
    manifest: object = None
    record: dict[str, object] = field(default_factory=dict)
    bounds: dict[str, tuple[float, float]] = field(default_factory=dict)
    found: dict[str, float] = field(default_factory=dict)
    actions: list[str] = field(default_factory=list)
    left_changed: list[str] = field(default_factory=list)
    started: bool = False
    cleaned: bool = False

    def ask(self, command: str, *arguments: str) -> Answer:
        """Run one command of the client program against the running supervisor.

        Args:
            command: The client command, as this program's own surface spells it.
            *arguments: Its own options.

        Returns:
            The exit it earned, the document it printed and everything it said.
        """
        completed = run(
            [
                self.program,
                command,
                "--print-id",
                self.print_id,
                "--json",
                "--config",
                str(self.config),
                *arguments,
            ],
            timeout=COMMAND_TIMEOUT_S,
        )
        try:
            document = json.loads(completed.stdout)
        except json.JSONDecodeError:
            document = None
        return Answer(completed.returncode, document, (completed.stdout or "") + completed.stderr)

    def act(self, step: str, command: str, *arguments: str) -> Answer:
        """Ask for one action, and require the supervisor to have accepted it.

        Args:
            step: The verification point this belongs to.
            command: The client command asking for the action.
            *arguments: Its own options, beside the reason and the actor.

        Returns:
            The answer, once it is an accepted one.

        Raises:
            VerificationError: If the supervisor did not accept it.
        """
        answer = self.ask(command, "--actor", ACTOR, "--reason", REASON, *arguments)
        if answer.exit != CLIENT_SUCCESS:
            raise VerificationError(
                step, f"`{command}` exited {answer.exit} rather than doing it: {answer.said}"
            )
        recorded = at(answer.document, "/record/id")
        if isinstance(recorded, str):
            self.actions.append(recorded)
        return answer

    def status(self) -> object:
        """One status read of this print, as the document it answers."""
        return self.ask("status").document

    def settles(self, pointer: str, expected: object, *, numeric: bool = False) -> object:
        """Poll the machine until it reports `expected`, and answer what it reports.

        Args:
            pointer: Where in a status read the value is.
            expected: What it has to report.
            numeric: Whether to compare as numbers rather than as values.

        Returns:
            What it last reported, which is `expected` when it settled there.
        """
        deadline = time.monotonic() + self.settle_s
        reported: object = None
        while True:
            document = self.status()
            reported = number_at(document, pointer) if numeric else at(document, pointer)
            if reported == expected:
                return reported
            if time.monotonic() >= deadline:
                return reported
            time.sleep(POLL_PAUSE_S)

    def requires(self, step: str, pointer: str, expected: object, *, numeric: bool = False) -> None:
        """Fail the smoke unless the machine itself reports `expected`.

        Args:
            step: The verification point this belongs to.
            pointer: Where in a status read the value is.
            expected: What the machine has to report.
            numeric: Whether to compare as numbers rather than as values.

        Raises:
            VerificationError: If the machine never reports it.
        """
        reported = self.settles(pointer, expected, numeric=numeric)
        if reported != expected:
            raise VerificationError(
                step,
                f"the printer reports {reported!r} at `{pointer}` rather than {expected!r}, "
                f"{self.settle_s:g}s after it was asked for",
            )

    # ~~ the sequence, in the order `SEQUENCE` declares and no other

    def step_context(self) -> None:
        """Read the context, and require the printer, the job and the bounds in it.

        Raises:
            VerificationError: If the read failed, if any of the three is missing, or
                if a bound it reports is wider than the configured envelope —
                which is a supervisor that would accept what the operator's own
                configuration forbids.
        """
        answer = self.ask("context")
        if answer.exit != CLIENT_SUCCESS:
            raise VerificationError(
                STEP_CONTEXT, f"the context read exited {answer.exit}: {answer.said}"
            )
        for pointer, what in (("/context/printer", "printer"), ("/context/job", "job")):
            if at(answer.document, pointer) is None:
                raise VerificationError(STEP_CONTEXT, f"the context reports no {what}")
        allowed = at(answer.document, "/context/bounds/allowed")
        if not isinstance(allowed, dict):
            raise VerificationError(STEP_CONTEXT, "the context reports no effective bounds")
        for name, (low, high) in CONSERVATIVE_ENVELOPE.items():
            reported = allowed.get(name)
            bottom = number_at(reported, "/min")
            top = number_at(reported, "/max")
            if bottom is None or top is None:
                raise VerificationError(
                    STEP_CONTEXT, f"the context reports no effective bound for `{name}`"
                )
            if bottom < low or top > high:
                raise VerificationError(
                    STEP_CONTEXT,
                    f"the effective bound for `{name}` is {bottom:g} to {top:g}, wider than "
                    f"the {low:g} to {high:g} the configured envelope allows",
                )
            self.bounds[name] = (bottom, top)

    def step_start_print(self) -> None:
        """Start the smoke print, and require the machine to report it printing.

        Raises:
            VerificationError: If the supervisor refused it or the machine never printed.
        """
        self.act(
            STEP_START,
            "start-print",
            "--file-name",
            FILE_NAME,
            "--manifest",
            json.dumps(self.manifest),
        )
        self.started = True
        self.requires(STEP_START, "/printer/connection", "printing")

    def step_adjust_inside(self) -> None:
        """Set each adjustable just inside its bound and read it back from the printer.

        Raises:
            VerificationError: If a bound is too narrow to step inside, if the printer
                reports no prior value to put back, or if it reports anything
                other than what it was set to.
        """
        for knob in KNOBS:
            low, high = self.bounds[knob.adjustable]
            value = asked(high - knob.step)
            if value < low:
                raise VerificationError(
                    STEP_ADJUST_INSIDE,
                    f"the effective bound for `{knob.adjustable}` is {low:g} to {high:g}, "
                    f"too narrow to step {knob.step:g} inside",
                )
            before = number_at(self.status(), knob.reported_at)
            if before is None:
                raise VerificationError(
                    STEP_ADJUST_INSIDE,
                    f"the printer reports no value for `{knob.adjustable}`, so this run "
                    f"would have nothing to put back",
                )
            self.found.setdefault(knob.adjustable, before)
            self.act(STEP_ADJUST_INSIDE, knob.command, *knob.asking(value))
            self.requires(STEP_ADJUST_INSIDE, knob.reported_at, value, numeric=True)
            say(f"`{knob.adjustable}` is {value:g}, and was {before:g}")

    def step_adjust_outside(self) -> None:
        """Ask for a value just outside each bound, and require nothing to have moved.

        Raises:
            VerificationError: If the supervisor did not refuse it, or if the machine
                reports the refused value afterwards — which is a rejection that
                reached the printer.
        """
        for knob in KNOBS:
            _, high = self.bounds[knob.adjustable]
            value = asked(high + knob.step)
            held = number_at(self.status(), knob.reported_at)
            answer = self.ask(
                knob.command, "--actor", ACTOR, "--reason", REASON, *knob.asking(value)
            )
            recorded = at(answer.document, "/record/id")
            if isinstance(recorded, str):
                self.actions.append(recorded)
            if answer.exit != CLIENT_REJECTED:
                raise VerificationError(
                    STEP_ADJUST_OUTSIDE,
                    f"asking for `{knob.adjustable}` at {value:g}, outside the bound ending "
                    f"{high:g}, exited {answer.exit} rather than being refused: {answer.said}",
                )
            after = number_at(self.status(), knob.reported_at)
            if after != held:
                raise VerificationError(
                    STEP_ADJUST_OUTSIDE,
                    f"the refused request changed the machine: `{knob.adjustable}` was "
                    f"{held!r} before it and is {after!r} after it",
                )
            say(f"`{knob.adjustable}` at {value:g} was refused, and the machine did not move")

    def step_bounded_intervention(self) -> None:
        """Set a bounded change, and require the prior value back when it expires.

        Raises:
            VerificationError: If the change did not take, or if the value it replaced
                was not put back once the bound had passed.
        """
        knob = KNOBS[0]
        low, high = self.bounds[knob.adjustable]
        value = asked(high - 2 * knob.step)
        if value < low:
            raise VerificationError(
                STEP_INTERVENTION,
                f"the effective bound for `{knob.adjustable}` is {low:g} to {high:g}, too "
                f"narrow to step {2 * knob.step:g} inside",
            )
        prior = number_at(self.status(), knob.reported_at)
        self.act(
            STEP_INTERVENTION,
            knob.command,
            *knob.asking(value),
            "--duration-s",
            str(self.duration_s),
        )
        self.requires(STEP_INTERVENTION, knob.reported_at, value, numeric=True)
        say(f"`{knob.adjustable}` is {value:g} for {self.duration_s}s; waiting it out")
        time.sleep(self.duration_s)
        self.requires(STEP_INTERVENTION, knob.reported_at, prior, numeric=True)
        say(f"`{knob.adjustable}` is {prior!r} again, which is what it replaced")

    def step_pause_resume(self) -> None:
        """Pause the print and resume it, requiring the machine to report each.

        Raises:
            VerificationError: If the machine never reports having taken one of them.
        """
        self.act(STEP_PAUSE_RESUME, "pause")
        self.requires(STEP_PAUSE_RESUME, "/printer/connection", "paused")
        self.act(STEP_PAUSE_RESUME, "resume")
        self.requires(STEP_PAUSE_RESUME, "/printer/connection", "printing")

    def step_cancel(self) -> None:
        """Put back what this run changed, cancel the print, and require it gone.

        The restore comes first and deliberately: an adjustment is valid from a
        printing or paused machine and from no other state, so a run that
        cancelled first could never put back what it changed.

        Raises:
            VerificationError: If a value could not be put back, if the cancel was
                refused, or if the machine reports a job still running after it.
        """
        self.restore(STEP_CANCEL)
        self.act(STEP_CANCEL, "cancel")
        self.requires(STEP_CANCEL, "/printer/connection", OPERATIONAL)
        running = at(self.status(), "/job/state")
        if running in RUNNING_STATES:
            raise VerificationError(
                STEP_CANCEL, f"the printer reports the job {running!r} after the cancel"
            )

    def step_history(self) -> None:
        """Read the history, and require every action, decision and outcome in it.

        Raises:
            VerificationError: If the read failed, or if one request this run made is
                not accounted for by a request event and a decision or outcome.
        """
        answer = self.ask("history", "--limit", "500")
        events = at(answer.document, "/events")
        if answer.exit != CLIENT_SUCCESS or not isinstance(events, list):
            raise VerificationError(
                STEP_HISTORY, f"the history read exited {answer.exit}: {answer.said}"
            )
        requested = _action_ids(events, "action_requested")
        decided = _action_ids(events, "action_rejected") | _action_ids(events, "action_executed")
        for action in self.actions:
            if action not in requested:
                raise VerificationError(
                    STEP_HISTORY, f"the history accounts for no request of the action {action}"
                )
            if action not in decided:
                raise VerificationError(
                    STEP_HISTORY,
                    f"the history carries no decision or outcome for the action {action}",
                )
        if not any(_kind_of(event) == "intervention_expired" for event in events):
            raise VerificationError(
                STEP_HISTORY, "the history carries no expiry of the bounded intervention"
            )
        say(f"the history accounts for all {len(self.actions)} actions this run asked for")

    # ~~ leaving the machine as it was found, on every exit path this has

    def restore(self, step: str) -> None:
        """Put every adjustable this run changed back to the value it found there.

        Every adjustable is attempted, and one that could not be put back does
        not cost the rest: a run that gave up on the first failure would leave
        the machine holding every value after it.

        Args:
            step: The verification point to blame a failure on.

        Raises:
            VerificationError: Naming every one that could not be put back,
                after each has been attempted.
        """
        unrestored: list[str] = []
        for knob in KNOBS:
            value = self.found.get(knob.adjustable)
            if value is None:
                continue
            if number_at(self.status(), knob.reported_at) == value:
                continue
            try:
                self.act(step, knob.command, *knob.asking(value))
                self.requires(step, knob.reported_at, value, numeric=True)
            except VerificationError as failure:
                unrestored.append(f"`{knob.adjustable}` ({failure.detail})")
                continue
            say(f"`{knob.adjustable}` is {value:g} again, which is where this run found it")
        if unrestored:
            raise VerificationError(step, f"these were not put back: {', '.join(unrestored)}")

    def clean_up(self) -> None:
        """Cancel the print and put back what this run changed, whatever happened.

        A run that finishes without restoring what it changed leaves the machine
        altered exactly as a crashed one does, so this runs on every exit path —
        and it reports what it could not do rather than raising, because a
        cleanup that failed must not replace the failure it was cleaning up
        after. What it could not put back is left in `left_changed`, which is
        read off the machine afterwards and is what makes the run exit non-zero
        naming it: a green report over a machine still carrying this run's own
        values is the worst answer this program could give.
        """
        if self.cleaned:
            return
        self.cleaned = True
        say("cleaning up: putting back what this run changed, and cancelling the print")
        for what, doing in (
            ("put back what it changed", lambda: self.restore(STEP_CLEANUP)),
            ("cancel the print", self._cancel_quietly),
            ("leave the printer operational", self._confirm_operational),
        ):
            try:
                doing()
            except VerificationError as failure:
                say(f"cleanup could not {what}: {failure.detail}")
        self.left_changed = self.still_changed()

    def still_changed(self) -> list[str]:
        """Everything the machine is holding that this run did not find there.

        Read off the machine rather than inferred from what the cleanup managed
        to do: a restore that was accepted and did not take leaves the machine
        altered exactly as one that was refused, and what a person needs is
        which values are still this run's own.
        """
        document = self.status()
        changed = [
            f"`{knob.adjustable}` is {number_at(document, knob.reported_at)!r} rather than "
            f"the {value:g} this run found there"
            for knob in KNOBS
            for value in [self.found.get(knob.adjustable)]
            if value is not None and number_at(document, knob.reported_at) != value
        ]
        reported = at(document, "/printer/connection")
        if reported != OPERATIONAL:
            changed.append(f"the printer reports {reported!r} rather than `{OPERATIONAL}`")
        return changed

    def _cancel_quietly(self) -> None:
        """Cancel the print, when the machine still reports one to cancel.

        Raises:
            VerificationError: If the cancel was refused.
        """
        if at(self.status(), "/printer/connection") not in RUNNING_STATES:
            return
        self.act(STEP_CLEANUP, "cancel")

    def _confirm_operational(self) -> None:
        """Require the machine to be left operational.

        Raises:
            VerificationError: If it reports anything else.
        """
        self.requires(STEP_CLEANUP, "/printer/connection", OPERATIONAL)


def _kind_of(event: object) -> object:
    """The kind one history event is of."""
    return at(event, "/kind")


def _action_ids(events: Sequence[object], kind: str) -> set[str]:
    """Every action identifier the events of one kind name."""
    found: set[str] = set()
    for event in events:
        if _kind_of(event) != kind:
            continue
        action = at(event, "/payload/action_id")
        if isinstance(action, str):
            found.add(action)
    return found


# ~~ the preconditions, every one of which fails closed


def _command_present(smoke: Smoke) -> str | None:
    """The program this smoke drives is on this host."""
    if shutil.which(smoke.program) is None:
        return (
            f"`{smoke.program}` is not on PATH: install this stack's own command, or name "
            f"another with {PROGRAM_ENV}"
        )
    return None


def _device_readable(smoke: Smoke) -> str | None:
    """The named serial device is there, and this user can read it."""
    device = Path(smoke.device)
    if not device.exists():
        return f"{DEVICE_ENV} names {smoke.device}, which is not there"
    if not os.access(device, os.R_OK):
        return (
            f"{smoke.device} cannot be read by this user: check that it is a serial device "
            f"and that this user is in the `dialout` group"
        )
    return None


def _octoprint_on_the_device(smoke: Smoke) -> str | None:
    """The scripted `OctoPrint` is connected to that device, over a real serial port."""
    record_file = smoke.state_dir / "instance.json"
    if not record_file.is_file():
        return (
            f"no scripted OctoPrint is recorded under {smoke.state_dir}: bring one up with "
            f"`OCTOPRINT_ENV_MODE=serial OCTOPRINT_ENV_DEVICE={smoke.device} just octoprint-up`"
        )
    try:
        record = json.loads(record_file.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return f"{record_file} could not be read: {error}"
    mode = record.get("mode")
    if mode != "serial":
        return (
            f"{record_file} records the `{mode}` mode rather than `serial`: this smoke drives "
            f"the real printer, and a virtual one would prove a virtual one"
        )
    if record.get("device") != smoke.device:
        return (
            f"{record_file} records the device {record.get('device')!r}, and {DEVICE_ENV} "
            f"names {smoke.device!r}"
        )
    url = str(record.get("url", ""))
    key_file = str(record.get("api_key_file", ""))
    try:
        key = Path(key_file).read_text(encoding="utf-8").strip()
        status, body = octoprint_call(url, key, CONNECTION_PATH)
    except (OSError, HTTPException, ValueError) as error:
        return f"{url} could not be asked what it is connected to: {error}"
    if status != 200:
        return f"{url} answered {status} when asked what it is connected to"
    port = at(body, "/current/port")
    if port != smoke.device:
        return (
            f"{url} is connected to {port!r} rather than to {smoke.device!r}: this smoke "
            f"drives the machine that OctoPrint is on"
        )
    smoke.record = record
    return None


def _configuration(smoke: Smoke) -> tuple[object, str | None]:
    """The configuration document, or why it could not be read."""
    if not smoke.config.is_file():
        return None, (
            f"{smoke.config} is not there: name the running supervisor's own file with {CONFIG_ENV}"
        )
    try:
        return tomllib.loads(smoke.config.read_text(encoding="utf-8")), None
    except (OSError, tomllib.TOMLDecodeError) as error:
        return None, f"{smoke.config} could not be read: {error}"


def _bound_to_the_verified_printer(smoke: Smoke) -> str | None:
    """The supervisor this run drives is the one attached to the printer it verified.

    Two links, and the run is refused unless both hold. The configuration this
    smoke reads is the **supervisor's own**, so the `OctoPrint` it names has to
    be the instance whose serial connection and device the precondition before
    this one checked. And every place that names where a supervisor is — that
    file's `listen`, its `[client]` table, and the variable that wins over both —
    has to name one address, because a command sent somewhere else acts on a
    machine nothing here verified. Checking a device on one printer and driving
    another is the failure this smoke exists to make impossible, and it is the
    one an opt-in naming a device cannot catch by itself.
    """
    document, unreadable = _configuration(smoke)
    if unreadable is not None:
        return unreadable

    attached = at(document, "/octoprint/url")
    if not isinstance(attached, str):
        return (
            f"{smoke.config} names no OctoPrint under `octoprint.url`, so it is not the "
            f"running supervisor's own configuration and nothing here can tell which "
            f"printer that supervisor drives"
        )
    verified = smoke.record.get("url")
    if not isinstance(verified, str) or address_of(attached) != address_of(verified):
        return (
            f"{smoke.config} names the supervisor's OctoPrint as {attached!r}, and the "
            f"instance this run verified is at {verified!r}: the printer checked and the "
            f"printer driven would be different machines"
        )

    named: list[tuple[str, str]] = []
    for where, value in (
        (f"{smoke.config}'s `[client] server`", at(document, "/client/server")),
        (f"{smoke.config}'s `listen`", at(document, "/listen")),
        (SERVER_ENV, os.environ.get(SERVER_ENV)),
    ):
        if isinstance(value, str) and value.strip():
            named.append((where, address_of(value)))
    if not named:
        return (
            f"nothing names where the supervisor is: {smoke.config} carries neither "
            f"`listen` nor a `[client] server`, and {SERVER_ENV} is unset"
        )
    disagreeing = [f"{where} names {address}" for where, address in named]
    if len({address for _, address in named}) > 1:
        return (
            f"two things name different supervisors and one of them would be driven: "
            f"{'; '.join(disagreeing)}"
        )
    return None


def _manifest_present(smoke: Smoke) -> str | None:
    """The print this smoke acts on has a manifest, and it is for its own payload."""
    if not smoke.print_id:
        return (
            f"{PRINT_ENV} names no print: a print record is minted by the supervisor, so "
            f"the one this smoke acts on is named rather than guessed"
        )
    answer = smoke.ask("manifest-get")
    if answer.exit != CLIENT_SUCCESS:
        return f"reading the manifest of the print {smoke.print_id} exited {answer.exit}"
    manifest = at(answer.document, "/manifest")
    named = at(manifest, "/file_name")
    if named != FILE_NAME:
        return (
            f"the print {smoke.print_id} carries the manifest {named!r} rather than one for "
            f"{FILE_NAME!r}: set one with `printobserver manifest-set`"
        )
    smoke.manifest = manifest
    return None


def _printer_operational(smoke: Smoke) -> str | None:
    """The printer reports itself operational."""
    reported = at(smoke.status(), "/printer/connection")
    if reported != OPERATIONAL:
        return f"the printer reports {reported!r} rather than `{OPERATIONAL}`"
    return None


def _nothing_is_printing(smoke: Smoke) -> str | None:
    """No job is running and none is paused."""
    reported = at(smoke.status(), "/job/state")
    if reported in RUNNING_STATES:
        return (
            f"the printer reports a job {reported!r}: this smoke starts a print of its own, "
            f"and would be starting it over somebody else's"
        )
    return None


def _conservative_envelope(smoke: Smoke) -> str | None:
    """The configured safety envelope is the conservative one this test ships."""
    document, unreadable = _configuration(smoke)
    if unreadable is not None:
        return unreadable
    allowed = at(document, "/safety/allowed")
    if not isinstance(allowed, dict):
        return f"{smoke.config} configures no safety envelope"
    configured = {
        name: (number_at(range_, "/min"), number_at(range_, "/max"))
        for name, range_ in allowed.items()
    }
    if configured != CONSERVATIVE_ENVELOPE:
        return (
            f"{smoke.config} allows {configured}, and this smoke requires the conservative "
            f"envelope it ships: {CONSERVATIVE_ENVELOPE}"
        )
    return None


#: Every precondition this smoke checks, in the order it checks them.
#:
#: The walk in `tests/test_preconditions.py` is over this tuple, and a test
#: there fails when it declares one the walk does not cover — so the coverage
#: cannot fall behind what this program actually requires.
PRECONDITIONS: tuple[tuple[str, Callable[[Smoke], str | None]], ...] = (
    ("printobserver-command", _command_present),
    ("serial-device", _device_readable),
    ("octoprint-serial-mode", _octoprint_on_the_device),
    ("supervisor-binding", _bound_to_the_verified_printer),
    ("smoke-manifest", _manifest_present),
    ("printer-operational", _printer_operational),
    ("no-job-running", _nothing_is_printing),
    ("safety-envelope", _conservative_envelope),
)


def smoke_of(environ: dict[str, str], device: str) -> Smoke:
    """One run, configured from the environment it was started in.

    Args:
        environ: The environment the recipe ran under.
        device: The serial device the selection named.

    Returns:
        The run, before any precondition has been checked.
    """
    return Smoke(
        device=device,
        config=Path(environ.get(CONFIG_ENV, DEFAULT_CONFIG)),
        print_id=environ.get(PRINT_ENV, "").strip(),
        program=environ.get(PROGRAM_ENV, PROGRAM),
        state_dir=Path(environ.get("OCTOPRINT_ENV_STATE_DIR", OCTOPRINT_STATE_DIR)),
        settle_s=float(environ.get(SETTLE_ENV, DEFAULT_SETTLE_S)),
        duration_s=int(environ.get(DURATION_ENV, DEFAULT_DURATION_S)),
    )


def main(argv: Sequence[str] | None = None) -> int:
    """Select, refuse or run, and leave the machine as it was found.

    Args:
        argv: The arguments the recipe passed on, or this process's own.

    Returns:
        Zero when this test was not selected, when a precondition refused it and
        when it passed and left the machine as it found it; one when a
        verification point was not satisfied, when the run was interrupted, and
        when the cleanup could not put back something this run changed — the
        last of those whether or not anything else went wrong, because a green
        report over a machine still holding this run's own values is the worst
        answer this program could give.
    """
    arguments = list(sys.argv[1:] if argv is None else argv)
    environ = dict(os.environ)

    selection = select(arguments, environ)
    if not selection.selected:
        say("not selected — nothing runs this by accident, and two things together run it:")
        for missing in selection.missing:
            say(f"  {missing}")
        say(f"  select it with `{DEVICE_ENV}=/dev/ttyACM0 just test-printer-smoke {FLAG}`")
        return 0

    smoke = smoke_of(environ, selection.device)
    for name, precondition in PRECONDITIONS:
        unmet = precondition(smoke)
        if unmet is not None:
            say(f"refused: the precondition `{name}` is not met: {unmet}")
            say("nothing was asked of the printer.")
            return 0
        say(f"precondition `{name}` is met")

    say("every precondition is met. This run drives the printer — stay next to it.")
    steps: dict[str, Callable[[], None]] = {
        STEP_CONTEXT: smoke.step_context,
        STEP_START: smoke.step_start_print,
        STEP_ADJUST_INSIDE: smoke.step_adjust_inside,
        STEP_ADJUST_OUTSIDE: smoke.step_adjust_outside,
        STEP_INTERVENTION: smoke.step_bounded_intervention,
        STEP_PAUSE_RESUME: smoke.step_pause_resume,
        STEP_CANCEL: smoke.step_cancel,
        STEP_HISTORY: smoke.step_history,
    }
    outcome = 0
    try:
        for step in SEQUENCE:
            say(f"verifying `{step}`")
            steps[step]()
    except VerificationError as failure:
        # Said here rather than after the cleanup, so that what a reader meets
        # first is the failure this run is about. What the cleanup could not do
        # is reported beneath it and never in its place.
        say(f"FAILED at `{failure.step}`: {failure.detail}")
        outcome = 1
    except KeyboardInterrupt:
        say("interrupted: putting the machine back before stopping")
        outcome = 1
    finally:
        smoke.clean_up()

    for still in smoke.left_changed:
        say(f"LEFT CHANGED: {still}")
    if smoke.left_changed:
        say("this run did not leave the machine as it found it. Put it right before printing.")
        return 1
    if outcome != 0:
        return outcome
    say(f"passed: this stack drove {smoke.device} through {', '.join(SEQUENCE)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
