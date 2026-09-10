"""The sequence the smoke issues, and that it verifies rather than merely issues.

This drives the committed smoke against a machine the test controls and asserts
the first half of what it is for: the operations it issued, in the order it
issued them, and a closing history assertion that accounts for every action the
run produced. `test_verification.py` is the other half — a smoke that read the
machine's answers and threw them away would pass everything here.
"""

from __future__ import annotations

from repo_checks.expect import contains, equal, passing, truth
from world import World

#: The operations the run issues that change the machine, in order. The five
#: adjustments inside their bounds, the five refused ones, the bounded change,
#: the pause and the resume, the five restores and the cancel.
ADJUSTMENTS = (
    "set_feedrate_factor",
    "set_flowrate_factor",
    "set_tool_target_c",
    "set_bed_target_c",
    "set_fan_percent",
)


def test_the_smoke_drives_the_sequence_in_the_order_it_declares(world: World) -> None:
    """Context, start, adjust, refuse, expire, pause, resume, restore, cancel, history."""
    run = world.smoke("--run")

    passing(run, describing="the smoke against a machine the test controls")
    issued = world.substitute.operations
    equal(issued[0], "manifest", describing="the read the manifest precondition makes")
    contains(issued, "status", describing="the reads the preconditions make")
    commands = world.substitute.commands
    equal(
        commands,
        [
            "start_print",
            *ADJUSTMENTS,
            *ADJUSTMENTS,
            "set_feedrate_factor",
            "pause",
            "resume",
            *ADJUSTMENTS,
            "cancel",
        ],
        describing="the operations the run asked the machine for, in order",
    )
    truth(
        issued.index("cancel") < issued.index("history"),
        describing="the history read to come after the cancel it accounts for",
    )
    equal(
        set(issued[issued.index("history") + 1 :]),
        {"status"},
        describing="the reads after the history, which are the cleanup's own last look "
        "at the machine and ask it for nothing",
    )


def test_the_context_read_comes_before_anything_is_started(world: World) -> None:
    """The bounds are read before the print they bound is started."""
    passing(world.smoke("--run"))

    issued = world.substitute.operations
    truth(
        issued.index("context") < issued.index("start_print"),
        describing="the context read to come before the print is started",
    )


def test_the_history_accounts_for_every_action_the_run_produced(world: World) -> None:
    """Every request, its decision and its outcome are in the record afterwards."""
    run = world.smoke("--run")

    passing(run)
    events = world.substitute.events
    requested = [event for event in events if event["kind"] == "action_requested"]
    decided = [event for event in events if event["kind"] in {"action_executed", "action_rejected"}]
    equal(len(decided), len(requested), describing="one decision or outcome per request")
    equal(
        len([event for event in events if event["kind"] == "action_rejected"]),
        len(ADJUSTMENTS),
        describing="one rejection per adjustable asked for outside its bound",
    )
    truth(
        any(event["kind"] == "intervention_expired" for event in events),
        describing="the bounded intervention to have expired in the record",
    )
    contains(run.stdout, "the history accounts for all", describing="what the run said")
