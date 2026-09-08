"""The assertion vocabulary fails when it should, and says why.

A helper that quietly passed would make every suite that uses it green and
worthless, so each one is driven over an input it must reject and an input it
must accept. `pytest.raises` is the mechanism, so nothing here needs `assert`.
"""

from __future__ import annotations

import subprocess

import pytest
from repo_checks.expect import (
    absent,
    accepted,
    contains,
    equal,
    failing,
    passing,
    refused,
    refused_naming,
    truth,
)

FINDINGS = ["the gate refused a thing", "and another thing"]


def _run(code: int, out: str = "") -> subprocess.CompletedProcess[str]:
    return subprocess.CompletedProcess(["a-program"], code, out, "")


def test_accepted_passes_on_no_findings() -> None:
    """An empty result is what being accepted looks like."""
    accepted([])


def test_accepted_fails_naming_every_finding() -> None:
    """A reader of the failure sees what the check actually said."""
    with pytest.raises(AssertionError, match="and another thing"):
        accepted(FINDINGS)


def test_refused_passes_when_a_finding_names_it() -> None:
    """A substring of any finding is a match."""
    refused(FINDINGS, "refused a thing")


def test_refused_fails_when_nothing_names_it() -> None:
    """The failure prints what there was instead."""
    with pytest.raises(AssertionError, match="the gate refused a thing"):
        refused(FINDINGS, "a defect nobody reported")


def test_refused_fails_loudly_on_no_findings_at_all() -> None:
    """The empty case is the one a broken check produces, so it must not pass."""
    with pytest.raises(AssertionError, match=r"\(none\)"):
        refused([], "anything")


def test_refused_naming_wants_all_the_parts_in_one_finding() -> None:
    """Two findings that each name one part are not one that names both."""
    refused_naming(FINDINGS, "gate", "refused")
    with pytest.raises(AssertionError, match="another"):
        refused_naming(FINDINGS, "gate", "another")


def test_equal_compares_and_reports_both_sides() -> None:
    """The failure shows what was expected and what arrived."""
    equal(2 + 2, 4)
    with pytest.raises(AssertionError, match="expected 5; got 4"):
        equal(2 + 2, 5)


def test_contains_and_absent_are_opposites() -> None:
    """Membership either way, each failing on the other's input."""
    contains("a haystack", "hay")
    absent("a haystack", "needle")
    with pytest.raises(AssertionError, match="needle"):
        contains("a haystack", "needle")
    with pytest.raises(AssertionError, match="hay"):
        absent("a haystack", "hay")


def test_truth_reports_what_was_expected() -> None:
    """The description is the whole message, so it has to carry the meaning."""
    truth(True, describing="something that holds")
    with pytest.raises(AssertionError, match="a printer to be idle"):
        truth(False, describing="a printer to be idle")


def test_passing_accepts_a_zero_exit_and_prints_a_failure() -> None:
    """A failed run comes back with everything it said."""
    passing(_run(0))
    with pytest.raises(AssertionError, match="it exited 1"):
        passing(_run(1, "the reason it failed"))


def test_failing_wants_a_non_zero_exit_that_names_the_defect() -> None:
    """A run that succeeded, or failed for another reason, is not the defect."""
    failing(_run(1, "unknown field `bad_setting`"), naming="bad_setting")
    with pytest.raises(AssertionError, match="the run succeeded"):
        failing(_run(0, "all good"), naming="bad_setting")
    with pytest.raises(AssertionError, match="expected the failure to name"):
        failing(_run(1, "something else went wrong"), naming="bad_setting")


def test_both_outcome_shapes_are_accepted() -> None:
    """A completed process and an (exit status, output) pair mean the same thing."""
    passing((0, ""))
    failing((3, "it named the defect"), naming="the defect")
