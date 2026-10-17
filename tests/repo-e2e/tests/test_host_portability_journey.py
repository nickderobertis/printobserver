"""The tree and its recipes behave the same whichever host they are taken on.

Each journey here takes the committed tree or its command surface the way a
host other than this one does — a checkout under Windows' line-ending default, a
recipe reaching this repository's own tools through the search path this host's
interpreter reads — and asserts what that host's user would observe.
"""

from __future__ import annotations

import os
from pathlib import Path

from journey import REPO_ROOT, clean_environment, copy_tracked, pythonpath, run
from repo_checks.expect import equal, passing, truth
from repo_checks.shell import run as shell_run


def _git(arguments: list[str], cwd: Path) -> None:
    """Run one git command that has to succeed for the journey to mean anything."""
    passing(
        shell_run(["git", *arguments], cwd=cwd, timeout=600, env=clean_environment()),
        describing=f"`git {' '.join(arguments)}`",
    )


def test_a_checkout_under_windows_line_ending_default_carries_no_carriage_return(
    tmp_path: Path,
) -> None:
    """A clone taken with `core.autocrlf=true` holds every text file as committed.

    That setting is Git for Windows' own default, and under it every text file
    is written with CRLF unless the tree says otherwise — which is what the
    Windows runners checked out, and what every formatter, `shellcheck` and the
    schema drift test then refused. The committed tree is committed afresh here
    so that the journey reads this change's attributes rather than the ones the
    checkout's last commit carried.
    """
    source = copy_tracked(tmp_path / "source")
    _git(["init", "-q", "-b", "main"], source)
    # Quiescent, as `GateCopy` makes its copies and for the same reason: the
    # clone below copies `.git/objects` as a directory, and the commit must not
    # have started a detached repack that is emptying it meanwhile.
    _git(["config", "maintenance.auto", "false"], source)
    _git(["config", "gc.auto", "0"], source)
    _git(["add", "-A"], source)
    _git(
        ["-c", "user.name=journey", "-c", "user.email=journey@invalid", "commit", "-q", "-m", "t"],
        source,
    )
    clone = tmp_path / "clone"
    _git(["-c", "core.autocrlf=true", "clone", "-q", str(source), str(clone)], tmp_path)
    _git(["config", "core.autocrlf", "true"], clone)

    listing = shell_run(
        ["git", "ls-files", "--eol"], cwd=clone, timeout=600, env=clean_environment()
    )
    passing(listing, describing="`git ls-files --eol` in the clone")
    # One line per file: the index's own ending, the working tree's, the
    # attributes, and the path. What git reads as binary is `-text` in both and
    # is left as the bytes it was committed as.
    carrying = [
        line.partition("\t")[2]
        for line in (listing.stdout or "").splitlines()
        if line.split()[1:2] == ["w/crlf"]
    ]

    truth(
        len((listing.stdout or "").splitlines()) > 1,
        describing="the clone to carry the committed tree",
    )
    equal(carrying, [], describing="the text files a Windows-default checkout wrote with CRLF")


def test_a_recipe_reaches_the_repositorys_own_tools_through_this_hosts_search_path() -> None:
    """`just` joins the tool packages with the separator this host's interpreter reads.

    Every recipe reaching `repo_checks` depends on it, and on Windows every one
    of them failed with `No module named repo_checks` when the list was joined
    with `:`. The recipe below is run with no search path of the caller's, so
    what it reaches it reaches through the justfile's own export.
    """
    environment = clean_environment()
    environment.pop("PYTHONPATH", None)

    exported = run(["just", "--evaluate", "PYTHONPATH"], REPO_ROOT)
    answered = shell_run(
        ["just", "tool-version", "release-plz"],
        cwd=REPO_ROOT,
        timeout=600,
        env=environment,
    )

    passing(exported, describing="`just --evaluate PYTHONPATH`")
    equal(exported.stdout, pythonpath(), describing="the search path the justfile exports")
    truth(
        os.pathsep in exported.stdout,
        describing=f"the exported search path to be joined with this host's `{os.pathsep}`",
    )
    passing(answered, describing="`just tool-version release-plz`, a recipe reaching repo_checks")
    truth(
        (answered.stdout or "").startswith("version="),
        describing=f"what `just tool-version` printed: {answered.stdout!r}",
    )
