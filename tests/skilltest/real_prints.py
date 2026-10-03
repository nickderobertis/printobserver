"""The real-print cases, read for the skill tests, and the rule a run is judged by.

Every scenario, every look and every step comes from a case's `case.json`
`assertions`, read when the tests run: nothing here holds a copy of what any
case expects. `tests/real-prints/README.md` states how a run is judged, and the
two halves of that rule are here:

- an outcome is met when its steps occur in order among the `printobserver`
  commands the agent ran ([`outcome_met`]), a step matching a command that names
  each of its `args` with that value, numbers compared numerically
  ([`step_matches`]);
- no command may match a `never` step, which the tier hands skilltest as a
  `not_called` eval over a spy whose pattern is [`step_pattern`].

`tools/repo-checks`'s `checks_real_prints` already holds every case to its
schema, its files, its history and the operations it names, so this module
reads them as valid; the recorded turns it replays are not held by any check,
so [`agent_turn`] reads one only once it has the shape this module reads.
"""

from __future__ import annotations

import json
import re
import shlex
import tomllib
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any, Literal, NewType, cast

from surface import PROGRAM, REPO, invocation_pattern, surface

CASES = REPO / "tests" / "real-prints"
SERVICE_CONFIG = CASES / "service-config.toml"
SKILL = REPO / "skills" / "printobserver"
TURN_PROMPT = REPO / "crates" / "printobserver-oneharness" / "assets" / "turn-prompt.md"
PROMPT_SLOTS_SOURCE = REPO / "crates" / "printobserver-oneharness" / "src" / "prompt.rs"
COMMON_OPERATIONS = SKILL / "reference" / "common-operations.md"
SERVICE_INSTALLER = REPO / "scripts" / "install-service.sh"
OBICO_SAMPLE = REPO / "crates" / "printobserver-obico" / "samples" / "obico" / "failure-alert.json"
SCHEMAS = REPO / "schemas"

CaseName = NewType("CaseName", str)
ScenarioId = NewType("ScenarioId", str)
EventId = NewType("EventId", str)
Trigger = Literal["recorded_alert", "synthetic_alert", "start_request"]


@dataclass(frozen=True)
class Step:
    """One `printobserver` command a case names: its operation and the args it pins."""

    operation: str
    args: dict[str, str | float]

    def describe(self) -> str:
        """The step as a reader of a failure would write it."""
        pinned = " ".join(
            f"--{name.replace('_', '-')} {value}" for name, value in self.args.items()
        )
        return f"{self.operation} {pinned}".strip()


@dataclass(frozen=True)
class Look:
    """What one `printobserver look` answers: a frame, and the events it delivers."""

    image: str
    arrived_event_ids: tuple[EventId, ...]


@dataclass(frozen=True)
class Scenario:
    """One situation a case puts the agent in, as its `assertions` declare it."""

    case: CaseName
    id: ScenarioId
    summary: str
    trigger: Trigger
    event_id: EventId | None
    event_image: str
    printer_state: str
    detector_warned: bool | None
    detector_paused_the_print: bool | None
    looks: tuple[Look, ...]
    accept_any_of: tuple[tuple[Step, ...], ...]
    never: tuple[Step, ...]

    @property
    def directory(self) -> Path:
        """The case directory every file the scenario names is in."""
        return CASES / self.case

    @property
    def test_id(self) -> str:
        """The id one test of this scenario is collected under."""
        return f"{self.case}/{self.id}"


@dataclass(frozen=True)
class Command:
    """One `printobserver` invocation inside a command the agent ran."""

    operation: str
    options: dict[str, str]

    def describe(self) -> str:
        """The invocation, with its long values cut so a failure stays readable."""
        shown = []
        for name, value in self.options.items():
            text = value if len(value) <= 60 else value[:57] + "..."
            shown.append(f"--{name.replace('_', '-')} {shlex.quote(text)}")
        return " ".join([PROGRAM, self.operation, *shown])


def _step(document: dict[str, Any]) -> Step:
    return Step(operation=document["operation"], args=dict(document.get("args", {})))


def _scenario(case: str, document: dict[str, Any]) -> Scenario:
    return Scenario(
        case=CaseName(case),
        id=ScenarioId(document["id"]),
        summary=document["summary"],
        trigger=cast("Trigger", document["trigger"]),
        event_id=EventId(document["event_id"]) if "event_id" in document else None,
        event_image=document["event_image"],
        printer_state=document["printer_state"],
        detector_warned=document["detector_warned"],
        detector_paused_the_print=document["detector_paused_the_print"],
        looks=tuple(
            Look(
                image=look["image"],
                arrived_event_ids=tuple(EventId(i) for i in look.get("arrived_event_ids", [])),
            )
            for look in document["looks"]
        ),
        accept_any_of=tuple(
            tuple(_step(step) for step in outcome) for outcome in document["accept_any_of"]
        ),
        never=tuple(_step(step) for step in document["never"]),
    )


