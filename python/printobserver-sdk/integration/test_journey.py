"""The one journey all three clients drive, in Python.

Nine steps against a **real supervisor backed by a real `OctoPrint`** — the
instance `just octoprint-up` started, with its virtual printer. Nothing here
stands in for a layer: the client is the published one, the server is the
program this repository builds, and the machine is a real one.

The same nine steps run in the Rust and the Node clients, in the same order,
asserting the same normalized answers. Three clients running three different
journeys would prove three different products.

Four orderings are load-bearing. The manifest is written before the print is
started, so the print runs under it. The accepted adjustment and the rejected
one both happen after the print has started and before history is read, so
history has them to account for. The print is cancelled last.
"""

from __future__ import annotations

import contextlib
import hashlib
import time
from pathlib import Path
from typing import cast

import pytest
from printobserver_sdk import Client, NoReasonError, RejectedError
from printobserver_sdk.contract import JobManifest, PrinterState
from repo_checks.expect import contains, equal, truth
from world import Standing, Supervisor

#: The reason every mutating step of this walk carries.
REASON = "a printer-integration journey is asking"

#: How long a bounded adjustment stands for, in whole seconds.
DURATION = 60

#: The feedrate factor the accepted adjustment asks for, inside the envelope
#: the world is configured with, and the one the refused adjustment asks for.
INSIDE = 1.1
OUTSIDE = 9.9

#: How long the machine is given to reach a state a step needs.
PATIENCE_SECONDS = 180.0


@pytest.fixture(scope="module")
def world(tmp_path_factory: pytest.TempPathFactory) -> object:
    """A supervisor over the scripted `OctoPrint`, held up for this module."""
    standing = Standing(tmp_path_factory.mktemp("world"))
    with standing as supervisor:
        yield supervisor


def _manifest(file_name: str) -> JobManifest:
    """The manifest this walk writes, which the print then runs under."""
    return cast(
        JobManifest,
        {
            "file_name": file_name,
            "material": "PLA",
            "nozzle_diameter_mm": 0.4,
            "slicer_profile": "a journey's own profile",
            "allowed": {"feedrate": {"min": 0.9, "max": 1.2}},
            "metadata": {},
        },
    )


def _until(client: Client, print_id: str, wanted: set[PrinterState]) -> None:
    """Wait until the machine reports one of these states.

    Raises:
        AssertionError: If it does not, saying what it reported instead.
    """
    deadline = time.monotonic() + PATIENCE_SECONDS
    last: object = "nothing was reported"
    while time.monotonic() < deadline:
        printer = client.status(print_id).get("printer")
        if printer is not None:
            last = printer["connection"]
            if last in wanted:
                return
        time.sleep(0.5)
    message = f"the machine reported {last!r} and this step needs one of {wanted}"
    raise AssertionError(message)


