"""The end-user install path, read from its one authoritative source.

`AGENTS.md`'s "The end-user install path" section states three alternative
routes to the `printobserver` program and then two commands in order. Every
other statement of one of them in this repository is derived from it, and
`drifted_statements` is what holds them together.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from urllib.parse import urlparse

from repo_checks.parsing import fenced_commands, section

SECTION_HEADING = "The end-user install path"

# The subsection stating what a caller runs to see what the route installed.
# Whichever route was taken, running the program is what tells an install that
# worked from one that put something unrunnable on the path — so it is stated
# here, in the one authoritative source of this path, rather than grown inside
# a job nobody declared it in.
VERIFICATION_HEADING = "then, check"

# What that command has to do to be a check of what was installed: run the
# program every route delivers, and ask it which version it is.
VERIFICATION_PROGRAM = "printobserver"
VERIFICATION_OPTION = "--version"

# The sentence the section must carry so a reader cannot mistake the routes for
# steps. Stated literally because a check cannot judge a paraphrase.
ALTERNATIVES_SENTENCE = (
    "These three routes are alternatives reaching the same program: take one of "
    "them, not all three."
)

# Wording that would present the routes as a sequence rather than as choices.
SEQUENCE_MARKERS = (
    "step 1",
    "step 2",
    "step 3",
    "then run route",
    "next, run",
    "followed by",
    "in sequence",
    "one after another",
    "all three in order",
)

# Anything a reader would have to reconstruct rather than paste.
PLACEHOLDER_MARKERS = ("...", "…", "<", ">", "TODO", "TBD", "PLACEHOLDER", "YOUR_", "${")

PINNED_VERSION = re.compile(r"--version\s+v\d+\.\d+\.\d+(?:\s|$)")
PINNED_DIRECTORY = re.compile(r"--to\s+(?:~|/)\S+")
RAW_URL = re.compile(r"https://raw\.githubusercontent\.com/\S+")
STARTS_SERVICE = re.compile(r"\bsystemctl\b.*\b(start|enable)\b|--now\b")


@dataclass(frozen=True, slots=True)
class Route:
    """One alternative route to the program."""

    heading: str
    commands: tuple[str, ...]

    @property
    def command(self) -> str:
        """The route's own command — the one a reader pastes."""
        return self.commands[0] if self.commands else ""


@dataclass(frozen=True, slots=True)
class InstallPath:
    """The whole path: three alternative routes, a check, then two commands in order."""

    intro: str
    routes: tuple[Route, ...]
    #: What a caller runs to see what the route installed.
    verification: tuple[str, ...]
    commands: tuple[str, ...]

    @property
    def checked(self) -> str:
        """The one command that reads back what a route installed."""
        return self.verification[0] if self.verification else ""

    @property
    def canonical(self) -> tuple[str, ...]:
        """Every command the section states, in the order it states them."""
        stated: list[str] = []
        for route in self.routes:
            stated.extend(route.commands)
        stated.extend(self.verification)
        stated.extend(self.commands)
        return tuple(stated)


def parse(agents_md: str) -> InstallPath:
    """Read the install path out of its authoritative section."""
    body = section(agents_md, SECTION_HEADING)
    blocks: list[tuple[int, str, list[str]]] = [(2, "", [])]
    for line in body.splitlines():
        if line.startswith("#### "):
            blocks.append((4, line[5:].strip(), []))
        elif line.startswith("### "):
            blocks.append((3, line[4:].strip(), []))
        else:
            blocks[-1][2].append(line)

    routes: list[Route] = []
    verification: tuple[str, ...] = ()
    commands: tuple[str, ...] = ()
    intro_lines: list[str] = []
    seen_route = False
    for level, title, lines in blocks:
        text = "\n".join(lines)
        if level == 4 and title.lower().startswith("route"):
            seen_route = True
            routes.append(Route(title, tuple(fenced_commands(text))))
        elif title.lower().startswith(VERIFICATION_HEADING):
            verification = tuple(fenced_commands(text))
        elif title.lower().startswith("then, in order"):
            commands = tuple(fenced_commands(text))
        elif not seen_route:
            intro_lines.extend(lines)
    return InstallPath("\n".join(intro_lines), tuple(routes), verification, commands)


def statement_key(command: str) -> tuple[str, ...]:
    """A key grouping the different spellings of one stated command together.

    A fetch is keyed by the path it fetches, so the default and pinned forms of
    one route share a key; anything else is keyed by its first two words.
    """
    match = RAW_URL.search(command)
    if match:
        return ("fetch", urlparse(match.group(0)).path)
    words = command.split()
    return tuple(words[:2])


def drifted_statements(path: InstallPath, candidates: dict[str, list[str]]) -> list[str]:
    """Report every restatement that differs from what the section states.

    `candidates` maps a place in the tree to the command lines found there. A
    candidate that keys to one of the section's commands must equal one of them
    exactly; a candidate that keys to nothing the section states is not a
    restatement and is left alone.
    """
    by_key: dict[tuple[str, ...], set[str]] = {}
    for stated in path.canonical:
        by_key.setdefault(statement_key(stated), set()).add(stated)

    findings: list[str] = []
    for place, lines in candidates.items():
        for line in lines:
            key = statement_key(line)
            if key in by_key and line not in by_key[key]:
                expected = " or ".join(sorted(by_key[key]))
                findings.append(
                    f"{place} states `{line}`, but AGENTS.md's "
                    f"`{SECTION_HEADING}` states `{expected}`"
                )
    return findings
