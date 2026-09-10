"""The smoke verifies rather than merely issues.

`test_sequence.py` proves which operations the run issued and in what order. A
smoke that read every answer the machine gave and threw it away would pass all
of that — so this is the other half, and it is the half a check on emitted
operations cannot reach. The machine is scripted to answer wrongly at each
verification point in turn, and the committed smoke has to fail naming that
point.
"""

from __future__ import annotations

import pytest
from printer_smoke import (
    STEP_ADJUST_INSIDE,
    STEP_ADJUST_OUTSIDE,
    STEP_CANCEL,
    STEP_CONTEXT,
    STEP_INTERVENTION,
    STEP_PAUSE_RESUME,
)
from repo_checks.expect import contains, equal
from world import World

#: Each wrong answer, and the verification point it has to be caught at.
WRONG: tuple[tuple[str, str, str], ...] = (
    (
        "adjustment-ignored",
        STEP_ADJUST_INSIDE,
        "the machine takes an adjustment and goes on reporting the old value",
    ),
    (
        "out-of-bounds-applied",
        STEP_ADJUST_OUTSIDE,
        "the refused request reaches the machine, which then reports the refused value",
    ),
    (
        "expiry-not-restored",
        STEP_INTERVENTION,
        "the bounded change expires and the value it replaced is not put back",
    ),
    (
        "pause-not-taken",
        STEP_PAUSE_RESUME,
        "the machine answers the pause and does not report having taken it",
    ),
    (
        "cancel-not-taken",
        STEP_CANCEL,
        "the machine answers the cancel and goes on reporting a job",
    ),
    (
        "bounds-widened",
        STEP_CONTEXT,
        "the context reports bounds wider than the configuration allows",
    ),
)


@pytest.mark.parametrize(("fault", "step", "what"), WRONG, ids=[case[0] for case in WRONG])
def test_a_wrong_answer_fails_the_smoke_naming_where(
    world: World, fault: str, step: str, what: str
) -> None:
    """Each verification point catches its own wrong answer and says which it was."""
    world.substitute.fail(fault)

    run = world.smoke("--run")

    equal(run.returncode, 1, describing=f"the exit of a run where {what}")
    contains(run.stdout, f"FAILED at `{step}`", describing="what the run said")


def test_a_machine_that_answers_everything_correctly_passes(world: World) -> None:
    """The same assertions pass over a machine that does what it is asked."""
    run = world.smoke("--run")

    equal(run.returncode, 0, describing="the exit of a run against a correct machine")
    contains(run.stdout, "passed:", describing="what the run said")
