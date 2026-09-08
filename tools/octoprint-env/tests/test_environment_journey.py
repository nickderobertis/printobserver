"""The whole scripted environment, in one journey that runs the real script.

Installed unattended over a directory that had no instance and again over the
one that produced; two instances started at once on ports each asked for as
free; a start that answers only once the instance answers its own API and
reports a connected printer; the hold print running when the start returned and
still running after the minimum the script states; and a stop after which no
process this journey started remains.

Nothing here is mocked. `octoprint_env.py` runs as a subprocess, a real
OctoPrint is installed and started, and every observation is an HTTP call to it.
"""

from __future__ import annotations

import time
from collections.abc import Callable
from pathlib import Path

import yaml
from environment import (
    answer,
    api,
    job_file,
    job_state,
    key_from,
    printer_state,
    running,
    said,
    script,
)
from repo_checks.expect import contains, equal, passing, truth

# The minimum this journey holds the script to, stated here rather than read
# out of it: a script that declared a shorter one fails here rather than
# passing against its own choice.
REQUIRED_HOLD_SECONDS = 120

CONNECTED = frozenset({"Operational", "Printing"})
# The G-code file this repository owns, which is what the tier is given to act on.
HOLD_PRINT = "hold.gcode"
REFUSED = frozenset({401, 403})


def test_the_scripted_environment_installs_starts_holds_a_print_and_stops(
    state_dir: Callable[[str], str],
) -> None:
    """One journey over everything the environment is asked to do."""
    first = state_dir("first")
    second = state_dir("second")

    provisioned = _installs_unattended_over_an_empty_directory(first)
    _installs_again_over_the_instance_it_made(first, provisioned)

    one = _starts_answering_and_connected(first)
    two = _starts_answering_and_connected(second)
    _two_instances_do_not_collide(one, two)

    _holds_a_print_for_the_minimum_it_states(one)

    _stops_leaving_no_process(one)
    _stops_leaving_no_process(two)


def _installs_unattended_over_an_empty_directory(state: str) -> dict[str, object]:
    """An install under a directory that had no instance, with no browser in it."""
    truth(not Path(state).exists(), describing="a state directory that has no instance yet")

    result = script("install", "--state-dir", state)

    passing(result, describing="installing into an empty state directory")
    provisioned = answer(result)
    truth(
        Path(str(provisioned["config_file"])).is_file(),
        describing=f"a configuration at {provisioned['config_file']}",
    )
    configured = yaml.safe_load(Path(str(provisioned["config_file"])).read_text(encoding="utf-8"))
    equal(
        configured["server"]["firstRun"],
        False,
        describing="the first-run wizard, which nothing may wait on a browser for",
    )
    truth(
        Path(str(provisioned["api_key_file"])).is_file(),
        describing=f"the API key at {provisioned['api_key_file']}, the path the script named",
    )
    return provisioned


def _installs_again_over_the_instance_it_made(state: str, provisioned: dict[str, object]) -> None:
    """A second run leaves the instance working rather than failing or duplicating it."""
    before = key_from(str(provisioned["api_key_file"]))

    result = script("install", "--state-dir", state)

    passing(result, describing="installing again over the instance the first run made")
    equal(answer(result), provisioned, describing="what a second install provisions")
    equal(
        key_from(str(provisioned["api_key_file"])),
        before,
        describing="the API key, which a second install must not replace",
    )
    instances = sorted(p.name for p in Path(state).iterdir() if p.is_dir())
    equal(instances, ["instance", "venv"], describing="what the state directory holds")


def _starts_answering_and_connected(state: str) -> dict[str, object]:
    """A start that has not answered until the instance has."""
    result = script("up", "--state-dir", state, "--port", "auto")

    passing(result, describing=f"starting the instance under {state}")
    started = answer(result)
    key = key_from(str(started["api_key_file"]))
    url = str(started["url"])

    # Asserted at the moment the start returned: this is what "does not answer
    # until the instance answers its own API and reports a connected printer"
    # means from the caller's side.
    status, version = api(url, "/api/version", key)
    equal(status, 200, describing=f"the API of {url} at the moment the start returned")
    truth(isinstance(version, dict), describing="a decoded /api/version answer")
    contains(CONNECTED, printer_state(url, key), describing="printer states that are connected")
    return started


def _two_instances_do_not_collide(one: dict[str, object], two: dict[str, object]) -> None:
    """Two runs on one host, each on a port it asked for as free."""
    truth(
        one["port"] != two["port"],
        describing=f"two different ports, not {one['port']} twice",
    )
    for instance, other in ((one, two), (two, one)):
        url = str(instance["url"])
        status, _ = api(url, "/api/version", key_from(str(instance["api_key_file"])))
        equal(status, 200, describing=f"{url} answering on its own")
        contains(
            REFUSED,
            api(url, "/api/version", key_from(str(other["api_key_file"])))[0],
            describing=f"{url} refusing the other instance's key, so the two are distinct",
        )


def _holds_a_print_for_the_minimum_it_states(started: dict[str, object]) -> None:
    """A print that is running when the start returns and after the stated minimum."""
    url = str(started["url"])
    key = key_from(str(started["api_key_file"]))
    stated = int(str(started["hold_seconds"]))

    truth(
        stated >= REQUIRED_HOLD_SECONDS,
        describing=f"a stated hold of at least {REQUIRED_HOLD_SECONDS}s, not {stated}s",
    )
    equal(
        job_file(url, key),
        HOLD_PRINT,
        describing="the file the instance uploaded, selected and started",
    )
    truth(
        job_state(url, key).startswith("Printing"),
        describing=f"a running print when the start returned, not `{job_state(url, key)}`",
    )

    time.sleep(stated)

    equal(job_file(url, key), HOLD_PRINT, describing="the file still selected")
    after = job_state(url, key)
    truth(
        after.startswith("Printing"),
        describing=(
            f"a running print after the {stated}s the script states it holds one for, not `{after}`"
        ),
    )


def _stops_leaving_no_process(started: dict[str, object]) -> None:
    """A stop after which the process this journey started is gone."""
    pid = int(str(started["pid"]))
    truth(running(pid), describing=f"process {pid} to still be running before it is stopped")

    result = script("down", "--state-dir", str(started["state_dir"]))

    passing(result, describing=f"stopping the instance under {started['state_dir']}")
    equal(answer(result)["stopped"], True, describing="the stop's own account of itself")
    truth(not running(pid), describing=f"process {pid} to be gone: {said(result)}")