def read_case(case: str) -> dict[str, Any]:
    """One case's `case.json`."""
    return json.loads((CASES / case / "case.json").read_text(encoding="utf-8"))


def scenarios() -> list[Scenario]:
    """Every scenario of every case on disk, so a case added later is covered unedited."""
    found = []
    for path in sorted(CASES.glob("*/case.json")):
        document = read_case(path.parent.name)
        found.extend(
            _scenario(path.parent.name, scenario)
            for scenario in document["assertions"]["scenarios"]
        )
    return found


def history(case: str) -> list[dict[str, Any]]:
    """The events printobserver recorded for a case's print, newest first; none without one."""
    path = CASES / case / "printobserver-history.json"
    if not path.is_file():
        return []
    return json.loads(path.read_text(encoding="utf-8"))["events"]


@dataclass(frozen=True)
class RecordedCommand:
    """One shell command a recorded turn ran, and what it was answered."""

    command: str
    answered: str
    failed: bool


def recorded_step(step: object, where: str) -> RecordedCommand | None:
    """A recorded step's shell command, or none for a step that ran no shell.

    Raises:
        ValueError: If a shell step is not in the shape a recorded turn writes one.
    """
    if not isinstance(step, dict) or step.get("tool") != "Bash":
        return None
    given = step.get("input")
    command = given.get("command") if isinstance(given, dict) else None
    result = step.get("result")
    if (
        not isinstance(command, str)
        or not isinstance(result, dict)
        or not isinstance(result.get("is_error"), bool)
        or not isinstance(result.get("content"), str)
    ):
        msg = f"{where} records a shell step without a command, an error flag and its answer"
        raise ValueError(msg)
    return RecordedCommand(command=command, answered=result["content"], failed=result["is_error"])


def agent_turn(case: str) -> list[RecordedCommand] | None:
    """The shell commands a case's recorded turn ran, in order, when it has one.

    Raises:
        ValueError: If the recorded turn is not in the shape `agent-turn.json` is written in.
    """
    path = CASES / case / "agent-turn.json"
    if not path.is_file():
        return None
    document = json.loads(path.read_text(encoding="utf-8"))
    steps = document.get("steps") if isinstance(document, dict) else None
    if not isinstance(steps, list):
        msg = f"{path} carries no list of steps"
        raise ValueError(msg)
    found = [recorded_step(step, str(path)) for step in steps]
    return [command for command in found if command is not None]


@dataclass(frozen=True)
class Bound:
    """The range one adjustable may be set to."""

    min: float
    max: float


def bounds() -> dict[str, Bound]:
    """What the configuration these prints ran under lets each adjustable be set to.

    Raises:
        ValueError: If its `[safety.allowed]` is not a range of numbers per adjustable.
    """
    config = tomllib.loads(SERVICE_CONFIG.read_text(encoding="utf-8"))
    allowed = config.get("safety", {}).get("allowed")
    if not isinstance(allowed, dict) or not allowed:
        msg = f"{SERVICE_CONFIG} allows no adjustable under [safety.allowed]"
        raise ValueError(msg)
    found = {}
    for name, bound in allowed.items():
        low = bound.get("min") if isinstance(bound, dict) else None
        high = bound.get("max") if isinstance(bound, dict) else None
        if not isinstance(low, int | float) or not isinstance(high, int | float) or low > high:
            msg = f"{SERVICE_CONFIG}'s [safety.allowed] {name} is not a range from min to max"
            raise ValueError(msg)
        found[name] = Bound(min=float(low), max=float(high))
    return found


def shipped_model() -> str | None:
    """The model the shipped configuration pins for supervision turns, or none.

    The installer writes the configuration a host starts from, and its
    `[supervisor]` table is where a pinned model would be. When it pins none, a
    production turn runs on the harness's own default, and so does this tier.

    Raises:
        ValueError: If the installer writes no `[supervisor]` table to read.
    """
    text = SERVICE_INSTALLER.read_text(encoding="utf-8")
    table = re.search(r"^\[supervisor\]\n(?P<body>.*?)(?=^\[)", text, re.MULTILINE | re.DOTALL)
    if table is None:
        msg = f"{SERVICE_INSTALLER} writes no [supervisor] table"
        raise ValueError(msg)
    model = re.search(r'^model\s*=\s*"(?P<name>[^"]*)"', table["body"], re.MULTILINE)
    return model["name"] if model else None


_SEPARATORS = {";", "&&", "||", "|", "&", "(", ")"}
# The words after which a shell reads the next word as a command.
_COMMAND_POSITION = {*_SEPARATORS, "do", "then", "else"}


def _lines_joined(command: str) -> str:
    """A command with every unquoted line break read as the separator a shell reads it as.

    A line break inside quotes is part of a word, and one escaped by a
    backslash continues the line.
    """
    out = []
    quote: str | None = None
    escaped = False
    for character in command:
        if escaped:
            escaped = False
            out.append("" if character == "\n" and quote is None else "\\" + character)
            continue
        if character == "\\" and quote != "'":
            escaped = True
            continue
        if character in "'\"" and quote in {None, character}:
            quote = None if quote else character
        out.append(" ; " if character == "\n" and quote is None else character)
    return "".join(out)


