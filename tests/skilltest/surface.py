"""The `printobserver` command line as the skill tests read it, from what the program declares.

Nothing here restates the program. Its commands, each command's option forms
and which fields it requires, and its exits come from
`skills/printobserver/reference/surface.json`, which `just docs-generate`
writes from the program's own `surface()`; which values are structured comes
from the server's `operations.json`; the labelled rendering's separators from
`render.rs`; and the tools and shell rules a production turn runs with from
`printobserver-oneharness`'s `turn.rs`.

The hook-side patterns are here too. A stub's or a spy's pattern is matched
inside the harness as a Rust regex over the compact JSON of a tool call,
`{"tool_name":"Bash","tool_input":{"command":"...",...}}`, so each is anchored
at a command the shell runs inside the `command` string, crosses no unescaped
quote, and uses only what Rust's regex and Python's `re` read the same way.
"""

from __future__ import annotations

import json
import re
import tomllib
from dataclasses import dataclass
from decimal import Decimal
from functools import cache
from itertools import permutations
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SURFACE = REPO / "skills" / "printobserver" / "reference" / "surface.json"
OPERATIONS = REPO / "schemas" / "printobserver-server" / "operations.json"
RENDER = REPO / "crates" / "printobserver" / "src" / "render.rs"
TURN = REPO / "crates" / "printobserver-oneharness" / "src" / "turn.rs"
SURFACE_RS = REPO / "crates" / "printobserver" / "src" / "surface.rs"

PROGRAM = "printobserver"


@dataclass(frozen=True)
class Field:
    """One value a command takes, and every option that can supply it."""

    name: str
    required: bool
    kind: str
    located: str
    forms: tuple[str, ...]
    file_forms: tuple[str, ...] = ()

    @property
    def structured(self) -> bool:
        """Whether the value is a document rather than text or a number."""
        return self.kind == "structured"


@dataclass(frozen=True)
class CommandSpec:
    """One command the program has."""

    name: str
    operation: str | None
    mutating: bool
    fields: tuple[Field, ...]

    def usage_line(self) -> str:
        """The command's line of the usage, as `surface.rs` writes it."""
        taken = []
        for field in self.fields:
            value = "<json>" if field.structured else "<value>"
            option = f"{field.forms[0]} {value}"
            taken.append(option if field.required else f"[{option}]")
        return f"  {PROGRAM} {self.name} {' '.join(taken)}".rstrip()


@dataclass(frozen=True)
class Surface:
    """Every command, the options every command takes, and the program's exits."""

    commands: tuple[CommandSpec, ...]
    global_options: tuple[str, ...]
    exits: dict[str, int]

    @property
    def operations(self) -> list[CommandSpec]:
        """The commands that are requests to a running server."""
        return [command for command in self.commands if command.operation is not None]

    def command(self, name: str) -> CommandSpec | None:
        """The command of one name, when the program has one."""
        return next((command for command in self.commands if command.name == name), None)


def _parameters(document: dict) -> dict[tuple[str, str], dict]:
    return {
        (operation["name"], parameter["name"]): parameter
        for operation in document["operations"]
        for parameter in operation["parameters"]
    }


@cache
def surface() -> Surface:
    """The program's surface, read from the manifest it generates."""
    document = json.loads(SURFACE.read_text(encoding="utf-8"))
    parameters = _parameters(json.loads(OPERATIONS.read_text(encoding="utf-8")))
    commands = []
    for command in document["commands"]:
        fields: dict[str, Field] = {}
        for option in command["options"]:
            held = fields.get(option["field"])
            forms = (*held.forms, option["option"]) if held else (option["option"],)
            parameter = parameters.get((command["operation"], option["field"]), {})
            files = held.file_forms if held else ()
            if option["supply"] == "file":
                files = (*files, option["option"])
            fields[option["field"]] = Field(
                name=option["field"],
                required=(held.required if held else False) or option["required"],
                kind=parameter.get("kind", "text"),
                located=option["located"],
                forms=forms,
                file_forms=files,
            )
        commands.append(
            CommandSpec(
                name=command["command"],
                operation=command["operation"],
                mutating=bool(command.get("mutating")),
                fields=tuple(fields.values()),
            )
        )
    return Surface(
        commands=tuple(commands),
        global_options=tuple(document["global_options"]),
        exits={entry["name"]: entry["status"] for entry in document["exits"]},
    )


def usage() -> str:
    """What the program prints for `--help`: its whole surface, as `surface.rs` writes it.

    `skilltest-wiring` holds this to what the built program prints.
    """
    lines = "\n".join(command.usage_line() for command in surface().commands)
    return (
        f"{PROGRAM} — a supervision layer between a 3D printer and an agent.\n\n"
        f"Usage:\n{lines}\n\nEvery command also takes --json (machine-readable output), "
        "--config <path>,\n--help and --version. Where the server is and what authenticates "
        "to it\nare read from that file and from the environment, never from a command line.\n"
    )


def version() -> str:
    """What the program prints for `--version`: the workspace's version."""
    manifest = tomllib.loads((REPO / "Cargo.toml").read_text(encoding="utf-8"))
    return f"{PROGRAM} {manifest['workspace']['package']['version']}\n"


def _rust_constant(source: Path, name: str) -> str:
    found = re.search(
        rf"pub const {name}: [^=]+= (?P<value>'[^']*'|\"[^\"]*\");", source.read_text()
    )
    if found is None:
        msg = f"{source} declares no {name}"
        raise ValueError(msg)
    return found["value"][1:-1]


