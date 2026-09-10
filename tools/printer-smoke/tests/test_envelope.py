"""The envelope this smoke ships is bounded by the rules rather than by itself.

Requiring the configuration to equal what the test ships is satisfied exactly
as well by a permissive envelope faithfully required, which is the fail-open
case this file exists to close. The ceiling below is `AGENTS.md`'s "The
real-printer smoke test" and this task's, written out here so that the shipped
envelope is held to it rather than to itself: no range it declares may be wider
than one of these, and none may be for something this ceiling does not bound.
"""

from __future__ import annotations

import pytest
from printer_smoke import CONSERVATIVE_ENVELOPE
from repo_checks.expect import accepted, refused_naming

#: The widest each range may be. Feedrate and flowrate are factors, the two
#: temperatures are degrees Celsius and the fan is a percentage. They
#: accommodate PLA comfortably and exclude by construction every material
#: needing temperatures nobody has reviewed.
CEILING: dict[str, tuple[float, float]] = {
    "feedrate": (0.5, 1.2),
    "flowrate": (0.9, 1.1),
    "tool_target:0": (0.0, 230.0),
    "bed_target": (0.0, 70.0),
    "fan": (0.0, 100.0),
}


def wider_than_the_ceiling(envelope: dict[str, tuple[float, float]]) -> list[str]:
    """Everything about one envelope that this ceiling does not admit."""
    findings = [
        f"the shipped envelope declares `{name}`, which this ceiling does not bound"
        for name in sorted(envelope)
        if name not in CEILING
    ]
    for name, (low, high) in CEILING.items():
        declared = envelope.get(name)
        if declared is None:
            findings.append(f"the shipped envelope declares no range for `{name}`")
        elif declared[0] < low or declared[1] > high:
            findings.append(
                f"the shipped envelope allows `{name}` from {declared[0]:g} to {declared[1]:g}, "
                f"wider than the {low:g} to {high:g} this ceiling allows"
            )
    return findings


def test_the_shipped_envelope_is_within_the_ceiling() -> None:
    """No range the smoke requires of a host is wider than the ceiling."""
    accepted(wider_than_the_ceiling(CONSERVATIVE_ENVELOPE), describing="the shipped envelope")


@pytest.mark.parametrize("name", sorted(CEILING), ids=sorted(CEILING))
def test_an_envelope_widened_past_the_ceiling_is_refused(name: str) -> None:
    """Widening any one of the five is refused naming that one."""
    low, high = CEILING[name]
    widened = dict(CONSERVATIVE_ENVELOPE) | {name: (low, high * 2 + 1)}

    refused_naming(wider_than_the_ceiling(widened), f"`{name}`", "wider than")


def test_an_envelope_missing_one_of_the_five_is_refused() -> None:
    """An adjustable the envelope does not bound is one nothing bounds."""
    without_the_fan = {
        name: value for name, value in CONSERVATIVE_ENVELOPE.items() if name != "fan"
    }

    refused_naming(wider_than_the_ceiling(without_the_fan), "no range for `fan`")


def test_an_envelope_bounding_something_this_ceiling_does_not_is_refused() -> None:
    """A range for an adjustable nobody reviewed is not one this ceiling admits."""
    with_a_chamber = dict(CONSERVATIVE_ENVELOPE) | {"tool_target:1": (0.0, 300.0)}

    refused_naming(wider_than_the_ceiling(with_a_chamber), "`tool_target:1`")
