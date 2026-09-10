"""`AGENTS.md`'s journey inventory names the journeys the suite carries.

The marker is what makes the inventory auditable: a check enumerates the marked
tests rather than guessing which of several thousand assertions is a user-facing
journey. What this establishes is that the two cannot drift — a marked journey
with no entry and an entry with no journey are both refused.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_docs import journeys
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

SUITE = "crates/printobserver/tests/cli.rs"


def test_the_committed_inventory_is_accepted(committed: Repo) -> None:
    """The inventory this repository ships names exactly the marked journeys."""
    accepted(journeys(committed))


def test_a_journey_the_inventory_does_not_list_is_refused(tree: Callable[[], Tree]) -> None:
    """A journey nobody wrote down is coverage nobody can audit."""
    copy = tree()
    copy.append(
        SUITE,
        "\n// journey: reading-the-programs-own-version\n"
        "#[test]\nfn the_version_reads_back() {\n    let _ = printobserver::VERSION;\n}\n",
    )

    refused(journeys(copy.repo), "`reading-the-programs-own-version` in")


def test_an_inventory_entry_naming_no_journey_is_refused(tree: Callable[[], Tree]) -> None:
    """And an entry for a journey the suite no longer carries is a stale claim."""
    copy = tree()
    copy.edit(
        "AGENTS.md",
        "[//]: # (BEGIN journey-inventory)\n",
        "[//]: # (BEGIN journey-inventory)\n"
        "- `printing-a-part-that-does-not-exist` — a journey nothing carries.\n",
    )

    refused(journeys(copy.repo), "names `printing-a-part-that-does-not-exist`")


def test_a_marker_that_sits_above_no_test_names_no_journey(tree: Callable[[], Tree]) -> None:
    """The marker names the test under it, so one above nothing is not a journey."""
    copy = tree()
    copy.append(SUITE, "\n// journey: a-marker-above-nothing\nconst UNUSED: u8 = 1;\n")

    accepted(journeys(copy.repo), describing="a marker with no test under it")
