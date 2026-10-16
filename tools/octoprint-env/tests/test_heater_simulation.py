"""The virtual printer heats a bed that stands exactly `delta` short of its target.

OctoPrint's virtual printer simulates each heater by stepping it towards its
target, at a fixed rate while it is more than `delta` (0.5 °C) away and at a
rate of the remaining distance when it is less. The pinned release tests those
two cases and nothing else, so a heater exactly `delta` short raises
`UnboundLocalError` in the thread answering the serial line, and the printer
goes silent. A print reaches that state at random, which is how it reached the
integration tier: a bed at 59.50 °C against a target of 60.00 °C.

This journey puts the machine there on purpose. The virtual printer starts
every heater at its configured ambient temperature, so an instance configured
with an ambient of 59.5 °C has its bed exactly `delta` short of a 60 °C target
the moment that target is set — through OctoPrint's own API, as a print sets it.
"""

from __future__ import annotations

import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

import yaml
from environment import answer, api, key_from, post, printer_state, script
from repo_checks.expect import contains, equal, passing, truth

#: Where the bed starts, and the target exactly the simulation's `delta` above it.
AMBIENT_C = 59.5
TARGET_C = 60.0

#: What the defect raises, as Python words it in the traceback it logs.
DEFECT = "UnboundLocalError"

#: Long enough for several of OctoPrint's temperature polls to be answered.
HEATING_S = 20.0

#: How long a freshly connected printer is given to answer its first poll.
REPORTED_S = 30.0

CONNECTED = frozenset({"Operational", "Printing"})


def _bed(url: str, key: str) -> dict[str, Any]:
    """The bed's actual and target temperature, once the instance reports them.

    A printer that has just connected has answered no temperature poll yet, so
    this waits for the first answer rather than reading an empty one.
    """
    deadline = time.monotonic() + REPORTED_S
    while True:
        status, body = api(url, "/api/printer", key)
        temperature = body.get("temperature") if isinstance(body, dict) else None
        bed = temperature.get("bed") if isinstance(temperature, dict) else None
        if isinstance(bed, dict) or time.monotonic() >= deadline:
            break
        time.sleep(1.0)
    equal(status, 200, describing="GET /api/printer")
    truth(isinstance(bed, dict), describing=f"a bed temperature in {body!r}")
    return bed if isinstance(bed, dict) else {}


def _raised(state: Path) -> list[str]:
    """Every traceback ending in the defect that the server or OctoPrint logged."""
    logs = [state / "server.log", *sorted((state / "instance" / "logs").glob("*.log"))]
    found: list[str] = []
    for log in logs:
        block: list[str] = []
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("Traceback ") or (block and line.startswith(" ")):
                block.append(line)
            elif block:
                block.append(line)
                if line.startswith(DEFECT):
                    found.append("\n".join(block))
                block = []
    return found


def test_a_bed_exactly_delta_short_of_its_target_heats_without_crashing_the_printer(
    state_dir: Callable[[str], str],
) -> None:
    """The heater simulation survives the state that took the tier's printer offline."""
    state = Path(state_dir("heater"))
    config = state / "instance" / "config.yaml"
    config.parent.mkdir(parents=True)
    config.write_text(
        yaml.safe_dump({"plugins": {"virtual_printer": {"ambientTemperature": AMBIENT_C}}}),
        encoding="utf-8",
    )
    result = script("up", "--state-dir", str(state), "--no-print")
    passing(result, describing="starting an instance whose bed stands at the ambient")
    started = answer(result)
    url = str(started["url"])
    key = key_from(str(started["api_key_file"]))
    equal(_bed(url, key)["actual"], AMBIENT_C, describing="the bed before it is heated")

    equal(
        post(url, "/api/printer/bed", key, {"command": "target", "target": TARGET_C}),
        204,
        describing="setting the bed target exactly `delta` above where it stands",
    )
    deadline = time.monotonic() + HEATING_S
    while time.monotonic() < deadline and _bed(url, key)["actual"] <= AMBIENT_C:
        time.sleep(1.0)

    equal(_raised(state), [], describing=f"tracebacks ending in {DEFECT} in the logs")
    contains(CONNECTED, printer_state(url, key), describing="the printer after heating")
    bed = _bed(url, key)
    equal(bed["target"], TARGET_C, describing="the bed target")
    truth(
        AMBIENT_C < bed["actual"] <= TARGET_C,
        describing=f"a bed heated from {AMBIENT_C} towards {TARGET_C}; it reads {bed['actual']}",
    )
    module = next(
        (state / "venv").glob("**/site-packages/octoprint/plugins/virtual_printer/virtual.py")
    )
    equal(
        module.stat().st_nlink,
        1,
        describing=f"the links to {module}, which the repair replaces rather than edits",
    )
