"""What the committed toolchain holds, read rather than restated by a suite.

`repo-policy.toml`'s `[[toolchain.tool]]` entries are the one statement of
which command each tool puts on PATH, how it is installed and at which release.
Two suites need the same three facts out of them — the install verb, the
program that verb produces, and the release held — and a copy of any of them in
a test is a copy that goes stale on a bump or a rename while the suite goes on
passing.

An entry's `install` is the argv that installs it, so where that argv goes
through `repo_checks` the word after it is the verb. An entry installed some
other way — `cargo install`, say — has no verb and is not one of these.
"""

from __future__ import annotations

from dataclasses import dataclass

from repo_checks.model import Repo, toolchain_tools
from treecopy import REPO_ROOT


@dataclass(frozen=True, slots=True)
class Held:
    """One tool the toolchain installs through a `repo_checks` verb."""

    #: The verb, as `python -m repo_checks <verb>` names it.
    verb: str
    #: The command that verb puts on PATH.
    command: str
    #: The release `repo-policy.toml` holds it at.
    version: str


def held_by_verb() -> tuple[Held, ...]:
    """Every tool the committed toolchain installs through a `repo_checks` verb."""
    found: list[Held] = []
    for tool in toolchain_tools(Repo(REPO_ROOT)):
        words = tool.install.split()
        if "repo_checks" in words and tool.version is not None:
            found.append(Held(words[words.index("repo_checks") + 1], tool.command, tool.version))
    return tuple(found)
