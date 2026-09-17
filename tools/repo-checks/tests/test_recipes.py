"""The check recipe invokes every declared tier and no recipe runs nothing."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import os
from collections.abc import Callable

from repo_checks.checks_repo import recipe_set
from repo_checks.expect import accepted, equal, refused
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import Tree


def test_the_committed_recipe_set_is_accepted(committed: Repo) -> None:
    """The gate this repository ships invokes every tier it declares."""
    accepted(recipe_set(committed))


def evaluated_profile_name(repo: Repo, *, windows: bool) -> str:
    """Read the profile-name decision through the committed justfile."""
    environment = os.environ.copy()
    if windows:
        environment["OS"] = "Windows_NT"
    else:
        environment.pop("OS", None)
    read = run(
        [
            "just",
            "--justfile",
            str(repo.path("justfile")),
            "--evaluate",
            "windows_coverage_profile",
        ],
        cwd=repo.root,
        env=environment,
        check=True,
    )
    return read.stdout.strip()


def test_the_committed_recipe_gives_windows_child_processes_unique_profiles(
    committed: Repo,
) -> None:
    """The real recipe selects a PID-unique Windows coverage record."""
    equal(
        evaluated_profile_name(committed, windows=True),
        "LLVM_PROFILE_FILE_NAME=printobserver-%p.profraw",
    )


def test_the_committed_recipe_leaves_linux_coverage_environment_unchanged(
    committed: Repo,
) -> None:
    """The Windows repair exports nothing into the established Linux path."""
    equal(evaluated_profile_name(committed, windows=False), "")


def test_a_check_recipe_omitting_a_declared_tier_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate that skips the end-to-end tier is a gate that proves less than it says."""
    broken = tree()
    broken.edit("justfile", "    just test-e2e\n", "")

    findings = recipe_set(broken.repo)

    refused(findings, "declared tier `test-e2e`")


def test_a_recipe_that_runs_nothing_is_refused(tree: Callable[[], Tree]) -> None:
    """A placeholder body is a tier that proves nothing while reporting success."""
    broken = tree()
    broken.edit(
        "justfile",
        "check-repo:\n    uv run -q python -m repo_checks all\n",
        'check-repo:\n    echo "TODO: check the repository"\n',
    )

    findings = recipe_set(broken.repo)

    refused(findings, "is a placeholder")


def test_an_empty_recipe_body_is_refused(tree: Callable[[], Tree]) -> None:
    """An empty body is the same hole with less to read."""
    broken = tree()
    broken.edit(
        "justfile",
        "build:\n    just node-modules\n    bunx nx run-many -t build --output-style=stream\n",
        "build:\n",
    )

    findings = recipe_set(broken.repo)

    refused(findings, "runs nothing")
