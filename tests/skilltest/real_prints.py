"""The real-print cases, read for the skill tests, and the rules they are judged by.

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
reads them as valid and does not check them a second time.
"""

from __future__ import annotations

import json
import re
import shlex
import tomllib
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from itertools import permutations
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[2]
CASES = REPO / "tests" / "real-prints"
SERVICE_CONFIG = CASES / "service-config.toml"
SKILL = REPO / "skills" / "printobserver"
TURN_PROMPT = REPO / "crates" / "printobserver-oneharness" / "assets" / "turn-prompt.md"
PROMPT_SLOTS_SOURCE = REPO / "crates" / "printobserver-oneharness" / "src" / "prompt.rs"
OPERATIONS = REPO / "schemas" / "printobserver-server" / "operations.json"
COMMON_OPERATIONS = SKILL / "reference" / "common-operations.md"
SERVICE_INSTALLER = REPO / "scripts" / "install-service.sh"
OBICO_SAMPLE = REPO / "crates" / "printobserver-obico" / "samples" / "obico" / "failure-alert.json"

PROGRAM = "printobserver"


@dataclass(frozen=True)
class Step:
    """One `printobserver` command a case names: its operation and the args it pins."""

    operation: str
    args: dict[str, str | int | float]

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
    arrived_event_ids: tuple[str, ...]


@dataclass(frozen=True)
class Scenario:
    """One situation a case puts the agent in, as its `assertions` declare it."""

    case: str
    id: str
    summary: str
    trigger: str
    event_id: str | None
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
        """The invocation, with its long reasons cut so a failure stays readable."""
        shown = []
        for name, value in self.options.items():
            text = value if len(value) <= 60 else value[:57] + "..."
            shown.append(f"--{name.replace('_', '-')} {shlex.quote(text)}")
        return " ".join([PROGRAM, self.operation, *shown])


def _step(document: dict[str, Any]) -> Step:
    return Step(operation=document["operation"], args=dict(document.get("args", {})))


def _scenario(case: str, document: dict[str, Any]) -> Scenario:
    return Scenario(
        case=case,
        id=document["id"],
        summary=document["summary"],
        trigger=document["trigger"],
        event_id=document.get("event_id"),
        event_image=document["event_image"],
        printer_state=document["printer_state"],
        detector_warned=document["detector_warned"],
        detector_paused_the_print=document["detector_paused_the_print"],
        looks=tuple(
            Look(image=look["image"], arrived_event_ids=tuple(look.get("arrived_event_ids", [])))
            for look in document["looks"]
        ),
        accept_any_of=tuple(
            tuple(_step(step) for step in outcome) for outcome in document["accept_any_of"]
        ),
        never=tuple(_step(step) for step in document["never"]),
    )


def case_directories() -> list[Path]:
    """Every case directory on disk, in name order."""
    return sorted(path.parent for path in CASES.glob("*/case.json"))


def read_case(directory: Path) -> dict[str, Any]:
    """One case's `case.json`."""
    return json.loads((directory / "case.json").read_text(encoding="utf-8"))


def scenarios() -> list[Scenario]:
    """Every scenario of every case on disk, so a case added later is covered unedited."""
    found = []
    for directory in case_directories():
        document = read_case(directory)
        found.extend(
            _scenario(directory.name, scenario) for scenario in document["assertions"]["scenarios"]
        )
    return found


def history(case: str) -> list[dict[str, Any]]:
    """The events printobserver recorded for a case's print, newest first; none without one."""
    path = CASES / case / "printobserver-history.json"
    if not path.is_file():
        return []
    return json.loads(path.read_text(encoding="utf-8"))["events"]


def agent_turn(case: str) -> dict[str, Any] | None:
    """The turn a case recorded running to its end, when it has one."""
    path = CASES / case / "agent-turn.json"
    if not path.is_file():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def operations() -> list[str]:
    """Every operation the server serves, spelled as the command line spells it."""
    document = json.loads(OPERATIONS.read_text(encoding="utf-8"))
    return [operation["name"].replace("_", "-") for operation in document["operations"]]


