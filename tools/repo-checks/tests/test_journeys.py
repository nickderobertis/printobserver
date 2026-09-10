"""The one journey all three clients drive, and the check that holds them to it.

Every journey here drives the committed check over a real copy of the committed
tree with one defect in it, and every defect is one a reader could commit: a
step deleted, a step taken out of its place, a step whose marker no longer has
its own code under it, and a plan asking for an operation the server does not
declare.

The copies are made once and the three journey files put back after each defect,
because what each of these breaks is one file and copying the whole tree
twenty-seven times would be twenty-seven copies of everything else.
"""

from __future__ import annotations

import json
from collections.abc import Iterator

import pytest
from repo_checks.checks_journeys import JOURNEYS, ORDERINGS, STEPS, journey_completeness
from repo_checks.expect import accepted, refused, refused_naming
from treecopy import Tree, copy_tree

#: Where the three journeys live, by the client that drives each.
PATHS = {journey.language: journey.path for journey in JOURNEYS}

#: The server's own operation list, which the plan is held against.
OPERATIONS = "schemas/printobserver-server/operations.json"


@pytest.fixture(scope="module")
def copied(tmp_path_factory: pytest.TempPathFactory) -> Tree:
    """One copy of the committed tree, for every journey in this module."""
    return Tree(copy_tree(tmp_path_factory.mktemp("journeys")))


@pytest.fixture(autouse=True)
def _unbroken(copied: Tree) -> Iterator[None]:
    """Put back whatever one journey broke, so the next starts from the tree."""
    was = {path: copied.read(path) for path in [*PATHS.values(), OPERATIONS]}
    yield
    for path, text in was.items():
        copied.write(path, text)


def _marker(number: int) -> str:
    """How the step with that number declares itself, in any of the three."""
    step = next(one for one in STEPS if one.number == number)
    return f"journey step {step.number}: {step.name}"


def _region(text: str, number: int) -> tuple[int, int]:
    """The lines one step of a journey occupies, as a half-open range.

    From the step's own marker to the next one, or — for the step that is last
    in the file — to the end of whatever block it is written in, which is the
    first line after it that starts a new one at the left margin.
    """
    lines = text.splitlines()
    opens = next(index for index, line in enumerate(lines) if _marker(number) in line)
    others = [
        index
        for index, line in enumerate(lines)
        if index > opens and any(_marker(one.number) in line for one in STEPS)
    ]
    margin = [
        index
        for index, line in enumerate(lines)
        if index > opens and line.strip() and not line[0].isspace()
    ]
    return opens, min([*others[:1], *margin[:1]])


def _without(text: str, number: int) -> str:
    """One journey with that step — its marker and its own code — deleted."""
    lines = text.splitlines(keepends=True)
    opens, closes = _region(text, number)
    return "".join(lines[:opens] + lines[closes:])


def _taken_before(text: str, number: int, other: int) -> str:
    """One journey with that step moved to before the step it comes after."""
    lines = text.splitlines(keepends=True)
    opens, closes = _region(text, number)
    moved = lines[opens:closes]
    rest = lines[:opens] + lines[closes:]
    at, _ = _region("".join(rest), other)
    return "".join(rest[:at] + moved + rest[at:])


def test_the_committed_journeys_take_the_same_nine_steps(copied: Tree) -> None:
    """All three clients drive the one walk, in the order it stands on."""
    accepted(journey_completeness(copied.repo))


@pytest.mark.parametrize("language", sorted(PATHS))
@pytest.mark.parametrize("number", [step.number for step in STEPS])
def test_a_step_deleted_from_a_journey_is_refused(copied: Tree, language: str, number: int) -> None:
    """A journey missing one of the nine proves a walk the other two do not."""
    path = PATHS[language]
    copied.write(path, _without(copied.read(path), number))

    refused_naming(journey_completeness(copied.repo), path, f"journey step {number}")


