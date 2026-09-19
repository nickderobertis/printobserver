"""The end-user install path, read from its one authoritative source.

`AGENTS.md`'s "The end-user install path" section states three alternative
routes to the `printobserver` program, then two commands in order, and how the
agent's harness is installed and signed in as the service's own user between
those two. Every
other statement of one of them in this repository is derived from it, and
`drifted_statements` is what holds them together.

The three routes are three ways of *obtaining* the program and stay three
however many platforms this repository supports; the two commands after them are
**per service manager**, because putting a service in place and starting it is
the one part of this path that is not the same sentence on every host. So the
section states one pair per service manager the supported-platform list names,
each under a subsection headed by that manager's own name, and a consumer asks
for the pair belonging to a platform through that platform's service-manager
column rather than taking whichever pair happened to be written first.
"""

from __future__ import annotations

import re
from collections.abc import Mapping
from dataclasses import dataclass, field
from urllib.parse import urlparse

from repo_checks.parsing import fenced_commands, section

SECTION_HEADING = "The end-user install path"

# The subsection under which one pair per service manager is stated. Each pair
# sits in a subsection of its own, headed by the service manager's own name as
# the supported-platform list spells it.
SERVICE_COMMANDS_HEADING = "then, in order"

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

# The subsection stating how the agent's harness is installed and signed in as
# the service's own user, between the two commands. Its commands are part of the
# section's canonical set, so every restatement of them is held to it.
SIGN_IN_HEADING = "between the two commands"

# What the sign-in subsection has to run to sign the harness in at all.
SIGN_IN_COMMAND = "printobserver sign-in"

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

# The pinned form of a script route's command: a concrete release tag and a
# concrete install directory, in whichever shell the script is fetched by — the
# shell form's `--version vX.Y.Z --to DIR`, the PowerShell form's
# `-Version vX.Y.Z -To DIR`. A directory is concrete when it is absolute on its
# own platform: rooted at `~` or `/`, or at a drive letter.
PINNED_VERSION = re.compile(r"(?:--version|-Version)\s+v\d+\.\d+\.\d+(?:\s|$)")
PINNED_DIRECTORY = re.compile(r"(?:--to|-To)\s+(?:~|/|[A-Za-z]:\\)\S+")
# What marks a command as the pinned form at all.
PINNED_OPTION = re.compile(r"(?:^|\s)(?:--version|-Version)(?:\s|$)")
# A fetch of a file of this repository. Ends before a closing parenthesis as
# well as at whitespace, because PowerShell's pinned form wraps the fetch in
# `([scriptblock]::Create((irm ...)))`.
RAW_URL = re.compile(r"https://raw\.githubusercontent\.com/[^\s)]+")
# A command that starts or enables a service, in any manager's vocabulary the
# section states a pair in: systemd's `start`, `enable` and `--now`, and the
# service control manager's `Start-Service`, `-Status Running` and an automatic
# start type.
STARTS_SERVICE = re.compile(
    r"\bsystemctl\b.*\b(start|enable)\b|--now\b"
    r"|\bStart-Service\b|-Status\s+Running\b|-StartupType\s+Automatic"
)


@dataclass(frozen=True, slots=True)
class Route:
    """One alternative route to the program."""

    heading: str
    commands: tuple[str, ...]

    @property
    def command(self) -> str:
        """The route's own command — the one a reader pastes."""
        return self.commands[0] if self.commands else ""

    @property
    def fetches(self) -> dict[str, str]:
        """The one command that fetches each script behind this route, by script.

        A route with a script behind it states one fetch command per script —
        the shell one for the platforms a shell reaches, the PowerShell one for
        Windows — and each is a command some install job has to run. The
        pinned form of the same fetch is the same script and is not a second
        one.
        """
        found: dict[str, str] = {}
        for command in self.commands:
            match = RAW_URL.search(command)
            if match is None:
                continue
            script = match.group(0).partition("/main/")[2]
            if script and script not in found:
                found[script] = command
        return found

    @property
    def pinned(self) -> tuple[str, ...]:
        """Every command of this route that names a release to install."""
        return tuple(command for command in self.commands if PINNED_OPTION.search(command))


@dataclass(frozen=True, slots=True)
class InstallPath:
    """The whole path: three routes, a check, a pair per service manager, the sign-in."""

    intro: str
    routes: tuple[Route, ...]
    #: What a caller runs to see what the route installed.
    verification: tuple[str, ...]
    #: The two commands after the routes, keyed by the service manager whose own
    #: pair they are. A platform's pair is the one its service-manager column
    #: names, which is what `commands_for` answers.
    service_commands: Mapping[str, tuple[str, ...]] = field(default_factory=dict)
    #: How the agent's harness is installed and signed in as the service's user.
    sign_in: tuple[str, ...] = ()
    #: Every service manager the section states more than one pair for. Kept
    #: rather than collapsed: the second subsection would otherwise replace the
    #: first, leaving a pair nobody wrote as the one every check reads.
    repeated: tuple[str, ...] = ()

    @property
    def checked(self) -> str:
        """The one command that reads back what a route installed."""
        return self.verification[0] if self.verification else ""

    def commands_for(self, service_manager: str) -> tuple[str, ...]:
        """The pair belonging to one service manager, or none where it states none."""
        return self.service_commands.get(service_manager, ())

    @property
    def commands(self) -> tuple[str, ...]:
        """Every command stated after the routes, in the order the section states it."""
        return tuple(command for pair in self.service_commands.values() for command in pair)

    @property
    def canonical(self) -> tuple[str, ...]:
        """Every command the section states, in the order it states them."""
        stated: list[str] = []
        for route in self.routes:
            stated.extend(route.commands)
        stated.extend(self.verification)
        stated.extend(self.commands)
        stated.extend(self.sign_in)
        return tuple(stated)


def parse(agents_md: str) -> InstallPath:
    """Read the install path out of its authoritative section.

    A subsection is read by its own heading and by the one it sits under, so a
    pair of commands belongs to the service manager its heading names rather
    than to wherever in the section it happens to sit.
    """
    body = section(agents_md, SECTION_HEADING)
    blocks: list[tuple[int, str, str, list[str]]] = [(2, "", "", [])]
    under = ""
    for line in body.splitlines():
        if line.startswith("#### "):
            blocks.append((4, line[5:].strip(), under, []))
        elif line.startswith("### "):
            under = line[4:].strip()
            blocks.append((3, under, "", []))
        else:
            blocks[-1][3].append(line)

    routes: list[Route] = []
    verification: tuple[str, ...] = ()
    service_commands: dict[str, tuple[str, ...]] = {}
    repeated: list[str] = []
    sign_in: tuple[str, ...] = ()
    intro_lines: list[str] = []
    seen_route = False
    for level, title, parent, lines in blocks:
        text = "\n".join(lines)
        if level == 4 and title.lower().startswith("route"):
            seen_route = True
            routes.append(Route(title, tuple(fenced_commands(text))))
        elif level == 4 and parent.lower().startswith(SERVICE_COMMANDS_HEADING):
            manager = title.strip().lower()
            if manager in service_commands:
                repeated.append(manager)
            service_commands[manager] = tuple(fenced_commands(text))
        elif title.lower().startswith(VERIFICATION_HEADING):
            verification = tuple(fenced_commands(text))
        elif title.lower().startswith(SIGN_IN_HEADING):
            sign_in = tuple(fenced_commands(text))
        elif not seen_route:
            intro_lines.extend(lines)
    return InstallPath(
        "\n".join(intro_lines),
        tuple(routes),
        verification,
        service_commands,
        sign_in,
        tuple(repeated),
    )


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
