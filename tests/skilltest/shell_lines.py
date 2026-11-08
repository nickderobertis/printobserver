"""How one shell line an agent wrote splits into the commands it runs.

Its own module, importing nothing of the suite's, because two sides read lines by it:
the suite, which reports a `printobserver` command run beside another, and the wiring
tests' stand-in for the harness, which refuses a line running a command no rule
allows. One reading keeps the two from disagreeing about a line.

Splitting a command line into its commands is Python's own POSIX shell lexer, `shlex`,
told which characters are the shell's operators: quotes, escapes and comments are its
to read. Two things the lexer reads as plain words run commands all the same, and are
read here: a command substitution, `$(...)` or between backquotes, which runs outside
single quotes whether or not double quotes surround it; and a program another one runs,
through a wrapper such as `env`, `sudo` or `timeout`, or as a shell's `-c` line.
"""

from __future__ import annotations

import itertools
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


#: The shells that run the line after their `-c` option.
SHELLS = {"sh", "bash", "dash", "zsh", "ksh"}
#: The programs that run another program named among their arguments.
WRAPPERS = {
    "env",
    "sudo",
    "doas",
    "nohup",
    "time",
    "exec",
    "command",
    "nice",
    "timeout",
    "stdbuf",
    "setsid",
    "xargs",
    "watch",
}
#: The word a substitution leaves in the command around it once it is read out.
SUBSTITUTED = "__substitution__"


def _closing(line: str, start: int, close: str) -> int:
    """Where the substitution opened just before `start` closes, or the line's end.

    Quotes and escapes inside it are skipped, and a `$(` inside a `$(` nests.
    """
    depth, quote, index = 1, "", start
    while index < len(line):
        character = line[index]
        if character == "\\" and quote != "'":
            index += 2
            continue
        if quote:
            quote = "" if character == quote else quote
        elif character in {"'", '"'} and close == ")":
            quote = character
        elif close == ")" and line.startswith("$(", index):
            depth += 1
            index += 1
        elif character == close:
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return len(line)


def substitutions(line: str) -> tuple[str, list[str]]:
    """The line with each command substitution its shell runs read out, and their lines.

    A substitution runs outside single quotes, inside double quotes or not; an escaped
    `$` or backquote is a literal one. Each is left in the line as one plain word.
    """
    kept, read, quote, index = [], [], "", 0
    while index < len(line):
        character = line[index]
        if character == "\\" and quote != "'":
            kept.append(line[index : index + 2])
            index += 2
            continue
        if quote == "'":
            quote = "" if character == "'" else quote
            kept.append(character)
            index += 1
            continue
        if not quote and character == "'":
            quote = "'"
            kept.append(character)
            index += 1
            continue
        if character == '"':
            quote = "" if quote == '"' else '"'
        elif line.startswith("$(", index) or character == "`":
            opened = 2 if character == "$" else 1
            end = _closing(line, index + opened, ")" if opened == 2 else "`")
            read.append(line[index + opened : end].replace("\\`", "`"))
            kept.append(SUBSTITUTED)
            index = end + 1
            continue
        kept.append(character)
        index += 1
    return "".join(kept), read


def _named(words: list[str]) -> list[str]:
    """A command's words from its program on, the variable assignments before it left out."""
    return [*itertools.dropwhile(lambda word: ASSIGNMENT.match(word) is not None, words)]


def _program(word: str) -> str:
    return Path(word).name.removesuffix(".exe")


#: A shell option cluster carrying `-c`, after which the shell runs the next word as a line.
LINE_OPTION = re.compile(r"^-[A-Za-z]*c[A-Za-z]*$")
#: A word a wrapper reads as its own argument rather than as the program it runs.
WRAPPER_ARGUMENT = re.compile(r"^(-.*|[A-Za-z_][A-Za-z0-9_]*=.*|\d+(\.\d+)?[smhd]?)$")


def _unwrapped(words: list[str]) -> list[list[str]]:
    """The commands one command's words run: itself, and what it runs as a wrapper or a shell.

    A shell with a `-c` option, alone or in a cluster such as `-lc`, runs the word after
    it as a line. A wrapper runs `printobserver` when that is among its words, and
    otherwise the first word that is none of its options, assignments or durations.
    """
    named = _named(words)
    if not named:
        return []
    program = _program(named[0])
    if program in SHELLS:
        option = next((at for at, word in enumerate(named) if LINE_OPTION.match(word)), None)
        if option is not None and option + 1 < len(named):
            return [named, *commands(named[option + 1])]
    if program in WRAPPERS:
        rest = named[1:]
        wrapped = next(
            (at for at, word in enumerate(rest) if _program(word) == "printobserver"),
            next((at for at, word in enumerate(rest) if not WRAPPER_ARGUMENT.match(word)), None),
        )
        if wrapped is not None:
            return [named[: wrapped + 1], *_unwrapped(rest[wrapped:])]
    return [named]


def commands(line: str) -> list[list[str]]:
    """The words of each command a line runs, as far as the reading above goes.

    Its own commands, those its substitutions run, and those its wrappers and shells
    run. A wrapper's own option that takes an argument may be read as the program it
    runs, unless that program is `printobserver`, which is always found.
    """
    outer, read = substitutions(line)
    found = [command for words in pieces(outer) for command in _unwrapped(words)]
    for inner in read:
        found.extend(commands(inner))
    return found


def runs_the_program(words: list[str]) -> bool:
    """Whether a command's words run `printobserver`: its first word after any assignments."""
    named = _named(words)
    return bool(named) and _program(named[0]) == "printobserver"


def is_compound(line: str) -> bool:
    """Whether a line runs a `printobserver` command beside or inside another command.

    Chained to another in one line, run after `cd`, run in a group or a substitution,
    or run through a wrapper or a shell's `-c`. The harness's permission rules allow a
    command by how it begins, so the skill asks for one command per call.
    """
    run = commands(line)
    return len(run) > 1 and any(runs_the_program(words) for words in run)