@pytest.mark.parametrize(("earlier", "later", "why"), ORDERINGS)
def test_a_journey_that_takes_two_steps_the_other_way_round_is_refused(
    copied: Tree, earlier: int, later: int, why: str
) -> None:
    """Each ordering is load-bearing, and the finding says what it holds up."""
    for path in PATHS.values():
        copied.write(path, _taken_before(copied.read(path), later, earlier))

    findings = journey_completeness(copied.repo)
    for path in PATHS.values():
        refused_naming(findings, path, f"journey step {later}", f"journey step {earlier}", why)


#: One step whose own call is taken out from under its marker, per language,
#: spelled as that client spells the call it stops making.
MOVED_AWAY = {
    "rust": (".status(&world.print_id)", ".context(&world.print_id)"),
    "python": ("client.status(world.print_id)", "client.context(world.print_id)"),
    "typescript": ("client.status(world.print_id)", "client.context(world.print_id)"),
}


@pytest.mark.parametrize("language", sorted(MOVED_AWAY))
def test_a_step_whose_own_code_is_gone_is_refused(copied: Tree, language: str) -> None:
    """A marker with nothing under it is a comment, and this is a check over code."""
    path = PATHS[language]
    was, instead = MOVED_AWAY[language]
    copied.write(path, copied.read(path).replace(was, instead, 1))

    refused_naming(journey_completeness(copied.repo), path, "journey step 1", "`status`")


def test_a_journey_that_declares_one_step_twice_is_refused(copied: Tree) -> None:
    """Which of the two the step is cannot be read, so neither can the order."""
    path = PATHS["python"]
    text = copied.read(path)
    copied.write(path, text.replace(_marker(5), f"{_marker(5)}\n    # {_marker(5)}", 1))

    refused_naming(journey_completeness(copied.repo), path, "journey step 5", "2 times")


def test_a_journey_that_declares_a_step_the_walk_has_no_such_one_is_refused(
    copied: Tree,
) -> None:
    """Nine steps, and a tenth is a walk two of the three clients do not drive."""
    path = PATHS["typescript"]
    copied.write(path, copied.read(path).replace(_marker(9), "journey step 10: reboot", 1))

    refused_naming(journey_completeness(copied.repo), path, "journey step 10")


def test_a_journey_that_calls_a_step_something_else_is_refused(copied: Tree) -> None:
    """The three name the same step the same, or a reader has three walks."""
    path = PATHS["rust"]
    copied.write(path, copied.read(path).replace(_marker(2), "journey step 2: pictures", 1))

    refused_naming(journey_completeness(copied.repo), path, "`pictures`", "`context`")


def test_a_journey_a_client_no_longer_carries_is_refused(copied: Tree) -> None:
    """A client with no journey is one nothing drives the nine steps in."""
    path = PATHS["python"]
    copied.remove(path)

    refused_naming(journey_completeness(copied.repo), path, "nothing drives the nine steps")


def test_a_step_about_an_operation_the_server_no_longer_declares_is_refused(
    copied: Tree,
) -> None:
    """The plan cannot go on demanding a call no client has a method for."""
    described = json.loads(copied.read(OPERATIONS))
    described["operations"] = [
        operation for operation in described["operations"] if operation["name"] != "pause"
    ]
    copied.write(OPERATIONS, json.dumps(described, indent=2))

    refused_naming(journey_completeness(copied.repo), "journey step 7", "`pause`")


def test_the_plan_is_read_from_a_tree_whose_operation_list_can_be_read(
    copied: Tree,
) -> None:
    """A list nothing can read is said so, rather than passing for an empty one."""
    copied.write(OPERATIONS, "{ not a document")

    refused(journey_completeness(copied.repo), "operation list could not be read")


def test_every_declared_ordering_is_between_two_steps_the_walk_has(copied: Tree) -> None:
    """An ordering over a step nobody takes would hold nothing up."""
    numbers = {step.number for step in STEPS}
    for earlier, later, _ in ORDERINGS:
        if earlier not in numbers or later not in numbers or earlier >= later:
            message = f"the ordering ({earlier}, {later}) is not one of the nine steps"
            raise AssertionError(message)
    accepted(journey_completeness(copied.repo))