def usage() -> str:
    """The usage the program prints, composed from the operations it serves."""
    document = json.loads(OPERATIONS.read_text(encoding="utf-8"))
    lines = []
    for operation in document["operations"]:
        words = [PROGRAM, operation["name"].replace("_", "-")]
        for parameter in operation["parameters"]:
            value = "<json>" if parameter["name"] in {"actor", "manifest"} else "<value>"
            option = f"--{parameter['name'].replace('_', '-')} {value}"
            words.append(option if parameter["required"] else f"[{option}]")
        lines.append("  " + " ".join(words))
    return (
        f"{PROGRAM} — a supervision layer between a 3D printer and an agent.\n\n"
        f"Usage:\n  {PROGRAM} server\n  {PROGRAM} sign-in\n" + "\n".join(lines) + "\n\n"
        "Every command also takes --json (machine-readable output), --config <path>,\n"
        "--help and --version. Where the server is and what authenticates to it\n"
        "are read from that file and from the environment, never from a command line.\n"
    )


def version() -> str:
    """The version the workspace's crates, the program among them, are at."""
    manifest = tomllib.loads((REPO / "Cargo.toml").read_text(encoding="utf-8"))
    return manifest["workspace"]["package"]["version"]


def reads() -> set[str]:
    """The operations that only read, which the server declares by their effect."""
    document = json.loads(OPERATIONS.read_text(encoding="utf-8"))
    return {
        operation["name"].replace("_", "-")
        for operation in document["operations"]
        if operation["effect"] == "read"
    }


def service_config() -> dict[str, Any]:
    """The supervisor's configuration these prints ran under."""
    return tomllib.loads(SERVICE_CONFIG.read_text(encoding="utf-8"))


def shipped_model() -> str | None:
    """The model the shipped configuration pins for supervision turns, or none.

    The installer writes the configuration a host starts from, and its
    `[supervisor]` table is where a pinned model would be. When it pins none, a
    production turn runs on the harness's own default, and so does this tier.
    """
    text = SERVICE_INSTALLER.read_text(encoding="utf-8")
    table = re.search(r"^\[supervisor\]\n(?P<body>.*?)(?=^\[)", text, re.MULTILINE | re.DOTALL)
    if table is None:
        msg = f"{SERVICE_INSTALLER} writes no [supervisor] table"
        raise ValueError(msg)
    model = re.search(r'^model\s*=\s*"(?P<name>[^"]*)"', table["body"], re.MULTILINE)
    return model["name"] if model else None


# ---------------------------------------------------------------------------
# What the agent ran
# ---------------------------------------------------------------------------

_SEPARATORS = {";", "&&", "||", "|", "&", "(", ")"}
# The words after which a shell reads the next word as a command.
_COMMAND_POSITION = {*_SEPARATORS, "do", "then", "else"}


def _words(command: str) -> list[str]:
    lexer = shlex.shlex(command, posix=True, punctuation_chars=";&|()")
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


