"""How one shell line an agent wrote splits into the commands it runs.

Its own module, importing nothing of the suite's, because two sides read lines by it:
the suite, which reports a `printobserver` command chained to another, and the wiring
tests' stand-in for the harness, which refuses a chained line the way the harness's
permission rules do. One reading keeps the two from disagreeing about a line.

The reading is Python's own POSIX shell lexer, `shlex`, told which characters are the
shell's operators: quotes, escapes and comments are its to read, and nothing here
restates them. What it does not look into, this does not either: a substitution inside
double quotes or between backquotes is one word to it.
"""

from __future__ import annotations

import re
import shlex
from pathlib import Path

#: The characters the shell's operators are written in, a line break among them.
PUNCTUATION = "();<>|&\n"
#: The characters a redirection is written in, which runs no other command.
REDIRECTION = "<>&"
#: A word assigning a variable for the command after it.
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")


def _tokens(line: str) -> list[str] | None:
    """The line's words and operators as the lexer reads them, or none for a line it cannot."""
    lexer = shlex.shlex(line, posix=True, punctuation_chars=PUNCTUATION)
    lexer.whitespace = " \t\r"
    lexer.whitespace_split = True
    try:
        return list(lexer)
    except ValueError:
        return None


def _separates(token: str) -> bool:
    """Whether a token is an operator ending one command, rather than a word or a redirection."""
    written = set(token)
    return written <= set(PUNCTUATION) and not (written <= set(REDIRECTION) and written & set("<>"))


def pieces(line: str) -> list[list[str]]:
    """The words of each command a line runs, split at the operators between commands.

    A line the lexer cannot read, such as one leaving a quote open, is one piece, which
    the shell would refuse to run at all.
    """
    tokens = _tokens(line)
    if tokens is None:
        return [line.split()]
    found: list[list[str]] = [[]]
    for token in tokens:
        if _separates(token):
            found.append([])
        else:
            found[-1].append(token)
    return [words for words in found if words]


def runs_the_program(words: list[str]) -> bool:
    """Whether a command's words run `printobserver`: its first word after any assignments.

    A program run through another, such as `env` or `sudo`, is that other's command.
    """
    command = next((word for word in words if not ASSIGNMENT.match(word)), "")
    return Path(command).name.removesuffix(".exe") == "printobserver"


def is_compound(line: str) -> bool:
    """Whether a line runs a `printobserver` command beside another command.

    Chained to another in one line, run after `cd`, or run in a group or a substitution
    the lexer reads as its own command. The harness refuses such a line whenever one of
    its commands is not one a permission rule allows, which costs the action it carried,
    so the skill asks for one command per call.
    """
    commands = pieces(line)
    return len(commands) > 1 and any(runs_the_program(words) for words in commands)
