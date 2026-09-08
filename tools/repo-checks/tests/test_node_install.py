"""Every recipe that reaches Nx installs the JavaScript dependencies first."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_repo import node_install
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

BUILD = "build:\n    just node-modules\n    bunx nx run-many -t build --output-style=stream\n"


def test_the_committed_recipes_install_before_they_reach_nx(committed: Repo) -> None:
    """The gate this repository ships runs in a clone that never ran `just bootstrap`."""
    accepted(node_install(committed))


def test_a_tier_reaching_nx_without_installing_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier that assumes an installed tree is a tier a fresh clone cannot run."""
    broken = tree()
    broken.edit("justfile", "format-check:\n    just node-modules\n", "format-check:\n")

    findings = node_install(broken.repo)

    refused(findings, "the `format-check` recipe reaches Nx without running")


def test_an_install_after_the_nx_invocation_is_refused(tree: Callable[[], Tree]) -> None:
    """Healing the state after Nx has already failed heals nothing."""
    broken = tree()
    broken.edit(
        "justfile",
        BUILD,
        "build:\n    bunx nx run-many -t build --output-style=stream\n    just node-modules\n",
    )

    findings = node_install(broken.repo)

    refused(findings, "the `build` recipe reaches Nx without running")


def test_an_unlocked_install_is_refused(tree: Callable[[], Tree]) -> None:
    """An install free to resolve is one the committed lockfile does not describe."""
    broken = tree()
    broken.edit("justfile", "    bun install --frozen-lockfile\n", "    bun install\n")

    findings = node_install(broken.repo)

    refused(findings, "an unlocked install")


def test_a_missing_install_recipe_is_refused(tree: Callable[[], Tree]) -> None:
    """With nothing installing them, every tier is back to failing on a fresh clone."""
    broken = tree()
    broken.edit("justfile", "node-modules:\n    bun install --frozen-lockfile\n", "")

    findings = node_install(broken.repo)

    refused(findings, "declares no `node-modules` recipe")
