"""The worker count the tier runs on is its share of this host, bounded by what it can hold."""

from __future__ import annotations

import os
import shutil
import sys
import tempfile
from pathlib import Path

import pytest
from repo_checks import workers
from repo_checks.expect import contains, equal, truth

TIER = workers.Suite("repo-e2e", "test-e2e")


@pytest.mark.parametrize(
    ("cpus", "memory_gib", "reserved", "beside", "chosen"),
    [
        (4, 16.0, 0, True, 2),  # the hosted Linux runners: beside the test tier's two suites
        (4, 16.0, 1, False, 3),  # the hosted Windows runners: alone, one processor left over
        (3, 7.0, 0, True, 1),  # the hosted macOS runner: a processor for each of three suites
        (20, 94.0, 0, True, 7),
        (8, 5.0, 0, False, 2),  # memory bounds it below the processors
        (4, 1.0, 0, False, 1),  # and never to none
        (1, 16.0, 1, False, 1),  # nor does a processor left to the host
    ],
)
def test_the_tier_takes_its_share_of_the_processors_the_memory_holds(
    cpus: int, memory_gib: float, reserved: int, beside: bool, chosen: int
) -> None:
    """The count is the tier's share of the smallest bound, and at least one."""
    host = workers.Host(cpus=cpus, memory_gib=memory_gib, free_disk_gib=50.0, reserved=reserved)

    equal(
        workers.share(host, TIER, parallelism=3, beside=beside),
        chosen,
        describing=f"the tier's workers for {cpus} CPUs and {memory_gib} GiB",
    )
    contains(
        workers.said(host, TIER, parallelism=3, beside=beside),
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
