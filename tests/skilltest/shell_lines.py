"""How one shell line an agent wrote splits into the commands it runs.

Its own module, importing nothing, because two sides read lines by it: the suite,
which reports a `printobserver` command chained to another, and the wiring tests'
stand-in for the harness, which refuses a chained line the way the harness's
permission rules do. One splitter keeps the two from disagreeing about a line.
"""

from __future__ import annotations

from pathlib import Path

#: Shell operators that make a line run more than one command, outside its quotes.
OPERATORS = ("&&", "||", ";", "|", "&", "\n")


def segments(command: str) -> list[str]:
    """The commands one shell line runs, split at the operators outside its quotes."""
    found, current, quote, index = [], "", "", 0
    while index < len(command):
        character = command[index]
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


def is_compound(command: str) -> bool:
    """Whether a shell line runs a `printobserver` command beside another command.

    Chained to another in one line, or run after `cd`. The harness refuses such a
    line whenever one of its commands is not one a permission rule allows, which
    costs the action it carried, so the skill asks for one command per call.
    """
    parts = segments(command)
    runs_the_program = any(
        Path(part.split()[0]).name.removesuffix(".exe") == "printobserver" for part in parts
    )
    return runs_the_program and len(parts) > 1
