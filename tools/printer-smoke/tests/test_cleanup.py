"""The machine is left as it was found, on every exit path this run has.

A run that finishes without restoring what it changed leaves the machine
altered exactly as a crashed one does, so all three paths are walked here: a
run that completes, a run interrupted while it is driving the machine, and a
run failed at each stage that has already sent something to the printer. After
every one of them the print is cancelled, every adjustable this run changed
carries the value it found there, and the printer is operational.
"""

from __future__ import annotations

import pytest
from repo_checks.expect import absent, contains, equal, truth
from world import World

#: Each stage that has already sent something to the printer, and the operation
#: to fail there. The count is how many of that operation to let through first,
#: which is how a run is failed at one stage rather than at the first stage
#: that uses it.
STAGES: tuple[tuple[str, str, int], ...] = (
    ("start-print", "start_print", 0),
    ("adjust-inside", "set_feedrate_factor", 0),
    ("adjust-outside", "set_feedrate_factor", 1),
    ("bounded-intervention", "set_feedrate_factor", 2),
    ("pause-resume", "pause", 0),
    ("cancel", "cancel", 0),
)


def left_as_found(world: World, found: dict[str, float], *, after: str, said: str = "") -> None:
    """Fail unless the machine carries what this run found, and nothing of the run.

    Args:
        world: The machine the run drove.
        found: What it was holding before the run.
        after: What the run did, for the failure message.
        said: Everything the run said, so that a failure here is diagnosable
            from its own output rather than from a second run.

    Raises:
        AssertionError: If the print is not cancelled, if a value was not put
            back, or if the printer is not operational.
    """
    where = f"{after}, which said:\n{said}" if said else after
    for name, value in found.items():
        equal(world.substitute.held(name), value, describing=f"`{name}` after {where}")
    equal(world.substitute.state(), "operational", describing=f"the printer after {where}")
    truth(
        world.substitute.printer.job_state not in {"printing", "paused"},
        describing=f"no job to be running after {where}; it is "
        f"{world.substitute.printer.job_state!r}",
    )


def test_a_run_that_completes_leaves_the_machine_as_it_found_it(world: World) -> None:
    """The successful path cleans up as much as the failed one does."""
    found = dict(world.substitute.printer.values)

    run = world.smoke("--run")

    equal(run.returncode, 0, describing="the exit of a run that completed")
    left_as_found(world, found, after="a run that completed", said=run.stdout)


def test_an_interrupted_run_leaves_the_machine_as_it_found_it(world: World) -> None:
    """A person stopping the run at the machine is an exit path like any other."""
    found = dict(world.substitute.printer.values)

    run = world.interrupt_the_smoke()

    contains(run.stdout, "interrupted", describing="what the interrupted run said")
    truth(
        "start_print" in world.substitute.commands,
        describing="the interrupted run to have started the print before it was stopped",
    )
    left_as_found(world, found, after="a run somebody interrupted", said=run.stdout)


@pytest.mark.parametrize(
    ("stage", "operation", "after"), STAGES, ids=[stage for stage, _, _ in STAGES]
)
def test_a_run_failed_at_a_stage_leaves_the_machine_as_it_found_it(
    world: World, stage: str, operation: str, after: int
) -> None:
    """Each stage that has already moved the machine puts it back when it fails."""
    found = dict(world.substitute.printer.values)
    world.substitute.refuse(operation, after=after)

    run = world.smoke("--run")

    equal(run.returncode, 1, describing=f"the exit of a run failed at `{stage}`")
    contains(run.stdout, f"FAILED at `{stage}`", describing="what the failed run said")
    left_as_found(world, found, after=f"a run failed at `{stage}`", said=run.stdout)


def test_a_run_whose_cleanup_cannot_put_a_value_back_exits_non_zero(world: World) -> None:
    """A machine whose cancel cools its heaters is one this run cannot fully put back.

    Every verification point passes and the run still fails, because what it
    leaves behind is the machine holding this run's own values. A green report
    over that is the worst answer this program can give, so it is the one this
    asserts against.
    """
    world.substitute.fail("cancel-cools-the-heaters")

    run = world.smoke("--run")

    equal(run.returncode, 1, describing="the exit of a run that left the machine changed")
    absent(run.stdout, "passed:", describing="what a run that left the machine changed said")
    absent(run.stdout, "FAILED at", describing="what it said: no verification point failed")
    contains(run.stdout, "LEFT CHANGED: `tool_target:0`", describing="what it said")
    contains(run.stdout, "LEFT CHANGED: `bed_target`", describing="what it said")


