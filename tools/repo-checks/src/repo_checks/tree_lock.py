"""The operating system's own lock on a file under a tree's `target` directory.

A tree some suite rewrites while others read it is read and written under one
such lock: shared by every reader, so readers run beside each other, and taken
exclusively by the writer, so no reader sees a file between its truncation and
its last write. The Rust suites take the same lock through `std::fs::File::lock`
and `lock_shared`, so a Python reader and a Rust writer contend for one lock.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import IO

# The lock is the operating system's own, taken the way the Rust holders take
# it through `std::fs::File::lock` and `lock_shared`, so the two sides contend
# for one lock: `flock` where there is one, and on Windows — which has no
# `fcntl` — `LockFileEx` over the whole file, which is exactly the range and the
# flags the Rust standard library asks for there. A lock held by another
# process blocks either call until it goes, and the kernel releases either when
# the handle does, so a copy that is killed leaves nothing held.
if (  # pragma: unreached on linux - only a Windows host locks through its kernel32
    sys.platform == "win32"
):  # pragma: unreached on darwin - only a Windows host locks through its kernel32
    import ctypes
    import msvcrt
    from ctypes import wintypes

    class _Overlapped(ctypes.Structure):
        """`OVERLAPPED`, zeroed: the lock starts at offset 0."""

        _fields_ = (
            ("Internal", ctypes.c_void_p),
            ("InternalHigh", ctypes.c_void_p),
            ("Offset", wintypes.DWORD),
            ("OffsetHigh", wintypes.DWORD),
            ("hEvent", wintypes.HANDLE),
        )

    _LOCKFILE_EXCLUSIVE_LOCK = 0x00000002
    _WHOLE_FILE = 0xFFFFFFFF
    _kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _kernel32.LockFileEx.argtypes = (
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.DWORD,
        ctypes.POINTER(_Overlapped),
    )
    _kernel32.LockFileEx.restype = wintypes.BOOL
    _kernel32.UnlockFileEx.argtypes = (
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.DWORD,
        ctypes.POINTER(_Overlapped),
    )
    _kernel32.UnlockFileEx.restype = wintypes.BOOL

    def _take(file: IO[str], *, exclusive: bool) -> None:
        handle = msvcrt.get_osfhandle(file.fileno())
        flags = _LOCKFILE_EXCLUSIVE_LOCK if exclusive else 0
        taken = _kernel32.LockFileEx(
            handle, flags, 0, _WHOLE_FILE, _WHOLE_FILE, ctypes.byref(_Overlapped())
        )
        if not taken:
            raise ctypes.WinError(ctypes.get_last_error())

    def _let_go(file: IO[str]) -> None:
        handle = msvcrt.get_osfhandle(file.fileno())
        released = _kernel32.UnlockFileEx(
            handle, 0, _WHOLE_FILE, _WHOLE_FILE, ctypes.byref(_Overlapped())
        )
        if not released:
            raise ctypes.WinError(ctypes.get_last_error())

else:  # pragma: unreached on win32 - Windows has no `fcntl`
    import fcntl

    def _take(file: IO[str], *, exclusive: bool) -> None:
        fcntl.flock(file.fileno(), fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH)

    def _let_go(file: IO[str]) -> None:
        fcntl.flock(file.fileno(), fcntl.LOCK_UN)


class TreeLock:
    """One lock file under a tree's `target`, which is per worktree."""

    def __init__(self, root: Path, name: str) -> None:
        """Open the lock file `name` under `root`'s `target`, making that directory."""
        directory = root / "target"
        directory.mkdir(parents=True, exist_ok=True)
        self.file: IO[str] = (directory / name).open("a", encoding="utf-8")

    def lock(self, *, exclusive: bool = False) -> None:
        """Take the lock, shared unless asked for exclusively."""
        _take(self.file, exclusive=exclusive)

    def release(self) -> None:
        """Let it go, and close the file it was taken on."""
        _let_go(self.file)
        self.file.close()
