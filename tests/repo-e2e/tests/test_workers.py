"""The worker count the tier runs on is this host's, bounded by what it can hold."""

from __future__ import annotations

import os
import shutil
import sys
import tempfile
from pathlib import Path

import pytest
import workers
from repo_checks.expect import contains, equal, truth


@pytest.mark.parametrize(
    ("cpus", "memory_gib", "reserved", "chosen"),
    [
        (4, 16.0, 0, 4),  # the hosted Linux runners: a worker per processor
        (4, 16.0, 1, 3),  # the hosted Windows runners: one processor left to the host
        (3, 7.0, 0, 3),  # the hosted macOS runner: its memory holds all three
        (20, 94.0, 0, 20),
        (8, 5.0, 0, 2),  # memory bounds it below the processors
        (4, 1.0, 0, 1),  # and never to none
        (1, 16.0, 1, 1),  # nor does a processor left to the host
    ],
)
def test_a_worker_per_processor_as_many_as_the_memory_holds(
    cpus: int, memory_gib: float, reserved: int, chosen: int
) -> None:
    """The count is the smallest of the bounds, and at least one."""
    host = workers.Host(cpus=cpus, memory_gib=memory_gib, free_disk_gib=50.0, reserved=reserved)

    equal(host.workers, chosen, describing=f"the workers for {cpus} CPUs and {memory_gib} GiB")
    contains(
        host.said(),
        f"repo-e2e: {chosen} xdist workers: {cpus} logical processors "
        f"({reserved} left to the host)",
    )


def test_this_host_is_read_from_its_own_answers() -> None:
    """Processors, memory and disk are this host's own, whichever host this is."""
    temporary = Path(tempfile.gettempdir())

    host = workers.here(temporary)

    equal(host.cpus, os.process_cpu_count(), describing="the processors read")
    equal(
        host.reserved,
        workers.RESERVED_ON_WINDOWS if sys.platform == "win32" else 0,
        describing="the processors left to this host",
    )
    truth(host.memory_gib > 0.5, describing=f"{host.memory_gib} GiB of memory read")
    free = shutil.disk_usage(temporary).free / workers.GIB
    truth(abs(host.free_disk_gib - free) < 1.0, describing=f"{host.free_disk_gib} GiB free read")