def _words(command: str) -> list[str]:
    lexer = shlex.shlex(_lines_joined(command), posix=True, punctuation_chars=";&|()")
    lexer.whitespace_split = True
    try:
        return list(lexer)
    except ValueError:
        # An unbalanced quote: the words are still worth reading for a command.
        return command.split()


def _is_program(word: str) -> bool:
    name = re.split(r"[\\/]", word)[-1].lower()
    return name in {PROGRAM, f"{PROGRAM}.exe"}


def _at_command_position(words: list[str], index: int) -> bool:
    """Whether the word at `index` is a command a shell runs, not an argument of one."""
    position = index - 1
    while position >= 0 and re.fullmatch(r"\w+=.*", words[position]):
        position -= 1
    return position < 0 or words[position] in _COMMAND_POSITION


def _options(words: list[str]) -> dict[str, str]:
    """The `--name value` options and lone flags of one invocation, up to where it ends."""
    options: dict[str, str] = {}
    position = 0
    while position < len(words):
        current = words[position]
        if current in _SEPARATORS or _is_program(current):
            break
        if current.startswith("--"):
            name = current[2:].replace("-", "_")
            following = words[position + 1] if position + 1 < len(words) else None
            if following is None or following.startswith("--") or following in _SEPARATORS:
                options[name] = ""
                position += 1
                continue
            options[name] = following
            position += 2
            continue
        position += 1
    return options


def commands_in(shell_command: str) -> list[Command]:
    """Every `printobserver <command>` invocation a shell command makes, in order.

    The program names its command first and every option after it as
    `--name value` or a lone flag, which is the only grammar it parses.
    """
    words = _words(shell_command)
    found = []
    for index, word in enumerate(words):
        if not _is_program(word) or not _at_command_position(words, index):
            continue
        if index + 1 >= len(words):
            continue
        operation = words[index + 1]
        if operation.startswith("-") or operation in _SEPARATORS:
            continue
        found.append(Command(operation=operation, options=_options(words[index + 2 :])))
    return found


def commands_ran(shell_commands: list[str]) -> list[Command]:
    """Every `printobserver` invocation across the shell commands the agent ran, in order."""
    return [command for shell in shell_commands for command in commands_in(shell)]


def takes_effect(command: Command) -> bool:
    """Whether the program would carry an invocation out rather than refuse it or answer at once.

    It is one of the program's commands; it asks for neither the usage nor the
    version, which the program answers without carrying anything out; every
    option it names is one that command or every command takes; a structured
    value given inline is JSON; and it supplies every value the command
    requires, by any of that value's forms. What the server then makes of the
    values is the server's, and the stubs stand in for it.
    """
    spec = surface().command(command.operation)
    if spec is None or {"help", "version"} & set(command.options):
        return False
    forms = {form: field for field in spec.fields for form in field.forms}
    given = {f"--{name.replace('_', '-')}": value for name, value in command.options.items()}
    for option, value in given.items():
        if option in surface().global_options:
            continue
        field = forms.get(option)
        if field is None:
            return False
        if field.structured and option == field.forms[0] and not _is_json(value):
            return False
    named = {option for option, value in given.items() if value}
    return all(
        any(form in named for form in field.forms) for field in spec.fields if field.required
    )


def _is_json(value: str) -> bool:
    try:
        json.loads(value)
    except ValueError:
        return False
    return True


def _same_value(expected: str | float, given: str) -> bool:
    if isinstance(expected, str):
        return given == expected
    try:
        return Decimal(given) == Decimal(str(expected))
    except InvalidOperation:
        return False


def step_matches(step: Step, command: Command) -> bool:
    """Whether a command takes the step: carried out, naming each of its args with that value."""
    if command.operation != step.operation or not takes_effect(command):
        return False
    return all(
        name in command.options and _same_value(expected, command.options[name])
        for name, expected in step.args.items()
    )


def outcome_met(outcome: tuple[Step, ...], ran: list[Command]) -> bool:
    """Whether the outcome's steps occur in order among the commands, others between them."""
    position = 0
    for command in ran:
        if position < len(outcome) and step_matches(outcome[position], command):
            position += 1
    return position == len(outcome)


def met_outcome(scenario: Scenario, ran: list[Command]) -> tuple[Step, ...] | None:
    """The first acceptable outcome the commands meet, or none."""
    return next((outcome for outcome in scenario.accept_any_of if outcome_met(outcome, ran)), None)


def required_steps(scenario: Scenario) -> list[Step]:
    """The steps every acceptable outcome asks for, which no passing run can skip."""
    first, *others = scenario.accept_any_of
    return [step for step in first if all(step in outcome for outcome in others)]


def step_pattern(step: Step) -> str:
    """The hook-side pattern of a command taking a step (`surface.invocation_pattern`)."""
    return invocation_pattern(step.operation, step.args)
