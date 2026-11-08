"""How one shell line an agent wrote splits into the commands it runs.

Its own module, importing nothing of the suite's, because two sides read lines by it: the suite,
which reports a `printobserver` command chained to another, and the wiring tests'
stand-in for the harness, which refuses a chained line the way the harness's
permission rules do. One splitter keeps the two from disagreeing about a line.
"""

from __future__ import annotations

import re
import shlex
from pathlib import Path

#: What ends one command and begins another, outside quotes and escapes: the chaining
#: operators and a line break.
OPERATORS = ("&&", "||", ";", "|", "&", "\n")
#: What opens a command substitution, which runs a command of its own even inside
#: double quotes, and what closes each.
OPENS = {"$(": ")", "`": "`"}
#: A word assigning a variable for the command after it.
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
#: The programs that run the command after them, which is then the one a piece runs.
LAUNCHERS = {"env", "sudo", "exec", "time", "nohup", "command", "xargs"}


def _redirects(command: str, index: int) -> bool:
    """Whether the `&` at an index is part of a redirection, `2>&1` or `&>`, not a chain."""
    return command.startswith("&>", index) or (index > 0 and command[index - 1] in "<>")


def segments(command: str) -> list[str]:
    """The pieces a line's commands are written in, as far as chaining them goes.

    Split wherever the shell would run another command: at a chaining operator or a
    line break outside quotes, and at the bounds of a command substitution, which
    double quotes do not stop. A quoted or escaped operator stays in its word, and a
    redirection is no chain. A piece is no parsed command: it may begin with an
    assignment or a launcher.
    """
    found, current, quote, closing, index = [], "", "", [], 0
    while index < len(command):
        character = command[index]
        if character == "\\" and quote != "'":
            current += command[index : index + 2]
            index += 2
            continue
        opened = next((mark for mark in OPENS if command.startswith(mark, index)), None)
        if quote != "'" and closing and command.startswith(closing[-1], index):
            # The substitution's own close, before a backtick reads as opening another.
            found.append(current)
            current, index = "", index + len(closing.pop())
            continue
        if quote != "'" and opened is not None:
            found.append(current)
            closing.append(OPENS[opened])
            current, index = "", index + len(opened)
            continue
        if quote:
            current += character
            quote = "" if character == quote else quote
            index += 1
            continue
        if character in {"'", '"'}:
            quote = character
            current += character
            index += 1
            continue
        joined = next((op for op in OPERATORS if command.startswith(op, index)), None)
        if joined is not None and not (joined == "&" and _redirects(command, index)):
            found.append(current)
            current, index = "", index + len(joined)
            continue
        current += character
        index += 1
    found.append(current)
    return [part.strip() for part in found if part.strip()]


def runs_the_program(piece: str) -> bool:
    """Whether a piece runs `printobserver`: as its command, after assignments or launchers."""
    try:
        words = shlex.split(piece)
    except ValueError:
        words = piece.split()
    for word in words:
        if ASSIGNMENT.match(word) or word in LAUNCHERS or word.startswith("-"):
            continue
        return Path(word).name.removesuffix(".exe") == "printobserver"
    return False


def is_compound(command: str) -> bool:
    """Whether a line chains a piece running the `printobserver` program to another piece.

    Chained to another in one line, run after `cd`, or run inside a substitution. The
    harness refuses such a line whenever one of its commands is not one a permission
    rule allows, which costs the action it carried, so the skill asks for one command
    per call.
    """
    parts = segments(command)
    return len(parts) > 1 and any(runs_the_program(part) for part in parts)
