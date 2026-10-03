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
import math
import re
import shlex
import tomllib
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from functools import cache
from pathlib import Path
from typing import Any, Literal, NewType, TypeGuard, cast

from jsonschema import Draft202012Validator
from jsonschema.protocols import Validator
from surface import PROGRAM, REPO, Field, duration_bounds, invocation_pattern, surface

CASES = REPO / "tests" / "real-prints"
SERVICE_CONFIG = CASES / "service-config.toml"
SKILL = REPO / "skills" / "printobserver"
TURN_PROMPT = REPO / "crates" / "printobserver-oneharness" / "assets" / "turn-prompt.md"
PROMPT_SLOTS_SOURCE = REPO / "crates" / "printobserver-oneharness" / "src" / "prompt.rs"
COMMON_OPERATIONS = SKILL / "reference" / "common-operations.md"
SERVICE_INSTALLER = REPO / "scripts" / "install-service.sh"
OBICO_SAMPLE = REPO / "crates" / "printobserver-obico" / "samples" / "obico" / "failure-alert.json"
SCHEMAS = REPO / "schemas"
DECISION = REPO / "crates" / "printobserver-core" / "src" / "decision.rs"
ADJUSTABLE = REPO / "crates" / "printobserver-printer-api" / "src" / "adjustable.rs"

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
    given: tuple[tuple[str, str], ...]
    # The directory the shell ran it in, which a file an option names is read from.
    cwd: Path | None = None

    @property
    def options(self) -> dict[str, str]:
        """Each option the invocation names, by its name; the last wins where one repeats."""
        return dict(self.given)

    @property
    def words(self) -> list[str]:
        """The arguments after the program's name, as the program receives them."""
        return [self.operation, *(w for name, value in self.given for w in _spelled(name, value))]

    def describe(self) -> str:
        """The invocation, with its long values cut so a failure stays readable."""
        shown = []
        for name, value in self.options.items():
            text = value if len(value) <= 60 else value[:57] + "..."
            shown.append(f"--{name.replace('_', '-')} {shlex.quote(text)}")
        return " ".join([PROGRAM, self.operation, *shown])


def _spelled(name: str, value: str) -> list[str]:
    option = "--" + name.replace("_", "-")
    return [option, value] if value else [option]


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


@cache
def _validator(schema: Path) -> Validator:
    return Draft202012Validator(json.loads(schema.read_text(encoding="utf-8")))


def _validated(path: Path, schema: Path) -> dict[str, Any]:
    """A JSON file's document, once it satisfies the schema that declares it.

    Raises:
        ValueError: If it does not, naming where it first departs from the schema.
    """
    return conforming(json.loads(path.read_text(encoding="utf-8")), schema, str(path))


def conforming(document: dict[str, Any], schema: Path, where: str) -> dict[str, Any]:
    """A document, once it satisfies the schema that declares it.

    Raises:
        ValueError: If it does not, naming where it first departs from the schema.
    """
    for error in sorted(_validator(schema).iter_errors(document), key=str):
        msg = f"{where} at {error.json_path} does not satisfy {schema.name}: {error.message}"
        raise ValueError(msg)
    return document


def read_case(case: str) -> dict[str, Any]:
    """One case's `case.json`, held to `case.schema.json`."""
    return _validated(CASES / case / "case.json", CASES / "case.schema.json")


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
    return _validated(path, SCHEMAS / "printobserver-server" / "HistoryAnswer.json")["events"]


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


def _finite(value: object) -> TypeGuard[float]:
    """Whether a value is a finite number, which TOML's booleans, `nan` and `inf` are not."""
    return isinstance(value, int | float) and not isinstance(value, bool) and math.isfinite(value)


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
        if not _finite(low) or not _finite(high) or low > high:
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


_PUNCTUATION = ";&|()"
# The words after which a shell reads the next word as a command.
_KEYWORDS = {"do", "then", "else"}


def _is_separator(word: str) -> bool:
    """Whether a word is the shell's punctuation, which the lexer runs together (`);`)."""
    return bool(word) and set(word) <= set(_PUNCTUATION)


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
    lexer = shlex.shlex(_lines_joined(command), posix=True, punctuation_chars=_PUNCTUATION)
    lexer.whitespace_split = True
    try:
        return list(lexer)
    except ValueError:
        # An unbalanced quote: the shell refuses the whole command, so it runs nothing.
        return []


def _is_program(word: str) -> bool:
    name = re.split(r"[\\/]", word)[-1].lower()
    return name in {PROGRAM, f"{PROGRAM}.exe"}


