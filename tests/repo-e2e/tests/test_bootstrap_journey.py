"""A clean clone reaches a state in which the gate runs, by running the recipe.

This is a journey rather than a reading: the recipe is executed in a directory
carrying no build products, no installed toolchain state and no dependency tree,
and then a declared tier of the gate is run to completion there.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from pathlib import Path

from journey import REPO_ROOT, copy_tracked, run
from repo_checks.expect import contains, passing, truth

BUILD_PRODUCTS = (".venv", "node_modules", "target", ".nx", "dist", ".git")


def test_bootstrap_brings_a_clean_clone_to_a_state_in_which_the_gate_runs(
    tmp_path: Path,
) -> None:
    """`just bootstrap` then a declared tier, in a directory that started with nothing."""
    clone = copy_tracked(tmp_path / "clean-clone")
    for product in BUILD_PRODUCTS:
        truth(
            not (clone / product).exists(),
            describing=f"a clean clone to carry no {product}",
        )

    bootstrap = run(["just", "bootstrap"], cwd=clone)
    passing(bootstrap)

    truth((clone / ".venv").is_dir(), describing="bootstrap to create the environment")
    truth(
        (clone / "node_modules").is_dir(),
        describing="bootstrap to install the dependency tree",
    )

    tier = run(["just", "check-repo"], cwd=clone)
    passing(tier)


def test_the_bootstrap_recipe_is_the_one_the_gate_job_runs() -> None:
    """The journey above drives the same recipe continuous integration drives."""
    workflow = (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")

    contains(workflow, "- run: just bootstrap")
    contains(workflow, "- run: just check")
