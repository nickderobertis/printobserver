"""The check recipe invokes every declared tier and no recipe runs nothing."""

from __future__ import annotations

from collections.abc import Callable

from conftest import Tree
from repo_checks.checks_repo import recipe_set
from repo_checks.model import Repo


def test_the_committed_recipe_set_is_accepted(committed: Repo) -> None:
    """The gate this repository ships invokes every tier it declares."""
    assert recipe_set(committed) == []


def test_a_check_recipe_omitting_a_declared_tier_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate that skips the end-to-end tier is a gate that proves less than it says."""
    broken = tree()
    broken.edit("justfile", "    just test-e2e\n", "")

    findings = recipe_set(broken.repo)

    assert any("declared tier `test-e2e`" in finding for finding in findings), findings


def test_a_recipe_that_runs_nothing_is_refused(tree: Callable[[], Tree]) -> None:
    """A placeholder body is a tier that proves nothing while reporting success."""
    broken = tree()
    broken.edit(
        "justfile",
        "check-repo:\n    uv run -q python -m repo_checks all\n",
        'check-repo:\n    echo "TODO: check the repository"\n',
    )

    findings = recipe_set(broken.repo)

    assert any("is a placeholder" in finding for finding in findings), findings


def test_an_empty_recipe_body_is_refused(tree: Callable[[], Tree]) -> None:
    """An empty body is the same hole with less to read."""
    broken = tree()
    broken.edit(
        "justfile",
        "build:\n    bunx nx run-many -t build --output-style=stream\n",
        "build:\n",
    )

    findings = recipe_set(broken.repo)

    assert any("runs nothing" in finding for finding in findings), findings