def _at_command_position(words: list[str], index: int) -> bool:
    """Whether the word at `index` is a command a shell runs, not an argument of one."""
    position = index - 1
    while position >= 0 and re.fullmatch(r"\w+=.*", words[position]):
        position -= 1
    return position < 0 or _is_separator(words[position]) or words[position] in _KEYWORDS


def _options(words: list[str]) -> tuple[tuple[str, str], ...]:
    """The `--name value` options and lone flags of one invocation, in order, up to its end."""
    options: list[tuple[str, str]] = []
    position = 0
    while position < len(words):
        current = words[position]
        if _is_separator(current) or _is_program(current):
            break
        if current.startswith("--"):
            name = current[2:].replace("-", "_")
            following = words[position + 1] if position + 1 < len(words) else None
            if following is None or following.startswith("--") or _is_separator(following):
                options.append((name, ""))
                position += 1
                continue
            options.append((name, following))
            position += 2
            continue
        # A word that is no option's value, which the program refuses.
        options.append(("", current))
        position += 1
    return tuple(options)


def commands_in(shell_command: str, cwd: Path | None = None) -> list[Command]:
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
        if operation.startswith("-") or _is_separator(operation):
            continue
        found.append(Command(operation=operation, given=_options(words[index + 2 :]), cwd=cwd))
    return found


def commands_written(shell_commands: list[str], cwd: Path | None = None) -> list[Command]:
    """Every `printobserver` invocation written in the shell commands the agent ran, in order.

    It reads what was written rather than what the shell went on to execute: an
    invocation in a branch the shell skips is counted, as the hook-side spies
    count it too.
    """
    return [command for shell in shell_commands for command in commands_in(shell, cwd)]


# The characters an identifier this system mints is made of, which a value
# bound for a request's path must keep to (`parse.rs`'s `check`).
_IDENTIFIER = re.compile(r"[A-Za-z0-9._~-]+")


def sent_to_server(command: Command) -> bool:
    """Whether the program would parse an invocation and send it to the supervisor.

    The rules are the program's own parser's (`parse.rs`): one of its commands
    that asks a server, not one it carries out locally; neither `--help` nor
    `--version`, which it answers at once; no word that is no option's value;
    every option one of that command's or `--json` or `--config`, a value given
    once; a number
    or a whole number that reads as one; a file an option names readable, from
    the directory the command ran in; a value bound for the request's path
    an identifier; a duration inside the bounds `surface.rs` declares; and
    every value the command requires. A value the shell expands (`"$A"`) is
    taken as given. What the supervisor then makes of the request is the
    stubs'. `skilltest-wiring` holds this to what the built program does.
    """
    spec = surface().command(command.operation)
    if spec is None or spec.operation is None or {"help", "version"} & set(command.options):
        return False
    forms = {form: field for field in spec.fields for form in field.forms}
    supplied: set[str] = set()
    for name, value in command.given:
        option = "--" + name.replace("_", "-")
        if option == "--json" and not value:
            continue
        if option == "--config":
            if not value or "config" in supplied:
                return False
            supplied.add("config")
            continue
        field = forms.get(option)
        if field is None or not value or field.name in supplied:
            return False
        supplied.add(field.name)
        if "$" in value:
            continue
        if option in field.file_forms:
            named = Path(value) if command.cwd is None else command.cwd / value
            if not named.is_file() or not _readable(field, named.read_text(encoding="utf-8")):
                return False
        elif not _readable(field, value):
            return False
    return all(field.name in supplied for field in spec.fields if field.required)


# What Rust's `f64` and `i64` parsers read, which is narrower than Python's
# `float` and `int`: no digit separators and no digits outside ASCII.
_RUST_FLOAT = re.compile(r"[+-]?(?:inf|infinity|nan|(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)", re.I)
_RUST_INTEGER = re.compile(r"[+-]?\d+")
_I64 = range(-(2**63), 2**63)


def _readable(field: Field, value: str) -> bool:
    """Whether the parser reads one value for one field (`parse.rs`'s `read_value`)."""
    text = value.strip()
    number: float | None = None
    match field.kind:
        case "number":
            if not text.isascii() or not _RUST_FLOAT.fullmatch(text):
                return False
            number = float(text)
        case "integer":
            if not text.isascii() or not _RUST_INTEGER.fullmatch(text) or int(text) not in _I64:
                return False
            number = int(text)
        case _:
            pass
    if field.located == "path" and not _IDENTIFIER.fullmatch(value):
        return False
    if field.name == "duration_s" and field.located == "body":
        low, high = duration_bounds()
        return number is not None and low <= number <= high
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
    if command.operation != step.operation or not sent_to_server(command):
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
