"""What the client's contract check admits, held to declarations the readers depend on.

`conforms` walks the shape table generated from the contracts. These hold it to
the two declarations whose rules are not plain types: a member declared as the
null scalar, which a valid document sends as null, and a recovery allowance,
whose `allowed` is false exactly when its `refusal` is sent.
"""

from __future__ import annotations

from printobserver_sdk._shapes import conforms
from repo_checks.expect import equal

#: An instant as the contracts write one.
AT = "2026-10-27T12:00:00Z"


def test_a_member_declared_null_is_read_only_as_null() -> None:
    """An unchecked detector's `since` is the null scalar: null is it, an instant is not."""
    equal(conforms({"status": "unchecked", "since": None}, "DetectorHealth"), True)
    equal(conforms({"status": "unchecked", "since": AT}, "DetectorHealth"), False)
    equal(conforms({"status": "available", "since": AT}, "DetectorHealth"), True)
    equal(conforms({"status": "lost", "since": AT}, "DetectorHealth"), False)


def test_an_allowance_is_read_only_when_its_two_halves_agree() -> None:
    """Allowed with no refusal, or refused with one; either contradiction, or a null, is not."""
    for agreeing in (
        {"action": "recover_lift", "allowed": True},
        {"action": "recover_heat", "allowed": False, "refusal": "z_untrusted"},
    ):
        equal(conforms(agreeing, "ActionAllowance"), True, describing=f"{agreeing!r}")
    for contradiction in (
        {"action": "recover_lift", "allowed": True, "refusal": "z_untrusted"},
        {"action": "recover_heat", "allowed": False},
        {"action": "recover_lift", "allowed": True, "refusal": None},
    ):
        equal(conforms(contradiction, "ActionAllowance"), False, describing=f"{contradiction!r}")
