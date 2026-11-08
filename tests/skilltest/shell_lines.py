"""How one shell line an agent wrote splits into the commands it runs.

Its own module, importing nothing of the suite's, because two sides read lines by it: the suite,
which reports a `printobserver` command chained to another, and the wiring tests'
stand-in for the harness, which refuses a chained line the way the harness's
permission rules do. One splitter keeps the two from disagreeing about a line.
"""

from __future__ import annotations

import shlex
from pathlib import Path

#: What ends one command and begins another, outside quotes and escapes: the chaining
#: operators, a line break, and the bounds of a command substitution.
OPERATORS = ("&&", "||", "$(", ";", "|", "&", "\n", "`", ")")


def segments(command: str) -> list[str]:
    """The commands one shell line runs, split where a command ends outside quotes and escapes.

    A command substitution is a command of its own, so its bounds split the line too.
    """
    found, current, quote, index = [], "", "", 0
    while index < len(command):
        character = command[index]
        if character == "\\" and quote != "'":
            current += command[index : index + 2]
            index += 2
            continue
        if quote:
            current += character
            if character == quote:
                quote = ""
        elif character in {"'", '"'}:
            quote = character
            current += character
        else:
            joined = next((op for op in OPERATORS if command.startswith(op, index)), None)
            if joined is not None:
                found.append(current)
                current = ""
                index += len(joined)
                continue
            current += character
        index += 1
    found.append(current)
    return [part.strip() for part in found if part.strip()]


def _program_of(part: str) -> str:
    """The program one command runs, its quotes taken off as the shell takes them off."""
    try:
        words = shlex.split(part)
    except ValueError:
        words = part.split()
    return Path(words[0]).name.removesuffix(".exe") if words else ""


def is_compound(command: str) -> bool:
    """Whether a shell line runs a `printobserver` command beside another command.

    Chained to another in one line, or run after `cd`. The harness refuses such a
    line whenever one of its commands is not one a permission rule allows, which
    costs the action it carried, so the skill asks for one command per call.
    """
    parts = segments(command)
    return len(parts) > 1 and any(_program_of(part) == "printobserver" for part in parts)
