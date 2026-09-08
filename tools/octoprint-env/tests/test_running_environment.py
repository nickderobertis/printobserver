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
from pathlib import Path
from typing import Any

import pytest
from environment import api, job_state, key_from, printer_state, running
from repo_checks.expect import contains, equal, truth

STATE_DIR = Path(os.environ.get("OCTOPRINT_ENV_STATE_DIR", ".octoprint-env"))
AUTHENTICATED = "/api/settings"
CONNECTED = frozenset({"Operational", "Printing"})
REFUSED = frozenset({401, 403})


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


def key_from_record(brought_up: dict[str, Any]) -> str:
    """The API key, read from the path the bring-up recipe named."""
    return key_from(str(brought_up["api_key_file"]))
