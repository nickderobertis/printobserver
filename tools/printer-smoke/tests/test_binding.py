"""The supervisor this run drives is the one attached to the printer it verified.

The device-naming opt-in is the whole of this tool's safety story, and it is
worth nothing if the machine checked and the machine driven can be different
objects. A valid instance record for printer A and an environment naming
supervisor B satisfy every other precondition independently, and the actions
then land on B. So each way the two can come apart is driven here, and each
must be refused before anything is driven.
"""

from __future__ import annotations

from collections.abc import Iterator

import pytest
from machine import Machine
from printer_smoke import CONSERVATIVE_ENVELOPE
from repo_checks.expect import contains, equal
from world import MANIFEST, World, write_the_configuration

REFUSED = "refused: the precondition `supervisor-binding`"


@pytest.fixture
def another_supervisor(world: World) -> Iterator[Machine]:
    """A second supervisor, on an address of its own, that nothing may drive."""
    other = Machine(
        device=world.device,
        envelope=CONSERVATIVE_ENVELOPE,
        manifest=MANIFEST,
        print_id=world.print_id,
    )
    try:
        yield other
    finally:
        other.stop()


def test_an_environment_naming_another_supervisor_is_refused(
    world: World, another_supervisor: Machine
) -> None:
    """The variable that wins over the file cannot point the actions elsewhere."""
    run = world.smoke("--run", environment={"PRINTOBSERVER_SERVER": another_supervisor.url})

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    contains(run.stdout, REFUSED, describing="what it said")
    contains(run.stdout, "different supervisors", describing="what it said")
    equal(another_supervisor.operations, [], describing="what reached the other supervisor")
    equal(world.substitute.commands, [], describing="what reached the verified printer")


def test_a_client_table_naming_another_supervisor_is_refused(
    world: World, another_supervisor: Machine
) -> None:
    """A configuration pointing its own client elsewhere is refused the same way."""
    write_the_configuration(
        world.config, world.substitute.url, client_server=another_supervisor.url
    )

    run = world.smoke("--run", environment={"PRINTOBSERVER_SERVER": ""})

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    contains(run.stdout, REFUSED, describing="what it said")
    equal(another_supervisor.operations, [], describing="what reached the other supervisor")


def test_a_configuration_naming_another_octoprint_is_refused(world: World) -> None:
    """The supervisor's own OctoPrint has to be the instance this run verified."""
    write_the_configuration(world.config, world.substitute.url, octoprint_url="http://127.0.0.1:1/")

    run = world.smoke("--run")

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    contains(run.stdout, REFUSED, describing="what it said")
    contains(run.stdout, "different machines", describing="what it said")
    equal(world.substitute.commands, [], describing="what reached the verified printer")


def test_a_configuration_that_is_not_the_supervisors_own_is_refused(world: World) -> None:
    """A file naming no OctoPrint says nothing about which printer is driven."""
    world.config.write_text(
        f'listen = "{world.substitute.url.removeprefix("http://")}"\n', encoding="utf-8"
    )

    run = world.smoke("--run")

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    contains(run.stdout, REFUSED, describing="what it said")
    contains(run.stdout, "octoprint.url", describing="what it said")


def test_a_configuration_naming_no_supervisor_at_all_is_refused(world: World) -> None:
    """Nothing naming where the supervisor is, is refused before anything is driven."""
    world.config.write_text(f'[octoprint]\nurl = "{world.substitute.url}"\n', encoding="utf-8")

    run = world.smoke("--run", environment={"PRINTOBSERVER_SERVER": ""})

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    contains(run.stdout, REFUSED, describing="what it said")
    contains(run.stdout, "nothing names where the supervisor is", describing="what it said")


def test_an_environment_naming_the_verified_supervisor_is_accepted(world: World) -> None:
    """A variable agreeing with the file names the same machine, and is no divergence."""
    world.substitute.manifest = dict(world.substitute.manifest, file_name="somebody-elses.gcode")

    run = world.smoke("--run", environment={"PRINTOBSERVER_SERVER": world.substitute.url})

    contains(run.stdout, "precondition `supervisor-binding` is met", describing="what it said")
    contains(run.stdout, "refused: the precondition `smoke-manifest`", describing="what it said")
