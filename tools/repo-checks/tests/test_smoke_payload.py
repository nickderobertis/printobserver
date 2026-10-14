"""The payload the real-printer smoke test prints, and the check that reads it.

Two halves, and the second is what stops the first being circular. The first
drives the check over a payload failing in each declared way and asserts each
is refused. The second holds the check itself to the set and the length
`AGENTS.md` and `repo-policy.toml` name — written out literally here, because a
check that chose its own safe set could call a dangerous command safe and one
that chose its own maximum could pick a length no file could exceed, and every
assertion in the first half would pass over either.
"""

from __future__ import annotations

from collections.abc import Callable

import pytest
from repo_checks.checks_smoke import max_lines, safe_commands, smoke_payload
from repo_checks.expect import accepted, equal, refused, refused_naming
from repo_checks.model import PolicyValueError, Repo
from treecopy import Tree

# The closed set the payload may be drawn from, as this repository's own rules
# name it. The anchor: the check is held to this rather than to itself.
CLOSED_SET = (
    "G21",
    "G90",
    "G91",
    "G92",
    "G28",
    "G0",
    "G1",
    "M82",
    "M83",
    "M104",
    "M109",
    "M140",
    "M190",
    "M106",
    "M107",
    "M84",
)

# The longest the payload may be, in lines.
LENGTH = 200

# The exclusions that matter, each excluded by not being in the set above
# rather than by a list of its own: the firmware writes, the calibration
# routines and the emergency stop.
EXCLUDED = ("M500", "M502", "M303", "G29", "M112")

# A payload that heats, moves, and says at its head what it needs.
COMPLETE = """; a payload
; heat: tool0 205 C for 900 s
G21
G90
M104 S205
M109 S205
G28
G1 X100.000 Y100.000 E1.00000 F1200
M104 S0
M84
"""

GCODE = "tools/printer-smoke/gcode/smoke.gcode"


def payload(tree: Tree, text: str) -> list[str]:
    """Put one payload in a copy of the tree and read what the check makes of it."""
    tree.write(GCODE, text)
    return smoke_payload(tree.repo)


def test_the_committed_payload_is_accepted(committed: Repo) -> None:
    """The file this repository ships passes every rule the check has."""
    accepted(smoke_payload(committed))


def test_the_check_enforces_exactly_the_closed_set_this_repository_names(
    committed: Repo,
) -> None:
    """The set the check reads is the set the rules name, in that order."""
    equal(safe_commands(committed), CLOSED_SET, describing="the safe command set")
    equal(max_lines(committed), LENGTH, describing="the payload's maximum length")


@pytest.mark.parametrize("command", EXCLUDED)
def test_a_command_outside_the_closed_set_is_refused(
    tree: Callable[[], Tree], command: str
) -> None:
    """Each exclusion that matters is refused for not being in the set."""
    findings = payload(tree(), COMPLETE.replace("M84\n", f"{command}\nM84\n"))

    refused_naming(findings, command, "not one of the commands")


def test_a_payload_longer_than_the_declared_maximum_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A payload nobody can read in full before it reaches a machine."""
    filler = "".join(f"G1 X{index % 100}.000 Y10.000 F1200\n" for index in range(LENGTH))

    findings = payload(tree(), COMPLETE + filler)

    refused_naming(findings, f"longer than the {LENGTH} lines")


def test_an_empty_payload_is_refused_for_moving_nothing(tree: Callable[[], Tree]) -> None:
    """An empty file satisfies every safety condition by containing nothing."""
    findings = payload(tree(), "")

    refused(findings, "carries no command that moves the machine")


def test_a_payload_that_only_heats_is_refused_for_moving_nothing(
    tree: Callable[[], Tree],
) -> None:
    """Heating and never moving is inert exactly as an empty file is."""
    findings = payload(tree(), "; heat: bed 60 C for 900 s\nM140 S60\nM190 S60\n")

    refused(findings, "carries no command that moves the machine")


def test_a_payload_that_heats_without_declaring_it_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A file is heat-free or says at its head what it needs; there is no third way."""
    findings = payload(tree(), COMPLETE.replace("; heat: tool0 205 C for 900 s\n", ""))

    refused_naming(findings, "neither heat-free nor", "tool0 at 205 C")


