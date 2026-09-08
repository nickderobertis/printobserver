"""The gate runs in a clone that has never been bootstrapped.

Every publication of this repository is made from a clone cut fresh for it, and
the `pre-push` hook runs the whole gate there. Nothing installs the workspace's
JavaScript dependencies in such a clone, so before this journey existed the
first tier died on `NX Could not find Nx modules` and no change could be pushed
at all.

Each journey below drives the real recipe over a real copy carrying no
installed dependencies. Nothing is mocked: `just` runs, `bun` installs, `nx`
fans out, and the assertion is on what the recipe said and what it left behind.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from journey import REPO_ROOT, GateCopy, output
from repo_checks.expect import absent, contains, equal, failing, passing, truth

PACKAGE_JSON = "package.json"
UNDESCRIBED_DEPENDENCY = '"left-pad": "^1.3.0",\n    "@types/bun": "^1.4.1"'


def test_a_gate_tier_runs_in_a_clone_that_never_bootstrapped(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A tier reaching Nx heals the missing dependency tree instead of dying on it."""
    fresh = gate_copy(node_modules=False)
    truth(
        not (fresh.root / "node_modules").exists(),
        describing="a fresh clone to carry no installed dependencies",
    )

    result = fresh.just("format-check")

    passing(result)
    truth(
        (fresh.root / "node_modules" / "nx").is_dir(),
        describing="the tier to install the Nx modules it needs",
    )
    contains(output(result), "Successfully ran target format-check")


def test_the_install_records_nothing_the_lockfile_does_not_already_describe(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The locked install cannot rewrite the lockfile the gate is meant to prove."""
    fresh = gate_copy(node_modules=False)

    passing(fresh.just("node-modules"))

    equal(
        fresh.read("bun.lock"),
        (REPO_ROOT / "bun.lock").read_text(encoding="utf-8"),
        describing="the lockfile a locked install left alone",
    )


def test_an_already_installed_tree_is_not_installed_again(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """Healing a fresh clone must not cost every other run an install."""
    fresh = gate_copy(node_modules=False)
    first = fresh.just("node-modules")
    passing(first)
    contains(output(first), "packages installed")

    again = fresh.just("node-modules")

    passing(again)
    contains(output(again), "no changes")
    absent(output(again), "packages installed")


def test_the_install_refuses_a_dependency_the_lockfile_does_not_describe(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A manifest the lockfile does not cover stops the tier rather than resolving."""
    broken = gate_copy(node_modules=False)
    broken.edit(PACKAGE_JSON, '"@types/bun": "^1.4.1"', UNDESCRIBED_DEPENDENCY)

    result = broken.just("format-check")

    failing(result, naming="lockfile is frozen")
    absent(output(result), "Successfully ran target format-check")
