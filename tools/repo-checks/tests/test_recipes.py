"""The check recipe invokes every declared tier and no recipe runs nothing."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_repo import recipe_set
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree


def test_the_committed_recipe_set_is_accepted(committed: Repo) -> None:
    """The gate this repository ships invokes every tier it declares."""
    accepted(recipe_set(committed))


def test_two_rust_targets_sharing_a_windows_profile_name_are_refused(
    tree: Callable[[], Tree],
) -> None:
    """Parallel crate runs cannot overwrite one another's Windows profiles."""
    broken = tree()
    broken.edit(
        "crates/printobserver-core/project.json",
        "LLVM_PROFILE_FILE_NAME=printobserver-core-%p-%9m.profraw",
        "LLVM_PROFILE_FILE_NAME=printobserver-types-%p-%9m.profraw",
    )

    findings = recipe_set(broken.repo)

    refused(findings, "shares Windows coverage profile")


def test_a_windows_profile_without_a_bounded_merge_pool_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Reused Windows process IDs cannot overwrite an earlier test's profile."""
    broken = tree()
    broken.edit(
        "crates/printobserver-core/project.json",
        "LLVM_PROFILE_FILE_NAME=printobserver-core-%p-%9m.profraw",
        "LLVM_PROFILE_FILE_NAME=printobserver-core-%p.profraw",
    )

    findings = recipe_set(broken.repo)

    refused(findings, "Windows coverage profile has no bounded merge pool")


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
