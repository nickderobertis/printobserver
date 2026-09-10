"""Every precondition, made unmet in turn, with nothing reaching the printer.

The walk below is over the preconditions the smoke itself declares, and the
first test here fails when it declares one this walk does not cover — so the
coverage cannot fall behind what the program actually requires.

"Nothing reached the printer" is asserted as no command reaching it: a
precondition about what the machine is doing cannot be evaluated without
reading what the machine is doing, and a read moves nothing. What must not
happen — and what is asserted after every one of these — is that a run refused
before it started has asked the machine for something.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from printer_smoke import PRECONDITIONS
from repo_checks.expect import contains, equal
from world import World, write_the_configuration, write_the_record


def _no_program(world: World) -> dict[str, str]:
    """Point the smoke at a program that is not on this host."""
    return {"PRINTOBSERVER_SMOKE_PROGRAM": str(world.root / "no-such-printobserver")}


def _no_device(world: World) -> dict[str, str]:
    """Name a serial device that is not there."""
    return {"PRINTOBSERVER_SMOKE_DEVICE": str(world.root / "not-a-device")}


def _a_virtual_printer(world: World) -> dict[str, str]:
    """Record an OctoPrint connected to its own virtual printer instead."""
    write_the_record(world.state_dir, world.substitute.url, world.device, mode="virtual")
    return {}


def _no_manifest_for_the_payload(world: World) -> dict[str, str]:
    """Leave the print carrying a manifest for somebody else's file."""
    world.substitute.manifest = dict(world.substitute.manifest, file_name="somebody-elses.gcode")
    return {}


def _a_printer_that_is_not_operational(world: World) -> dict[str, str]:
    """Leave the machine reporting itself already printing."""
    world.substitute.printer.connection = "printing"
    world.substitute.printer.job_state = "printing"
    return {}


def _a_job_already_running(world: World) -> dict[str, str]:
    """Leave the machine operational and the job paused, as a resumable print is."""
    world.substitute.printer.job_state = "paused"
    return {}


def _a_permissive_envelope(world: World) -> dict[str, str]:
    """Configure the envelope the host had rather than the one this smoke ships."""
    write_the_configuration(
        world.config,
        world.substitute.url,
        {"feedrate": (0.1, 5.0), "tool_target:0": (0.0, 300.0)},
    )
    return {}


#: One way to make each declared precondition unmet, by the name it is declared
#: under. Every precondition the smoke declares is a key here, and the first
#: test asserts exactly that.
UNMET: dict[str, Callable[[World], dict[str, str]]] = {
    "printobserver-command": _no_program,
    "serial-device": _no_device,
    "octoprint-serial-mode": _a_virtual_printer,
    "smoke-manifest": _no_manifest_for_the_payload,
    "printer-operational": _a_printer_that_is_not_operational,
    "no-job-running": _a_job_already_running,
    "safety-envelope": _a_permissive_envelope,
}


def test_the_walk_covers_every_precondition_the_smoke_declares() -> None:
    """A precondition nothing here makes unmet is one nothing here proves fails closed."""
    equal(
        sorted(UNMET),
        sorted(name for name, _ in PRECONDITIONS),
        describing="the preconditions this walk covers",
    )


@pytest.mark.parametrize("name", list(UNMET), ids=list(UNMET))
def test_an_unmet_precondition_stops_the_run_naming_it(world: World, name: str) -> None:
    """It refuses, says which precondition it was, and asks the machine for nothing."""
    environment = UNMET[name](world)

    run = world.smoke("--run", environment=environment)

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    contains(run.stdout, f"refused: the precondition `{name}`", describing="what it said")
    contains(run.stdout, "nothing was asked of the printer", describing="what it said")
    equal(
        world.substitute.commands,
        [],
        describing="the commands that reached the printer before a precondition refused",
    )


def test_a_precondition_about_this_host_reaches_the_printer_at_all(world: World) -> None:
    """The preconditions checked before the machine is read ask it for nothing."""
    run = world.smoke("--run", environment=_no_device(world))

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    equal(
        world.substitute.operations,
        [],
        describing="the operations reaching the supervisor before the device was read",
    )


def test_the_preconditions_are_reported_in_the_order_they_are_declared(world: World) -> None:
    """A reader sees which held before the one that did not."""
    run = world.smoke("--run", environment=_a_permissive_envelope(world))

    met = [line for line in run.stdout.splitlines() if "is met" in line]
    equal(
        [line.split("`")[1] for line in met],
        [name for name, _ in PRECONDITIONS][:-1],
        describing="the preconditions reported met before the one that refused",
    )


def test_a_state_directory_naming_no_octoprint_is_refused(world: World, tmp_path: Path) -> None:
    """A host with no scripted OctoPrint at all is told so by name."""
    empty = tmp_path / "nothing-here"
    empty.mkdir()

    run = world.smoke("--run", environment={"OCTOPRINT_ENV_STATE_DIR": str(empty)})

    equal(run.returncode, 0, describing="the exit of a run a precondition refused")
    contains(run.stdout, "refused: the precondition `octoprint-serial-mode`", describing="it")
    contains(run.stdout, "just octoprint-up", describing="the next action it names")


def test_a_record_naming_another_device_is_refused(world: World) -> None:
    """An OctoPrint on a different serial port is not the machine this run names."""
    write_the_record(world.state_dir, world.substitute.url, "/dev/ttyUSB9")
    (world.state_dir / "instance.json").write_text(
        json.dumps(
            {
                **json.loads((world.state_dir / "instance.json").read_text(encoding="utf-8")),
                "device": "/dev/ttyUSB9",
            }
        ),
        encoding="utf-8",
    )

    run = world.smoke("--run")

    contains(run.stdout, "refused: the precondition `octoprint-serial-mode`", describing="it")
    equal(world.substitute.commands, [], describing="the commands that reached the printer")
