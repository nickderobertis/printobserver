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
from repo_checks import platforms
from repo_checks.model import Repo
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

LOCK = REPO / "uv.lock"
#: The locked distribution each upstream program a run drives arrives in.
DISTRIBUTIONS = {"skilltest": "skilltest-sdk", "oneharness": "oneharness-cli"}


@cache
def publishing(program: str) -> frozenset[str]:
    """The supported platforms the locked release of `program` carries a build for.

    Read off the platform tags of the wheels `uv.lock` records for its
    distribution, which are what the registry serves for that release. A pure
    wheel names no platform and counts for none: `skilltest-sdk`'s is the SDK
    without the program its platform wheels bundle.
    """
    lock = tomllib.loads(LOCK.read_text(encoding="utf-8"))
    (package,) = [p for p in lock["package"] if p["name"] == DISTRIBUTIONS[program]]
    tags = {
        tag
        for wheel in package.get("wheels", [])
        for tag in Path(wheel["url"]).stem.rsplit("-", 1)[-1].split(".")
    }
    return frozenset(
        platform.id
        for platform in platforms.supported(Repo(REPO))
        if any(
            tag.startswith(platform.naming.wheel_family)
            and tag.endswith(f"_{platform.naming.wheel_machine}")
            for tag in tags
        )
    )


def unpublished_reason(program: str) -> str:
    """Why a test needing `program` cannot run here: the upstream artifact that is absent."""
    lock = tomllib.loads(LOCK.read_text(encoding="utf-8"))
    distribution = DISTRIBUTIONS[program]
    (version,) = [p["version"] for p in lock["package"] if p["name"] == distribution]
    artifact = {
        "skilltest": "platform wheel bundling the skilltest binary",
        "oneharness": "wheel carrying the oneharness binary",
    }[program]
    return (
        f"{distribution} {version}, as locked, publishes no {artifact} for "
        f"{platforms.host(Repo(REPO)).id}"
    )


def published(program: str) -> bool:
    """Whether `program` publishes a build for the supported platform this host is.

    A test that needs one skips where it does not and nowhere else: on every
    other host a missing program is a failure.
    """
    return platforms.host(Repo(REPO)).id in publishing(program)


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
    # The options whose value the shell expands before the program reads it.
    expanded: frozenset[str] = frozenset()

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


@dataclass(frozen=True)
class Word:
    """One word of a shell command, and whether the shell expands something in it."""

    text: str
    expands: bool = False


class _Lexer:
    """Reads a shell command into words as a POSIX shell splits it, for the subset agents write.

    Single quotes keep everything literal; double quotes keep all but `$`, which
    expands; a backslash keeps the next character, and before a line break
    continues the line; an unquoted line break separates commands, as does a
    run of `;&|()`. A `$` outside single quotes is an expansion.
    """

    def __init__(self, command: str) -> None:
        self.command = command
        self.words: list[Word] = []
        self.text: list[str] = []
        self.expands = False
        self.started = False

    def end_word(self) -> None:
        if self.started:
            self.words.append(Word("".join(self.text), self.expands))
        self.text, self.expands, self.started = [], False, False

    def separator(self, mark: str) -> None:
        self.end_word()
        last = self.words[-1] if self.words else None
        if last is not None and _is_separator(last.text) and not last.expands and mark != "\n":
            self.words[-1] = Word(last.text + mark)
        else:
            self.words.append(Word(";" if mark == "\n" else mark))

    def read(self) -> list[Word] | None:
        command, position = self.command, 0
        while position < len(command):
            match command[position]:
                case " " | "\t":
                    self.end_word()
                case character if character == "\n" or character in _PUNCTUATION:
                    self.separator(character)
                case "\\":
                    position += 1
                    if position < len(command) and command[position] != "\n":
                        self.text.append(command[position])
                        self.started = True
                case "'":
                    closing = command.find("'", position + 1)
                    if closing < 0:
                        return None
                    self.text.append(command[position + 1 : closing])
                    self.started, position = True, closing
                case '"':
                    position = self.double_quoted(position)
                    if position < 0:
                        return None
                case character:
                    self.expands = self.expands or character == "$"
                    self.text.append(character)
                    self.started = True
            position += 1
        self.end_word()
        return self.words

    def double_quoted(self, position: int) -> int:
        """Read a double-quoted span from its opening quote; the closing quote's position."""
        command = self.command
        self.started = True
        position += 1
        while position < len(command) and command[position] != '"':
            character = command[position]
            if (
                character == "\\"
                and position + 1 < len(command)
                and command[position + 1] in '"\\$`'
            ):
                position += 1
                character = command[position]
            elif character == "$":
                self.expands = True
            self.text.append(character)
            position += 1
        return position if position < len(command) else -1


def _words(command: str) -> list[Word]:
    """A command's words; none for one with an unbalanced quote, which the shell refuses."""
    return _Lexer(command).read() or []


def _is_program(word: str) -> bool:
    name = re.split(r"[\\/]", word)[-1].lower()
    return name in {PROGRAM, f"{PROGRAM}.exe"}


def _at_command_position(words: list[Word], index: int) -> bool:
    """Whether the word at `index` is a command a shell runs, not an argument of one."""
    position = index - 1
    while position >= 0 and re.fullmatch(r"\w+=.*", words[position].text, re.DOTALL):
        position -= 1
    if position < 0:
        return True
    return _is_separator(words[position].text) or words[position].text in _KEYWORDS


def _options(words: list[Word]) -> tuple[tuple[tuple[str, str], ...], frozenset[str]]:
    """An invocation's options in order, up to its end, and the ones whose value expands."""
    options: list[tuple[str, str]] = []
    expanded: set[str] = set()
    position = 0
    while position < len(words):
        current = words[position].text
        if _is_separator(current) or _is_program(current):
            break
        if current.startswith("--"):
            name = current[2:].replace("-", "_")
            following = words[position + 1] if position + 1 < len(words) else None
            if (
                following is None
                or following.text.startswith("--")
                or _is_separator(following.text)
            ):
                options.append((name, ""))
                position += 1
                continue
            options.append((name, following.text))
            if following.expands:
                expanded.add(name)
            position += 2
            continue
        # A word that is no option's value, which the program refuses.
        options.append(("", current))
        position += 1
    return tuple(options), frozenset(expanded)


def commands_in(shell_command: str, cwd: Path | None = None) -> list[Command]:
    """Every `printobserver <command>` invocation a shell command makes, in order.

    The program names its command first and every option after it as
    `--name value` or a lone flag, which is the only grammar it parses.
    """
    words = _words(shell_command)
    found = []
    for index, word in enumerate(words):
        if not _is_program(word.text) or not _at_command_position(words, index):
            continue
        if index + 1 >= len(words):
            continue
        operation = words[index + 1].text
        if operation.startswith("-") or _is_separator(operation):
            continue
        given, expanded = _options(words[index + 2 :])
        found.append(Command(operation=operation, given=given, cwd=cwd, expanded=expanded))
    return found


def begins_with_program(shell_command: str) -> bool:
    """Whether a shell command's first word is the program, which is all a turn may run."""
    words = _words(shell_command)
    return bool(words) and words[0].text == PROGRAM


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
    every value the command requires. A value the shell expands (`"$A"`, not
    `'$A'`) is taken as given, since what it expands to is the shell's. What the
    supervisor then makes of the request is the stubs'. `skilltest-wiring`
    holds this to what the built program does.
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
        if name in command.expanded:
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
