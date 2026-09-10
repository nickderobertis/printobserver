"""The recipes that bracket the printer integration tier, run for real.

`just octoprint-up` starts a real OctoPrint under a copy of the committed tree,
`just test-integration` runs the tier against what it started, and `just
octoprint-down` stops it. The assertion at the end is the one that matters on a
machine that runs this repeatedly: nothing the bring-up recipe started is still
there afterwards.
"""

from __future__ import annotations

import json
import os
from collections.abc import Callable

from journey import GateCopy
from repo_checks.expect import passing, truth

STATE = ".octoprint-env"


def running(pid: int) -> bool:
    """Whether a process the bring-up recipe started is still there."""
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


# journey: the-printer-tier-runs-between-its-bracketing-recipes
def test_the_tier_runs_between_the_bring_up_and_bring_down_recipes(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Up, the tier, down — and no process left behind."""
    copy = gate_copy()

    brought_up = copy.just("octoprint-up", timeout=1200)

    passing(brought_up, describing="`just octoprint-up`")
    record = json.loads(copy.read(f"{STATE}/instance.json"))
    pid = int(record["pid"])
    truth(running(pid), describing=f"the process {pid} the bring-up recipe started")

    tier = copy.just("test-integration", timeout=2400)
    brought_down = copy.just("octoprint-down", timeout=600)

    passing(tier, describing="`just test-integration` against what the bring-up started")
    passing(brought_down, describing="`just octoprint-down`")
    truth(
        not running(pid),
        describing=f"process {pid} to be gone once the bring-down recipe has run",
    )
