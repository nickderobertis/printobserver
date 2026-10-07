"""The tier runs against the environment `just octoprint-up` started.

This is the file that makes the tier a tier rather than a suite of its own
journeys: it drives the instance the bring-up recipe left running, the way the
printer adapter's own integration tests will. Run on its own, with nothing
brought up, it says so and fails — `just octoprint-up` is what comes first, and
`just octoprint-down` is what comes after.
"""

from __future__ import annotations

import json
import os
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
from environment import api, job_state, key_from, post, printer_state, running, tool_target
from repo_checks.expect import contains, equal, truth

STATE_DIR = Path(os.environ.get("OCTOPRINT_ENV_STATE_DIR", ".octoprint-env"))
AUTHENTICATED = "/api/settings"
CONNECTED = frozenset({"Operational", "Printing"})
REFUSED = frozenset({401, 403})

#: Commands sent into one print: enough to carry its line counter, which every
#: job's `M110` starts again, past line 115 — the last line OctoPrint's virtual
#: printer injects a communication fault at unless it is configured not to.
PAST_THE_INJECTED_FAULTS = 120

#: What OctoPrint logs each time the printer asks for a line again.
RESEND_REQUEST = "Got a resend request from the printer"

#: The hotend target the last command sets, which is how the walk knows the
#: printer has answered every command before it, and the one it is put back to.
MARKER_TARGET = 1.0
HOLDING_TARGET = 0.0

#: How long the machine is given to reach a state it was asked for: a cancel
#: waits out the dwell in flight, which the hold print makes ten seconds long.
SETTLE_S = 120.0


@pytest.fixture(scope="module")
def brought_up() -> dict[str, Any]:
    """What the bring-up recipe recorded about the instance it started.

    Raises:
        AssertionError: If nothing is running, naming the recipe that starts it.
    """
    record = STATE_DIR / "instance.json"
    if not record.is_file():
        message = (
            f"no instance is recorded at {record.resolve()}: this tier runs against the "
            f"environment `just octoprint-up` starts, and `just octoprint-down` stops"
        )
        raise AssertionError(message)
    return json.loads(record.read_text(encoding="utf-8"))


def test_the_instance_the_recipe_started_is_running(brought_up: dict[str, Any]) -> None:
    """The process the recipe started is there, and answering its own API."""
    truth(
        running(int(brought_up["pid"])),
        describing=f"the process {brought_up['pid']} the bring-up recipe started",
    )

    status, version = api(str(brought_up["url"]), "/api/version", key_from_record(brought_up))

    equal(status, 200, describing=f"{brought_up['url']} answering its own API")
    truth(isinstance(version, dict), describing="a decoded /api/version answer")


def test_the_instance_authenticates_the_tier(brought_up: dict[str, Any]) -> None:
    """The tier reaches it with the provisioned key, and nothing reaches it without one."""
    url = str(brought_up["url"])

    carried, _ = api(url, AUTHENTICATED, key_from_record(brought_up))
    refused, _ = api(url, AUTHENTICATED, None)

    equal(carried, 200, describing=f"GET {AUTHENTICATED} carrying the provisioned key")
    contains(REFUSED, refused, describing=f"the status of GET {AUTHENTICATED} carrying no key")


def test_the_instance_reports_a_connected_printer(brought_up: dict[str, Any]) -> None:
    """A tier that drives a printer needs one connected."""
    state = printer_state(str(brought_up["url"]), key_from_record(brought_up))

    contains(CONNECTED, state, describing="printer states that are connected")


def test_the_print_the_recipe_started_is_there_to_be_acted_on(
    brought_up: dict[str, Any],
) -> None:
    """The hold print is what gives the tier something to act on."""
    if not brought_up["printing"]:
        message = (
            "the bring-up recipe started no print; `--print` is what the tier needs to "
            "have something to act on"
        )
        raise AssertionError(message)

    state = job_state(str(brought_up["url"]), key_from_record(brought_up))

    truth(state.startswith("Printing"), describing=f"a running print, not `{state}`")


def test_a_print_driven_past_a_hundred_lines_meets_no_injected_fault(
    brought_up: dict[str, Any],
) -> None:
    """A print the tier adjusts past line 115 is answered without a resend.

    The integration tier cancels, restarts and adjusts its prints many times,
    and every adjustment is a numbered line of the running print. OctoPrint's
    virtual printer injects a fault at lines 100, 105, 110 and 115 by default;
    on a slow Windows host one of them, landing in a cancel and a restart, left
    the printer `Offline after error` with the tier half walked. So the hold
    print is restarted here — its `M110` starting the count again — and carried
    past all four, and OctoPrint's own log is read for the resend requests a
    fault answers with. The print is left running, as the bring-up left it.
    """
    url = str(brought_up["url"])
    key = key_from_record(brought_up)
    log = Path(str(brought_up["state_dir"])) / "instance" / "logs" / "octoprint.log"

    equal(post(url, "/api/job", key, {"command": "cancel"}), 204, describing="the cancel")
    settled(lambda: printer_state(url, key), "Operational", describing="the printer state")
    already = log.read_text(encoding="utf-8", errors="replace").count(RESEND_REQUEST)
    restart = {"command": "select", "print": True}
    equal(post(url, "/api/files/local/hold.gcode", key, restart), 204, describing="the restart")
    settled(lambda: printer_state(url, key), "Printing", describing="the printer state")
    for line in range(PAST_THE_INJECTED_FAULTS):
        command = {"commands": [f"M117 printobserver line {line}"]}
        equal(post(url, "/api/printer/command", key, command), 204, describing="a command")
    marker = {"commands": [f"M104 T0 S{MARKER_TARGET:g}"]}
    equal(post(url, "/api/printer/command", key, marker), 204, describing="the marker")
    settled(lambda: tool_target(url, key), MARKER_TARGET, describing="the hotend target")
    holding = {"commands": [f"M104 T0 S{HOLDING_TARGET:g}"]}
    equal(post(url, "/api/printer/command", key, holding), 204, describing="the put-back")
    settled(lambda: tool_target(url, key), HOLDING_TARGET, describing="the hotend target")

    resent = log.read_text(encoding="utf-8", errors="replace").count(RESEND_REQUEST) - already
    equal(resent, 0, describing=f"the resend requests {log} records over the print")
    equal(printer_state(url, key), "Printing", describing="the printer state afterwards")


def settled(read: Callable[[], object], wanted: object, *, describing: str) -> None:
    """Wait until `read` answers `wanted`, failing with what it answered last.

    Raises:
        AssertionError: If it does not inside `SETTLE_S`.
    """
    deadline = time.monotonic() + SETTLE_S
    seen = read()
    while seen != wanted and time.monotonic() < deadline:
        time.sleep(0.5)
        seen = read()
    equal(seen, wanted, describing=f"{describing} after {SETTLE_S:g}s")


def key_from_record(brought_up: dict[str, Any]) -> str:
    """The API key, read from the path the bring-up recipe named."""
    return key_from(str(brought_up["api_key_file"]))
