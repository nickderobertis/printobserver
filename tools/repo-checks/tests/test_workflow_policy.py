"""Actions are pinned, and every command a workflow step runs is one the allowlist names."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import workflow_policy
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

CI = ".github/workflows/ci.yml"


def test_the_committed_workflows_are_accepted(committed: Repo) -> None:
    """Every action is pinned and every command is one this repository runs."""
    accepted(workflow_policy(committed))


def test_an_unpinned_action_is_refused(tree: Callable[[], Tree]) -> None:
    """A branch ref re-points under the repository with no commit here."""
    broken = tree()
    broken.edit(CI, "      - uses: actions/checkout@v5\n", "      - uses: actions/checkout@main\n")

    findings = workflow_policy(broken.repo)

    refused(findings, "not pinned in the form")


def test_a_command_the_allowlist_does_not_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A step running something outside the allowlist is a grant nobody made."""
    broken = tree()
    broken.edit(
        CI,
        "      - run: just check\n",
        "      - run: just check\n      - run: rm -rf /tmp/whatever\n",
    )

    findings = workflow_policy(broken.repo)

    refused(findings, "the command allowlist does not name")


def test_the_install_paths_own_commands_are_not_refused(committed: Repo) -> None:
    """The install jobs run what the section states, which is not a recipe."""
    findings = workflow_policy(committed)

    accepted(
        [finding for finding in findings if "install-path.yml" in finding],
        describing="the install-path jobs, whose commands the section states",
    )
