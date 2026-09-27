"""The held GitHub CLI takes every option the release pull request is armed with.

`release_artifacts.arming` arms the release pull request by running `gh pr
merge` with the options its `ARM` names, and every suite that drives that step
does it through a stand-in `gh` that accepts anything, because the real one
would arm a real pull request. So this is what notices a `gh` that no longer
takes one of them: it reads the options against the real `gh`'s own help, at
the release `repo-policy.toml` holds. It is here rather than in the gate for
the reason the rest of this project is: that `gh` is given to this project's
job alone, and a case that skipped where it was absent would pass on any host
that happened to carry none.
"""

from __future__ import annotations

import re

from release_artifacts.arming import ARM
from repo_checks.expect import equal, truth
from repo_checks.shell import run

#: A long option as a help text lists one: the whole name, so `--body` is not
#: found inside `--body-file` and a `gh` that dropped the one while keeping the
#: other is noticed.
OPTION = re.compile(r"(?<![\w-])--[a-z0-9][a-z0-9-]*")


def test_the_held_gh_takes_every_option_arming_passes(gh: str) -> None:
    """`gh pr merge --help` lists every option `ARM` hands it."""
    _, *subcommand = ARM[:3]
    helped = run([gh, *subcommand, "--help"], timeout=60)
    said = helped.stdout + helped.stderr

    equal(helped.returncode, 0, describing=f"`gh pr merge --help`: {said}")
    listed = set(OPTION.findall(said))
    for option in ARM[3:]:
        truth(
            option in listed,
            describing=f"`gh pr merge --help` to list `{option}` among {sorted(listed)}",
        )