def test_a_restore_that_cannot_be_made_is_still_attempted_for_the_rest(world: World) -> None:
    """One value that cannot be put back does not cost the four after it.

    The feedrate's own restore is refused for the rest of the run — the fourth
    request of it onwards, which is the one the cleanup makes. Every other
    adjustable is put back, the run exits non-zero, and what it reports first is
    the verification point that failed rather than the cleanup beneath it.
    """
    found = dict(world.substitute.printer.values)
    world.substitute.refuse("set_feedrate_factor", times=99, after=3)

    run = world.smoke("--run")

    equal(run.returncode, 1, describing="the exit of a run that could not put a value back")
    for name in ("flowrate", "tool_target:0", "bed_target", "fan"):
        equal(
            world.substitute.held(name),
            found[name],
            describing=f"`{name}`, which the failed restore of `feedrate` must not have cost",
        )
    truth(
        world.substitute.held("feedrate") != found["feedrate"],
        describing="the feedrate to be the one this run could not put back",
    )
    contains(run.stdout, "LEFT CHANGED: `feedrate`", describing="what it said")
    said = run.stdout.splitlines()
    truth(
        next(i for i, line in enumerate(said) if "FAILED at" in line)
        < next(i for i, line in enumerate(said) if "LEFT CHANGED" in line),
        describing="the verification point that failed to be reported before the cleanup, "
        f"so that the cleanup never replaces it as the cause; it said:\n{run.stdout}",
    )


def test_a_restoration_that_never_answers_costs_neither_the_rest_nor_the_cancel(
    world: World,
) -> None:
    """A command that hangs is one adjustable's failure and not the cleanup's end.

    The fourth request of the feedrate — the one the cancel step makes to put it
    back — never answers. The four adjustables after it are still put back, the
    print is still cancelled, and what the run reports first is the verification
    point that failed rather than the cleanup beneath it.
    """
    found = dict(world.substitute.printer.values)

    run = world.smoke(
        "--run",
        environment=world.relaying(hang_on="set-feedrate-factor", after=3),
        timeout=600,
    )

    equal(run.returncode, 1, describing="the exit of a run whose restoration never answered")
    contains(run.stdout, "never ran to completion", describing="what it said about the command")
    for name in ("flowrate", "tool_target:0", "bed_target", "fan"):
        equal(
            world.substitute.held(name),
            found[name],
            describing=f"`{name}`, which the hung restore of `feedrate` must not have cost",
        )
    equal(
        world.substitute.state(),
        "operational",
        describing="the printer, which the hung restore must not have cost the cancel",
    )
    truth(
        world.substitute.printer.job_state not in {"printing", "paused"},
        describing="the print to have been cancelled after the restoration never answered",
    )
    contains(run.stdout, "LEFT CHANGED: `feedrate`", describing="what it said")
    said = run.stdout.splitlines()
    truth(
        next(i for i, line in enumerate(said) if "FAILED at" in line)
        < next(i for i, line in enumerate(said) if "LEFT CHANGED" in line),
        describing="the verification point that failed to be reported before the cleanup, "
        f"so that the cleanup never replaces it as the cause; it said:\n{run.stdout}",
    )


def test_a_final_read_that_never_answers_is_reported_rather_than_passed_over(
    world: World,
) -> None:
    """A machine this run cannot see afterwards is not one it may report green on.

    Every verification point passes, and from the history read onwards the
    machine stops answering. The cancellation is still attempted over a state
    nothing could read — the safe half of that ignorance — and the run exits
    non-zero saying what it could not tell rather than `passed`.
    """
    run = world.smoke(
        "--run",
        environment=world.relaying(hang_on="status", armed_by="history", timeout_s="1"),
        timeout=600,
    )

    equal(run.returncode, 1, describing="the exit of a run that could not read the machine")
    absent(run.stdout, "passed:", describing="what a run that could not see the machine said")
    absent(run.stdout, "FAILED at", describing="what it said: no verification point failed")
    contains(run.stdout, "UNVERIFIED:", describing="what it said")
    contains(run.stdout, "could not be read", describing="what it said")
    equal(
        world.substitute.commands.count("cancel"),
        2,
        describing="the cancels the machine received: the sequence's own, and the cleanup's "
        "over a state it could not read",
    )
