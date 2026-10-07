"""The check recipe invokes every declared tier and no recipe runs nothing."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

import pytest
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
        "LLVM_PROFILE_FILE_NAME=printobserver-core-%p-%m.profraw",
        "LLVM_PROFILE_FILE_NAME=printobserver-types-%p-%m.profraw",
    )

    findings = recipe_set(broken.repo)

    refused(findings, "shares Windows coverage profile")


@pytest.mark.parametrize(
    "profile", ["printobserver-core-%p.profraw", "printobserver-core-%m.profraw"]
)
def test_a_windows_profile_name_a_reused_process_identifier_could_overwrite_is_refused(
    tree: Callable[[], Tree], profile: str
) -> None:
    """Parallel test processes on Windows share identifiers; a file per process must merge."""
    broken = tree()
    broken.edit(
        "crates/printobserver-core/project.json",
        "LLVM_PROFILE_FILE_NAME=printobserver-core-%p-%m.profraw",
        f"LLVM_PROFILE_FILE_NAME={profile}",
    )

    findings = recipe_set(broken.repo)

    refused(findings, "which a reused process identifier could overwrite")


def test_a_check_recipe_omitting_a_declared_tier_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate that skips the end-to-end tier is a gate that proves less than it says."""
    broken = tree()
    broken.edit("justfile", "    just test-e2e {{quote(tier)}}\n", "")

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
        'build tier="affected":\n    just node-modules\n'
        "    uv run -q python -m repo_checks.gate_tier run {{quote(tier)}} build\n",
        'build tier="affected":\n',
    )

    findings = recipe_set(broken.repo)

    refused(findings, "runs nothing")


def test_a_check_recipe_taking_no_tier_is_refused(tree: Callable[[], Tree]) -> None:
    """The tier is a flag on the one gate command, never a second gate."""
    broken = tree()
    broken.edit("justfile", 'check tier="affected":\n', "check:\n")

    refused(recipe_set(broken.repo), "the `check` recipe takes no `tier` parameter")


def test_a_graph_tier_taking_no_tier_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier that cannot be told which projects to run over runs what it likes."""
    broken = tree()
    broken.edit("justfile", 'typecheck tier="affected":\n', "typecheck:\n")

    refused(recipe_set(broken.repo), "the `typecheck` recipe reaches Nx and takes no `tier`")


def test_a_graph_tier_running_nx_itself_is_refused(tree: Callable[[], Tree]) -> None:
    """`nx run-many` in a tier would sweep every project whatever tier `check` was asked for."""
    broken = tree()
    broken.edit(
        "justfile",
        "    uv run -q python -m repo_checks.gate_tier run {{quote(tier)}} build\n",
        "    bunx nx run-many -t build --output-style=stream\n",
    )

    refused(recipe_set(broken.repo), "reaches Nx without the gate-tier runner deciding")


def test_a_check_not_handing_its_tier_down_is_refused(tree: Callable[[], Tree]) -> None:
    """`just check all` that ran the affected lint would be a sweep in name only."""
    broken = tree()
    broken.edit("justfile", "    just lint {{quote(tier)}}\n", "    just lint\n")

    refused(recipe_set(broken.repo), "runs `just lint` without handing it its own `tier`")
