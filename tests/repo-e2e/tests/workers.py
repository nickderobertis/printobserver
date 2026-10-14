"""How many xdist workers the end-to-end tier runs on, read off the host it runs on.

The tier's `test-e2e` target runs `pytest -n auto`, and xdist asks
`pytest_xdist_auto_num_workers` what `auto` is. The answer is this host's own
logical processors, bounded by its memory: a journey's heaviest moment is a
`cargo` build in a copy of the tree, and one of those per worker is what the
memory has to hold.

On Windows one processor is left to the host. A worker on every processor of a
4-processor Windows runner keeps the host's own services from answering inside
the deadlines the journeys set on them: OctoPrint's bring-down asks Windows
which program a process is through a PowerShell CIM query bounded at 30
seconds.

Free disk is read and reported but bounds nothing: every journey keeps its
temporary directory until the session ends, so what the tier leaves on disk is
the same whichever worker ran which journey, and a worker adds only its own
environment to it.
"""

from __future__ import annotations

import ctypes
import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

#: What one worker's heaviest journey — a `cargo` build in a copy of the tree —
#: needs of the host's memory.
MEMORY_PER_WORKER_GIB = 2.0

#: The processors left to a Windows host's own services rather than to a worker.
RESERVED_ON_WINDOWS = 1

GIB = 1024**3


@dataclass(frozen=True, slots=True)
class Host:
    """What the worker count is chosen from."""

    cpus: int
    memory_gib: float
    free_disk_gib: float
    #: The processors left to the host's own services.
    reserved: int = 0

    @property
    def workers(self) -> int:
        """A worker per processor not left to the host, as many as the memory holds, never none."""
        processors = self.cpus - self.reserved
        return max(1, min(processors, int(self.memory_gib // MEMORY_PER_WORKER_GIB)))

    def said(self) -> str:
        """The one line a run states its choice in, with the figures it was chosen from."""
        return (
            f"repo-e2e: {self.workers} xdist workers: {self.cpus} logical processors "
            f"({self.reserved} left to the host), {self.memory_gib:.1f} GiB memory "
            f"({MEMORY_PER_WORKER_GIB:g} GiB a worker), {self.free_disk_gib:.1f} GiB free disk"
        )


def memory_bytes() -> int:
    """This host's physical memory, in bytes."""
    if sys.platform == "win32":
        return _windows_memory_bytes()
    return os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE")


def _windows_memory_bytes() -> int:
    """Windows' own answer for its installed memory.

    `GetPhysicallyInstalledSystemMemory` writes one unsigned 64-bit count of
    kilobytes through the one pointer it takes, so no structure's layout is
    restated here.

    Raises:
        OSError: On a host that is not Windows, or where Windows refused the query.
    """
    if sys.platform != "win32":
        message = "only Windows answers GetPhysicallyInstalledSystemMemory"
        raise OSError(message)
    kilobytes = ctypes.c_ulonglong()
    if not ctypes.windll.kernel32.GetPhysicallyInstalledSystemMemory(ctypes.byref(kilobytes)):
        raise ctypes.WinError()
    return kilobytes.value * 1024


def here(temporary: Path) -> Host:
    """This host, with the free disk read where the tier's temporary directories go."""
    return Host(
        cpus=os.process_cpu_count() or 1,
        memory_gib=memory_bytes() / GIB,
        free_disk_gib=shutil.disk_usage(temporary).free / GIB,
        reserved=RESERVED_ON_WINDOWS if sys.platform == "win32" else 0,
    )
