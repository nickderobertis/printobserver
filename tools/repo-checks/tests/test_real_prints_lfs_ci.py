"""`real-prints-lfs-ci`: no CI job downloads the cases but through the committed steps.

Driven through `python -m repo_checks real-prints-lfs-ci --root`, which is what
`just lint-workflows` runs, over the committed workflows and over a copy given
one workflow more: the job the real-print suite's own node will add, written
here as it would be and then broken in one way at a time.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

import pytest
import yaml
from closures import closure
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal
from treecopy import REPO_ROOT, Tree

CLOSURE = closure("real-prints-lfs-ci")
CHECK = "real-prints-lfs-ci"
STEPS: list[dict[str, Any]] = yaml.safe_load(
    (REPO_ROOT / "tests/real-prints-lfs/ci-steps.yml").read_text(encoding="utf-8")
)
CHECKOUT: dict[str, Any] = {"uses": "actions/checkout@v5", "with": {"lfs": False}}
BOOTSTRAP: dict[str, Any] = {"run": "just bootstrap"}
SUITE: dict[str, Any] = {"run": "just skilltest"}

Run = Callable[[Tree | None], tuple[int, str]]


@pytest.fixture
def check(capsys: pytest.CaptureFixture[str]) -> Run:
    """Run the check over a tree, answering its exit and what it printed."""

    def run(tree: Tree | None = None) -> tuple[int, str]:
        root = REPO_ROOT if tree is None else tree.root
        code = main([CHECK, "--root", str(root)])
        return code, capsys.readouterr().err

    return run


def with_job(tree: Tree, steps: list[dict[str, Any]]) -> Tree:
    """A copy carrying one workflow more, whose one job runs `steps`."""
    workflow = {
        "name": "suite",
        "on": {"workflow_dispatch": {}},
        "jobs": {"suite": {"runs-on": "ubuntu-24.04", "steps": steps}},
    }
    tree.write(".github/workflows/suite.yml", yaml.safe_dump(workflow, sort_keys=False))
    return tree


def test_the_committed_workflows_pass(check: Run) -> None:
    """Every committed checkout declares `lfs: false`, and no job fetches."""
    equal(check(None), (0, ""))


def test_a_job_carrying_the_steps_verbatim_passes(check: Run, tree: Callable[[], Tree]) -> None:
    """The suite's job as its node will write it: checkout, bootstrap, the steps, the suite."""
    equal(check(with_job(tree(), [CHECKOUT, BOOTSTRAP, *STEPS, SUITE])), (0, ""))


@pytest.mark.parametrize(
    "options",
    [None, {"fetch-depth": 0}, {"lfs": True}, {"lfs": "false"}],
    ids=["no-with", "no-lfs", "lfs-true", "lfs-a-string"],
)
def test_a_checkout_without_lfs_false_is_refused(
    check: Run, tree: Callable[[], Tree], options: dict[str, Any] | None
) -> None:
    """A checkout not declaring `lfs: false` may download every object."""
    step: dict[str, Any] = {"uses": "actions/checkout@v5"}
    if options is not None:
        step["with"] = options
    code, err = check(with_job(tree(), [step]))
    equal(code, 1)
    contains(
        err, "suite.yml: job `suite` checks out with `actions/checkout@v5` and no `lfs: false`"
    )


@pytest.mark.parametrize(
    "fetch",
    ["just real-prints-fetch", "git lfs pull", "just real-prints-checkout"],
)
def test_a_job_fetching_without_the_steps_is_refused(
    check: Run, tree: Callable[[], Tree], fetch: str
) -> None:
    """A fetch with no cache in front of it spends the bandwidth on every run."""
    code, err = check(with_job(tree(), [CHECKOUT, {"run": fetch}, SUITE]))
    equal(code, 1)
    contains(err, "job `suite` reaches the real-print cases' LFS objects without carrying")


def test_steps_carried_with_a_change_are_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """Saving the cache on every run, hit or miss, is no longer the committed steps."""
    changed = [dict(step) for step in STEPS]
    saving = next(step for step in changed if step.get("uses") == "actions/cache/save@v4")
    del saving["if"]
    code, err = check(with_job(tree(), [CHECKOUT, *changed, SUITE]))
    equal(code, 1)
    contains(err, "without carrying tests/real-prints-lfs/ci-steps.yml verbatim")


def test_a_fetch_beside_the_steps_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """Carrying the steps does not license a second fetch after them."""
    code, err = check(with_job(tree(), [CHECKOUT, *STEPS, {"run": "just real-prints-fetch"}]))
    equal(code, 1)
    contains(err, "runs `just real-prints-fetch` outside tests/real-prints-lfs/ci-steps.yml")


def test_unreadable_steps_are_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """With no steps to hold a job to, the check says so rather than passing every job."""
    copy = tree()
    copy.write("tests/real-prints-lfs/ci-steps.yml", "just: a mapping\n")
    code, err = check(copy)
    equal(code, 1)
    contains(err, "ci-steps.yml is not a list of steps")