def test_a_declaration_naming_a_temperature_nothing_sets_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A truthful declaration and a false one are otherwise indistinguishable."""
    findings = payload(tree(), COMPLETE.replace("; a payload\n", "; heat: bed 60 C for 900 s\n"))

    refused_naming(findings, "declares bed at 60 C", "which no command in it sets")


def test_a_heating_command_the_declaration_does_not_account_for_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The declaration is held to the commands in both directions."""
    findings = payload(tree(), COMPLETE.replace("M104 S0\n", "M140 S60\nM104 S0\n"))

    refused_naming(findings, "bed at 60 C", "does not account for")


def test_a_heat_free_payload_that_moves_is_accepted(tree: Callable[[], Tree]) -> None:
    """The other way a payload may be safe: it never heats at all."""
    findings = payload(tree(), "G21\nG90\nG28\nG1 X10.000 Y10.000 F1200\nM84\n")

    accepted(findings, describing="a heat-free payload that moves")


def test_a_cooldown_needs_no_declaration(tree: Callable[[], Tree]) -> None:
    """Setting a heater to zero turns it off; nobody has to have been told."""
    findings = payload(tree(), "G21\nG28\nG1 X10.000 Y10.000 F1200\nM104 S0\nM140 S0\nM84\n")

    accepted(findings, describing="a payload that only cools down")


def test_a_line_that_is_no_command_at_all_is_refused(tree: Callable[[], Tree]) -> None:
    """A payload carrying something no firmware would read is refused naming it."""
    findings = payload(tree(), COMPLETE.replace("M84\n", "sudo reboot\nM84\n"))

    refused_naming(findings, "'sudo'", "which is no G-code command")


def test_a_declaration_after_the_head_is_refused(tree: Callable[[], Tree]) -> None:
    """A declaration a reader meets after the commands it is about is not one."""
    findings = payload(tree(), COMPLETE + "; heat: bed 60 C for 900 s\nM140 S60\n")

    refused_naming(findings, "after the head of the file")


def test_a_declaration_in_a_shape_the_check_cannot_read_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A shape nothing can read is a declaration nobody is held to."""
    findings = payload(tree(), COMPLETE.replace("205 C for 900 s", "hot for a while"))

    refused_naming(findings, "cannot read", "; heat: <tool0|bed> <degrees> C for <seconds> s")


def test_a_tool_number_is_read_off_the_command(tree: Callable[[], Tree]) -> None:
    """A second tool is a second heater, and its declaration names it."""
    heats_tool_one = COMPLETE.replace("M109 S205\n", "M109 T1 S215\n")

    findings = payload(tree(), heats_tool_one)

    refused_naming(findings, "tool1 at 215 C", "does not account for")


def test_a_payload_the_policy_names_and_the_tree_does_not_hold_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A rule guarding a file nothing carries has stopped being a rule."""
    copy = tree()
    copy.remove(GCODE)

    refused(smoke_payload(copy.repo), "which is not there")


def test_a_tree_declaring_no_smoke_section_is_refused(tree: Callable[[], Tree]) -> None:
    """The check reads its own anchor, and refuses a tree carrying none."""
    copy = tree()
    copy.write("repo-policy.toml", copy.read("repo-policy.toml").replace("[smoke]", "[unsmoke]"))

    refused(smoke_payload(copy.repo), "declares no `[smoke]` section")


def test_a_maximum_that_is_no_length_is_refused(tree: Callable[[], Tree]) -> None:
    """A malformed anchor is one finding about the anchor, not an attribute error."""
    copy = tree()
    copy.edit("repo-policy.toml", "max_lines = 200", 'max_lines = "long"')

    with pytest.raises(PolicyValueError, match=r"smoke\.max_lines"):
        max_lines(copy.repo)


def test_a_heating_command_carrying_no_readable_temperature_sets_nothing(
    tree: Callable[[], Tree],
) -> None:
    """A command whose value nothing can read asks for no temperature at all."""
    unreadable = COMPLETE.replace("M104 S205", "M104 Swarm").replace("M109 S205", "M109 Swarm")

    findings = payload(tree(), unreadable)

    refused_naming(findings, "declares tool0 at 205 C", "which no command in it sets")


def test_blank_lines_and_ordinary_comments_are_read_past(tree: Callable[[], Tree]) -> None:
    """A payload a person can read is one they may lay out, and neither is a command."""
    findings = payload(tree(), "; a square\n\nG21\n\n; and it moves\nG1 X1.000 F1200\n")

    accepted(findings, describing="a payload carrying blank lines and comments")
