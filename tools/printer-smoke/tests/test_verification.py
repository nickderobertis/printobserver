"""The smoke verifies rather than merely issues.

`test_sequence.py` proves which operations the run issued and in what order. A
smoke that read every answer the machine gave and threw it away would pass all
of that — so this is the other half, and it is the half a check on emitted
operations cannot reach. The machine is scripted to answer wrongly at each
verification point in turn, and the committed smoke has to fail naming that
point.
"""

from __future__ import annotations

from typing import NamedTuple

import pytest
from printer_smoke import (
    STEP_ADJUST_INSIDE,
    STEP_ADJUST_OUTSIDE,
    STEP_CANCEL,
    STEP_CONTEXT,
    STEP_INTERVENTION,
    STEP_PAUSE_RESUME,
    STEP_START,
)
from repo_checks.expect import contains, equal
from world import World


class WrongAnswer(NamedTuple):
    """One wrong answer the machine is scripted to give."""

    #: The fault the substitute is scripted with.
    fault: str
    #: The verification point that has to catch it.
    step: str
    #: What the machine does wrong, as the failure is described.
    what: str


#: Each wrong answer, and the verification point it has to be caught at.
WRONG: tuple[WrongAnswer, ...] = (
    WrongAnswer(
        fault="adjustment-ignored",
        step=STEP_ADJUST_INSIDE,
        what="the machine takes an adjustment and goes on reporting the old value",
    ),
    WrongAnswer(
        fault="out-of-bounds-applied",
        step=STEP_ADJUST_OUTSIDE,
        what="the refused request reaches the machine, which then reports the refused value",
    ),
    WrongAnswer(
        fault="expiry-not-restored",
        step=STEP_INTERVENTION,
        what="the bounded change expires and the value it replaced is not put back",
    ),
    WrongAnswer(
        fault="pause-not-taken",
        step=STEP_PAUSE_RESUME,
        what="the machine answers the pause and does not report having taken it",
    ),
    WrongAnswer(
        fault="cancel-not-taken",
        step=STEP_CANCEL,
        what="the machine answers the cancel and goes on reporting a job",
    ),
    WrongAnswer(
        fault="start-names-no-print",
        step=STEP_START,
        what="the start is answered with no print it was recorded against",
    ),
    WrongAnswer(
        fault="bounds-widened",
        step=STEP_CONTEXT,
        what="the context reports bounds wider than the configuration allows",
    ),
)


@pytest.mark.parametrize(("fault", "step", "what"), WRONG, ids=[case.fault for case in WRONG])
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
