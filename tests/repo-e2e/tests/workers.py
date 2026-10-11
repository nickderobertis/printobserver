"""How many xdist workers the end-to-end tier runs on, read off the host it runs on.

The tier's `test-e2e` target runs `pytest -n auto`, and xdist asks
`pytest_xdist_auto_num_workers` what `auto` is. The answer is this host's own
logical processors, bounded by its memory: a journey's heaviest moment is a
`cargo` build in a copy of the tree, and one of those per worker is what the
memory has to hold.

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

GIB = 1024**3


@dataclass(frozen=True, slots=True)
class Host:
    """What the worker count is chosen from."""

    cpus: int
    memory_gib: float
    free_disk_gib: float

    @property
    def workers(self) -> int:
        """One worker per logical processor, as many as the memory holds, and never none."""
        return max(1, min(self.cpus, int(self.memory_gib // MEMORY_PER_WORKER_GIB)))

    def said(self) -> str:
        """The one line a run states its choice in, with the figures it was chosen from."""
        return (
            f"repo-e2e: {self.workers} xdist workers: {self.cpus} logical processors, "
            f"{self.memory_gib:.1f} GiB memory ({MEMORY_PER_WORKER_GIB:g} GiB a worker), "
            f"{self.free_disk_gib:.1f} GiB free disk"
        )


def memory_bytes() -> int:
    """This host's physical memory, in bytes."""
    if sys.platform == "win32":
        return _windows_memory_bytes()
    return os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE")


def _windows_memory_bytes() -> int:
    """Windows' own answer for its physical memory, from `GlobalMemoryStatusEx`.

    Raises:
        OSError: On a host that is not Windows, or where Windows refused the query.
    """
    if sys.platform != "win32":
        message = "only Windows answers GlobalMemoryStatusEx"
        raise OSError(message)

    class MemoryStatus(ctypes.Structure):
        _fields_ = (
            ("dwLength", ctypes.c_ulong),
            ("dwMemoryLoad", ctypes.c_ulong),
            ("ullTotalPhys", ctypes.c_ulonglong),
            ("ullAvailPhys", ctypes.c_ulonglong),
            ("ullTotalPageFile", ctypes.c_ulonglong),
            ("ullAvailPageFile", ctypes.c_ulonglong),
            ("ullTotalVirtual", ctypes.c_ulonglong),
            ("ullAvailVirtual", ctypes.c_ulonglong),
            ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
        )

    status = MemoryStatus()
    status.dwLength = ctypes.sizeof(MemoryStatus)
    if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
        raise ctypes.WinError()
    return int(status.ullTotalPhys)


def here(temporary: Path) -> Host:
    """This host, with the free disk read where the tier's temporary directories go."""
    return Host(
        cpus=os.process_cpu_count() or 1,
        memory_gib=memory_bytes() / GIB,
        free_disk_gib=shutil.disk_usage(temporary).free / GIB,
    )
