"""The publication path's first commit: the merge git writes for itself.

Publishing a branch of this repository merges the base into it before anything
else happens, and when `main` has moved underneath the branch that merge makes a
commit. Git writes that commit's subject itself — `Merge remote-tracking branch
'origin/main' into <branch>` — and the committed `commit-msg` hook used to
refuse it for not being a Conventional Commit, so the merge never completed and
the publication failed. Nothing anybody typed was wrong; the hook was ruling on
a subject nobody typed.

The hook tells the two apart by the *state* of the commit rather than by its
wording — `MERGE_HEAD` is present exactly while git is completing a merge — and
that narrowing is what the refusal half below is about. `Merge remote-tracking
branch 'origin/main' into work` typed by a person is textually identical to what
git writes, so a rule reading the subject would hand anybody a bypass of the
whole convention by typing one word.

This journey drives that path rather than describing it: a real repository, the
real committed hook installed the way `just bootstrap` installs it, a real
`git merge` of a real remote-tracking ref. The refusal half is driven the same
way, in the same repository and with the very subject git generates, so an
exemption that moved back onto the wording fails the suite here.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import subprocess
from collections.abc import Callable

import pytest
from journey import GateCopy, capture, clean_environment, output
from repo_checks.expect import contains, equal, failing, passing

MERGE_SUBJECT = "Merge remote-tracking branch 'origin/main' into work"
NOT_CONVENTIONAL = "not a Conventional Commit"


def git(copy: GateCopy, *args: str) -> subprocess.CompletedProcess[str]:
    """Run real git in the copy, in the environment a publication runs it in.

    Git runs the committed hook itself, so the hook — and the `uv` invocation
    inside it — inherits this environment. `UV_PROJECT_ENVIRONMENT` is what
    keeps every copy resolving into the one environment the tier shares.
    """
    return capture(
        ["git", *args],
        copy.root,
        timeout=300,
        env=clean_environment(UV_PROJECT_ENVIRONMENT=str(copy.shared_venv)),
    )


def subject(copy: GateCopy) -> str:
    """The subject of the copy's current commit."""
    return git(copy, "log", "-1", "--format=%s").stdout.strip()


def publishable_branch(copy: GateCopy) -> None:
    """A branch whose base has moved under it, as every publication here finds.

    `refs/remotes/origin/main` is written directly rather than fetched from a
    second repository, because what the hook rules on is the subject git
    generates for the merge, and git generates the remote-tracking form from the
    ref's name whether a network was involved or not.
    """
    for args in (
        ["config", "user.email", "e2e@printobserver.test"],
        ["config", "user.name", "e2e"],
        ["config", "core.hooksPath", ".githooks"],
        ["checkout", "-q", "-b", "work"],
    ):
        passing(git(copy, *args), describing=f"git {args[0]}")

    (copy.root / "on-the-branch").write_text("the finished work\n", encoding="utf-8")
    git(copy, "add", "-A")
    passing(
        git(copy, "commit", "-q", "-m", "feat(server): the finished work"),
        describing="a conforming commit on the branch, which proves the hook is installed",
    )

    passing(git(copy, "checkout", "-q", "main"), describing="git checkout main")
    (copy.root / "on-the-base").write_text("what landed meanwhile\n", encoding="utf-8")
    git(copy, "add", "-A")
    passing(
        git(copy, "commit", "-q", "-m", "fix(core): what landed meanwhile"),
        describing="a commit on the base",
    )
    passing(git(copy, "update-ref", "refs/remotes/origin/main", "HEAD"), describing="update-ref")
    passing(git(copy, "checkout", "-q", "work"), describing="git checkout work")


# journey: publishing-a-branch-once-main-has-moved
def test_a_publication_merges_the_base_into_the_branch(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The merge every publication makes completes, and git's own subject stands."""
    copy = gate_copy()
    publishable_branch(copy)

    merge = git(copy, "merge", "--no-edit", "origin/main")

    passing(merge, describing="git merge origin/main, the first step of every publication")
    equal(subject(copy), MERGE_SUBJECT, describing="the subject git wrote for the merge")
    equal(
        len(git(copy, "log", "-1", "--format=%P").stdout.split()),
        2,
        describing="the merge to have made a commit of two parents rather than fast-forwarded",
    )


@pytest.mark.parametrize(
    "authored",
    [
        "made some changes",
        "Merge the two configuration files by hand",
        # The subject git generates, typed by a person into an ordinary commit.
        # Nothing distinguishes it from the accepted one above but the state git
        # is in, so this is the case that fails if the narrowing is ever lost.
        MERGE_SUBJECT,
    ],
)
def test_an_authored_subject_is_still_held_to_conventional_commits(
    authored: str, gate_copy: Callable[..., GateCopy]
) -> None:
    """A subject a person types is theirs to get right, `Merge` at the front or not."""
    copy = gate_copy()
    publishable_branch(copy)
    before = subject(copy)

    (copy.root / "by-hand").write_text("typed by a person\n", encoding="utf-8")
    git(copy, "add", "-A")
    refused = git(copy, "commit", "-m", authored)

    failing(refused, naming=NOT_CONVENTIONAL)
    contains(output(refused), authored)
    equal(subject(copy), before, describing="the refused commit not to have been made")
