"""A clean clone reaches a state in which the gate runs, by running the recipe.

This is a journey rather than a reading: the recipe is executed in a directory
carrying no build products, no installed toolchain state and no dependency tree,
and then a declared tier of the gate is run to completion there.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from pathlib import Path

from journey import REPO_ROOT, run
from repo_checks.expect import contains, passing, truth
from repo_checks.model import Repo

BUILD_PRODUCTS = (".venv", "node_modules", "target", ".nx", "dist")
BASE_BRANCH = str(Repo(REPO_ROOT).policy["repository"]["base_branch"])


def reads(repository: Path, ref: str) -> bool:
    """Whether a repository resolves a fully-qualified ref to a commit."""
    found = run(["git", "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"], cwd=repository)
    return found.returncode == 0


def carrying_the_base_branch(clone: Path) -> None:
    """Give the clone the base branch a clone of the published repository carries.

    `git clone` copies the source's own *branches* into `refs/remotes/origin/*`
    and copies none of its remote-tracking refs — and the source here is
    whichever checkout this tier is running in. On a developer's machine that is
    one carrying the base branch as a branch, so the clone arrives with
    `origin/main` and every check reading the commit this work was cut from has
    something to read. On the checkout a pull request is built from it is a
    remote-tracking ref and no branch at all, so the same clone arrives with no
    base branch at all and those checks refuse the tree for having no history to
    compare against. Fetching it by name is what makes the clone the same clone
    in both places.

    Raises:
        AssertionError: If the checkout this tier is running in carries the base
            branch under neither name, which is a tree nothing here can cut a
            faithful clone of.
    """
    named = f"refs/remotes/origin/{BASE_BRANCH}"
    for ref in (f"refs/heads/{BASE_BRANCH}", named):
        if reads(REPO_ROOT, ref):
            passing(
                run(["git", "fetch", "--no-tags", str(REPO_ROOT), f"+{ref}:{named}"], cwd=clone),
                describing=f"the fetch of `{ref}` into the clone",
            )
            break
    else:
        message = (
            f"this checkout carries `{BASE_BRANCH}` as neither a branch nor a "
            f"remote-tracking ref, so no clone of it can carry the base branch"
        )
        raise AssertionError(message)
    truth(
        reads(clone, named),
        describing=f"the clone to carry `{named}`, which the install-path check reads",
    )


def test_bootstrap_brings_a_clean_clone_to_a_state_in_which_the_gate_runs(
    tmp_path: Path,
) -> None:
    """`just bootstrap` then a declared tier, in a directory that started with nothing."""
    clone = tmp_path / "clean-clone"
    # A real clone carries committed history even though it carries none of the
    # source's build products, and the checks reading the commit this work was
    # cut from need it; the helper is why cloning alone does not always give it.
    passing(run(["git", "clone", "--no-local", str(REPO_ROOT), str(clone)], cwd=tmp_path))
    carrying_the_base_branch(clone)
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
