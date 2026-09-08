"""The committed continuous-integration configuration is runnable, not just well-shaped.

`just lint-workflows` is the validation: `actionlint` parses and checks every
workflow, and this repository's own policy checks hold the actions to the pinned
form it declares and every step's command to the agent allowlist. Each journey
below runs that tier over a copy carrying one defect.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from journey import GateCopy
from repo_checks.expect import failing, passing

CI = ".github/workflows/ci.yml"


def test_the_validation_accepts_the_committed_configuration(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Every committed workflow parses, is pinned, and runs allowlisted commands."""
    clean = gate_copy()

    result = clean.just("lint-workflows")

    passing(result)


def test_a_workflow_that_does_not_parse_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A configuration that cannot be read cannot be run."""
    broken = gate_copy()
    broken.write(CI, "name: ci\non: [push]\njobs:\n  gate:\n  - this is not a job\n")

    result = broken.just("lint-workflows")

    failing(result, naming="ci.yml")


def test_an_action_outside_the_declared_pinned_form_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A branch ref re-points under the repository with no commit here."""
    broken = gate_copy()
    broken.edit(CI, "      - uses: actions/checkout@v5\n", "      - uses: actions/checkout@main\n")

    result = broken.just("lint-workflows")

    failing(result, naming="not pinned in the form")


def test_a_step_running_a_command_the_allowlist_does_not_name_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A workflow step is not a way around the allowlist the recipes are held to."""
    broken = gate_copy()
    broken.edit(
        CI,
        "      - run: just check\n",
        "      - run: just check\n      - run: rm -rf /tmp/whatever\n",
    )

    result = broken.just("lint-workflows")

    failing(result, naming="the command allowlist does not name")