def test_the_same_nine_steps_are_answered_against_a_real_octoprint(world: Supervisor) -> None:
    """The nine steps, in the one order a real machine admits."""
    client = Client(world.server, "operator")

    # 1. Read status.
    equal(client.status(world.print_id)["print"]["id"], world.print_id)

    # 2. Read context, and materialize its latest image.
    equal(client.context(world.print_id)["context"]["print"]["id"], world.print_id)
    image = client.image(world.image_id)
    path = image.get("path")
    truth(isinstance(path, str), describing="the image to be on the server's own host")
    truth(Path(str(path)).is_absolute(), describing=f"{path} to be an absolute path")
    equal(
        hashlib.sha256(Path(str(path)).read_bytes()).hexdigest(),
        image["record"]["sha256"],
        describing="the file at the answered path",
    )

    # 3. Write a job manifest and read it back, before the print is started.
    wanted = _manifest(world.file_name)
    written = client.manifest_set(world.print_id, REASON, wanted)
    equal(written["manifest"], wanted)
    equal(client.manifest_get(world.print_id)["manifest"], written["manifest"])

    # Setting the running print down is this walk's own setup rather than one
    # of the nine steps; it starts one again at the end.
    with contextlib.suppress(RejectedError):
        client.cancel(world.print_id, "making room for the step that starts one")
    _until(client, world.print_id, {"operational"})

    # 4. Start a print.
    started = client.start_print(world.print_id, world.file_name, wanted, REASON)
    equal(started["record"]["decision"], "accepted")
    _until(client, world.print_id, {"printing"})

    # 5. One accepted adjustment, carrying a reason and a duration.
    adjusted = client.set_feedrate_factor(world.print_id, INSIDE, REASON, DURATION)
    equal(adjusted["record"]["decision"], "accepted")
    intervention = adjusted.get("intervention")
    truth(intervention is not None, describing="a bounded adjustment to open an intervention")
    equal(cast("dict[str, object]", intervention)["applied_value"], INSIDE)

    # 6. One adjustment outside the effective bounds, and the typed rejection.
    try:
        client.set_feedrate_factor(world.print_id, OUTSIDE, REASON)
    except RejectedError as refused:
        truth(
            isinstance(refused.reason, dict) and "out_of_bounds" in refused.reason,
            describing="the refusal itself to be carried",
        )
        equal(refused.requested, OUTSIDE)
        allowed = refused.allowed
        truth(allowed is not None, describing="the range that is allowed")
        truth(
            cast("dict[str, float]", allowed)["max"] < OUTSIDE,
            describing="what is allowed not to admit what was asked for",
        )
    else:
        truth(False, describing="an adjustment outside the bounds to be refused")

    # 7. One mutating call whose reason is empty, and one whose reason is
    #    omitted — both refused here, with no request reaching the server.
    _unreasoned(client, world)

    # 8. Read history, and find the accepted action, its decision and outcome.
    _accounting(client, world)

    # 9. Cancel the print, last.
    cancelled = client.cancel(world.print_id, REASON)
    equal(cancelled["record"]["decision"], "accepted")

    # Teardown rather than a tenth step: the environment's own suite asserts a
    # print is there to be acted on.
    _until(client, world.print_id, {"operational"})
    client.start_print(
        world.print_id,
        world.file_name,
        wanted,
        "putting the hold print back where the bring-up left it",
    )


def _unreasoned(client: Client, world: Supervisor) -> None:
    """A mutating call with no reason reaches no server at all."""
    before = len(client.history(world.print_id)["events"])

    for empty in ("", "   "):
        try:
            client.pause(world.print_id, empty)
        except NoReasonError:
            continue
        truth(False, describing=f"{empty!r} to be refused for want of a reason")

    # Omitted rather than empty: in this client the reason is a required
    # argument that can still be left out at run time, and leaving it out is
    # refused where the two typed clients refuse it at compile time.
    try:
        client.pause(world.print_id)  # ty: ignore[missing-argument]
    except TypeError:
        pass
    else:
        truth(False, describing="an omitted reason to be refused")

    # Pointed at an address nothing is listening on, the same call still
    # refuses for want of a reason rather than for want of a server — which is
    # what "no request reached the server" means.
    nowhere = Client("127.0.0.1:1", "operator")
    try:
        nowhere.pause(world.print_id, "")
    except NoReasonError:
        pass
    else:
        truth(False, describing="a call with no reason to reach no server")

    equal(
        len(client.history(world.print_id)["events"]),
        before,
        describing="what a call with no reason left in the history",
    )


def _accounting(client: Client, world: Supervisor) -> None:
    """The history accounts for the accepted action, its decision and its outcome."""
    events = client.history(world.print_id, 200)["events"]
    asked = [
        event
        for event in events
        if event["kind"] == "action_requested"
        and event["payload"]["action"]["action"] == "set_feedrate_factor"
        and event["payload"]["action"]["factor"] == INSIDE
    ]
    truth(asked, describing="the accepted adjustment to be in the history")
    action_id = asked[0]["payload"]["action_id"]

    rejected = {
        event["payload"]["action_id"] for event in events if event["kind"] == "action_rejected"
    }
    truth(
        action_id not in rejected,
        describing="the accepted adjustment not to be in the history as a rejected one",
    )
    executed = {
        event["payload"]["action_id"] for event in events if event["kind"] == "action_executed"
    }
    contains(executed, action_id, describing="the actions the history says reached the machine")
    truth(rejected, describing="the history to account for the refused adjustment")