def commands_in(shell_command: str) -> list[Command]:
    """Every `printobserver <operation>` invocation a shell command makes, in order.

    The command names its operation first and every option after it as
    `--name value` or a lone flag, which is the only grammar the program parses.
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
        options: dict[str, str] = {}
        rest = words[index + 2 :]
        position = 0
        while position < len(rest):
            current = rest[position]
            if current in _SEPARATORS or _is_program(current):
                break
            if current.startswith("--"):
                name = current[2:].replace("-", "_")
                following = rest[position + 1] if position + 1 < len(rest) else None
                if following is None or following.startswith("--") or following in _SEPARATORS:
                    options[name] = ""
                    position += 1
                    continue
                options[name] = following
                position += 2
                continue
            position += 1
        found.append(Command(operation=operation, options=options))
    return found


def commands_ran(shell_commands: list[str]) -> list[Command]:
    """Every `printobserver` invocation across the shell commands the agent ran, in order."""
    return [command for shell in shell_commands for command in commands_in(shell)]


def _same_value(expected: str | int | float, given: str) -> bool:
    if isinstance(expected, str):
        return given == expected
    try:
        return Decimal(given) == Decimal(str(expected))
    except InvalidOperation:
        return False


def takes_effect(command: Command) -> bool:
    """Whether the program would carry a command out rather than print its usage or refuse it.

    `--help` prints the usage whatever else is given, and every operation that
    changes something requires a reason, without which the program refuses it.
    """
    if "help" in command.options:
        return False
    return command.operation in reads() or bool(command.options.get("reason"))


def step_matches(step: Step, command: Command) -> bool:
    """Whether a command takes the step: its operation, naming each of its args with that value."""
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


# ---------------------------------------------------------------------------
# The hook-side patterns
# ---------------------------------------------------------------------------
#
# A spy's or a stub's pattern is matched inside the harness, as a Rust regex,
# over the compact JSON of the tool call: `{"tool_name":"Bash","tool_input":
# {"command":"...",...}}`. So every pattern below is anchored inside the
# `command` string, crosses no unescaped quote, and uses only what Rust's regex
# and Python's `re` both read the same way (no lookaround).

# The start of a command the shell runs, inside the JSON string: its first
# word, or the word after a separator, `do`, `then` or `else`, with any variable
# assignments before it. A word inside an argument (`grep "printobserver look"`,
# a path ending in the program's name) is not at one.
_COMMAND_START = (
    r'"command":"(?:(?:[^"\\]|\\.)*?(?:[;&|(`]|\\n|\b(?:do|then|else)\s))?\s*'
    r"""(?:\w+=(?:[^\s"\\]|\\.)*\s+)*"""
)
# The program as a shell names it, by path or not, quoted or not.
_PROGRAM_WORD = r"""(?:\\?["'])?(?:[\w.~:-]*(?:/|\\\\))*printobserver(?:\.exe)?(?:\\?["'])?"""
# Anything else inside the same command string.
_REST = r"""(?:[^"\\]|\\.)*?"""
# Where a word ends: not a further letter, digit, hyphen or point.
_WORD_END = r"(?:[^\w.-]|$)"


def program_pattern(suffix: str = r"""(?:\s|\\?"|$)""") -> str:
    """The pattern of a command invoking the program at all, followed by `suffix`."""
    return _COMMAND_START + _PROGRAM_WORD + suffix


def version_pattern() -> str:
    """The pattern of a command asking the program which version it is."""
    return program_pattern(_REST + r"--version" + _WORD_END)


def help_pattern() -> str:
    """The pattern of a command asking the program for its usage."""
    return program_pattern(_REST + r"--help" + _WORD_END)


def operation_pattern(operation: str) -> str:
    """The pattern of a command invoking one operation."""
    return program_pattern(r"\s+" + re.escape(operation) + _WORD_END)


def _number(value: int | float) -> str:
    text = format(Decimal(str(value)).normalize(), "f")
    whole, _, fraction = text.partition(".")
    whole = whole.lstrip("0")
    if fraction:
        return f"0*{whole}\\.{fraction}0*"
    return f"0*{whole or '0'}(?:\\.0*)?"


def step_pattern(step: Step) -> str:
    """The pattern of a command taking a step: its operation, and each arg it pins.

    The args may come in any order, and numbers match however the agent spells
    the same value (`100`, `100.0`). An operation that changes something is
    taken only with a reason, as [`takes_effect`] reads it.
    """
    pinned = []
    for name, value in step.args.items():
        spelled = re.escape(value) if isinstance(value, str) else _number(value)
        option = re.escape("--" + name.replace("_", "-"))
        pinned.append(_REST + option + r"""\s+['"\\]*""" + spelled + _WORD_END)
    if step.operation not in reads():
        pinned.append(_REST + r"--reason\s")
    orders = ["".join(order) for order in permutations(pinned)] if pinned else [""]
    alternatives = orders[0] if len(orders) == 1 else "(?:" + "|".join(orders) + ")"
    return operation_pattern(step.operation) + alternatives