def duration_bounds() -> tuple[int, int]:
    """The shortest and longest a bounded change may stand for, as `surface.rs` declares them."""
    source = SURFACE_RS.read_text(encoding="utf-8")
    found = [
        re.search(rf"pub const {name}: i64 = (?P<value>[\d_]+);", source)
        for name in ("MIN_DURATION_SECONDS", "MAX_DURATION_SECONDS")
    ]
    if None in found:
        msg = f"{SURFACE_RS} declares no duration bounds"
        raise ValueError(msg)
    low, high = (int(match["value"].replace("_", "")) for match in found if match)
    return low, high


def event_kind(payload: str) -> str:
    """The kind an event carrying one payload type is written down under, as its crate declares.

    Raises:
        ValueError: If no crate declares that payload as an event's.
    """
    declaration = re.compile(
        rf"impl EventPayload for {payload} \{{\s*const KIND: &'static str = \"(?P<kind>\w+)\";"
    )
    for source in sorted((REPO / "crates").glob("*/src/**/*.rs")):
        found = declaration.search(source.read_text(encoding="utf-8"))
        if found is not None:
            return found["kind"]
    msg = f"no crate declares {payload} as an event's payload"
    raise ValueError(msg)


def separators() -> tuple[str, str]:
    """The labelled rendering's path separator and label separator, as `render.rs` declares them."""
    return _rust_constant(RENDER, "PATH_SEPARATOR"), _rust_constant(RENDER, "LABEL_SEPARATOR")


def _rust_list(source: Path, name: str) -> list[str]:
    found = re.search(rf"const {name}: \[&str; \d+\] = \[(?P<items>[^\]]*)\];", source.read_text())
    if found is None:
        msg = f"{source} declares no {name}"
        raise ValueError(msg)
    return re.findall(r'"([^"]+)"', found["items"])


def turn_tool_rules() -> tuple[str, str, list[str], list[str]]:
    """How a production Claude Code turn is narrowed: its two flags, its tools and its rules.

    `turn.rs`'s `permissions` passes its tools after one flag and its allowed
    rules after the other: the read tools outright, and the shell only for the
    program's own commands, one per operation (`printobserver-server`'s
    `agent_commands`), each through the rule template it formats.

    Raises:
        ValueError: If `turn.rs` no longer narrows a turn in that shape.
    """
    source = TURN.read_text(encoding="utf-8")
    flags = re.search(
        r'vec!\["(?P<tools>--\w+)"\.to_owned\(\)\];.*?push\("(?P<allowed>--\w+)"\.to_owned\(\)\)',
        source,
        re.DOTALL,
    )
    rule = re.search(r'format!\("(?P<rule>\w+\(\{command\}[^"]*)"\)', source)
    if flags is None or rule is None or "(PermissionMode::Default, arguments)" not in source:
        msg = f"{TURN} no longer narrows a Claude Code turn by a tool flag and a rule flag"
        raise ValueError(msg)
    # llmlint: ignore[least_privilege_grants] suppressions.toml has the reason.
    allowed = _rust_list(TURN, "CLAUDE_READ_TOOLS") + [
        rule["rule"].replace("{command}", f"{PROGRAM} {command.name}")
        for command in surface().operations
    ]
    return flags["tools"], flags["allowed"], _rust_list(TURN, "CLAUDE_TURN_TOOLS"), allowed


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
# Where the command string ends.
_END = r"""\s*(?:"|\\n|;|&|\|)"""


def program_pattern() -> str:
    """The pattern of a command invoking the program with anything after it."""
    return _COMMAND_START + _PROGRAM_WORD + r"\s+"


def bare_program_pattern() -> str:
    """The pattern of a command invoking the program with nothing after it."""
    return _COMMAND_START + _PROGRAM_WORD + _END


def asking_pattern(option: str) -> str:
    """The pattern of a command naming an option that answers on its own (`--help`)."""
    return _COMMAND_START + _PROGRAM_WORD + _REST + re.escape(option) + _WORD_END


def operation_pattern(command: str) -> str:
    """The pattern of a command invoking one of the program's commands."""
    return _COMMAND_START + _PROGRAM_WORD + r"\s+" + re.escape(command) + _WORD_END


def _number(value: float) -> str:
    text = format(Decimal(str(value)).normalize(), "f")
    whole, _, fraction = text.partition(".")
    whole = whole.lstrip("0")
    if fraction:
        return f"0*{whole}\\.{fraction}0*"
    return f"0*{whole or '0'}(?:\\.0*)?"


def _option(name: str, value: str | float | None) -> str:
    forms = "|".join(re.escape(form) for form in _forms(name))
    if value is None:
        return _REST + f"(?:{forms})" + r"\s"
    spelled = re.escape(value) if isinstance(value, str) else _number(value)
    return _REST + f"(?:{forms})" + r"""\s+['"\\]*""" + spelled + _WORD_END


def _forms(name: str) -> list[str]:
    for command in surface().commands:
        for field in command.fields:
            if field.name == name:
                return list(field.forms)
    return ["--" + name.replace("_", "-")]


def invocation_pattern(command: str, args: dict[str, str | float]) -> str:
    """The pattern of a command naming each arg with that value, and a reason where one is due.

    The args may come in any order, and numbers match however the agent spells
    the same value (`100`, `100.0`). A command that requires a reason names one,
    since the program refuses it otherwise; the rest of what a command requires
    is left to the judge in `real_prints.sent_to_server`, because a pattern
    without lookaround can only demand several options by listing every order
    they may come in.
    """
    spec = surface().command(command)
    pinned = [_option(name, value) for name, value in args.items()]
    if spec and any(field.name == "reason" and field.required for field in spec.fields):
        pinned.append(_option("reason", None))
    orders = ["".join(order) for order in permutations(pinned)] or [""]
    alternatives = orders[0] if len(orders) == 1 else "(?:" + "|".join(orders) + ")"
    return operation_pattern(command) + alternatives
