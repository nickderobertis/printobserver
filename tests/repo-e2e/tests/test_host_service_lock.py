"""A journey holding this host's service manager keeps every other holder waiting.

The service journeys and the script route's proofs run in different xdist
groups, on different workers, and meet only at the host's service manager. What
keeps them apart is the `host_service_manager` fixture's lock, so this holds
the fixture and has a second process — as another worker is — ask for the same
lock, and reads that it is still waiting while the fixture holds it.
"""

from __future__ import annotations

import sys
import time

import pytest
from journey import REPO_ROOT, clean_environment, pythonpath
from journey_session import HOST_SERVICE_MANAGER_LOCK
from repo_checks.expect import equal, truth
from repo_checks.shell import start

#: How long the second holder is given to take the lock it must not get.
WAITING_SECONDS = 3

#: A second holder: it takes the lock exclusively, then says so.
SECOND_HOLDER = (
    "import sys\n"
    "from pathlib import Path\n"
    "from repo_checks.tree_lock import TreeLock\n"
    "held = TreeLock(Path(sys.argv[1]), sys.argv[2])\n"
    "held.lock(exclusive=True)\n"
    "print('taken', flush=True)\n"
)


@pytest.mark.usefixtures("host_service_manager")
def test_a_second_holder_waits_while_a_journey_holds_the_host_service_manager() -> None:
    """The fixture's lock is exclusive, and a holder in another process waits on it."""
    second = start(
        [sys.executable, "-c", SECOND_HOLDER, str(REPO_ROOT), HOST_SERVICE_MANAGER_LOCK],
        cwd=REPO_ROOT,
        env=clean_environment(PYTHONPATH=pythonpath()),
    )
    try:
        time.sleep(WAITING_SECONDS)
        still_waiting = second.poll() is None
    finally:
        second.kill()
        said, _ = second.communicate(timeout=30)

    truth(still_waiting, describing="the second holder to be waiting on the lock")
    equal(said, "", describing="what the second holder said while the journey held the lock")
