"""A real copy of the committed tree a check can be pointed at.

Every test here drives the committed check functions against a real copy of the
committed tree, with one defect introduced. Nothing is mocked: the check reads
files, and the files are the ones this repository ships.
"""

from __future__ import annotations

import shutil
import sys
from collections.abc import Iterable
from pathlib import Path
from typing import IO

from repo_checks.model import Repo
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

# The lock is the operating system's own, taken the way the Rust holders take
# it through `std::fs::File::lock` and `lock_shared`, so the two sides contend
# for one lock: `flock` where there is one, and on Windows — which has no
# `fcntl` — `LockFileEx` over the whole file, which is exactly the range and the
# flags the Rust standard library asks for there. A lock held by another
# process blocks either call until it goes, and the kernel releases either when
# the handle does, so a copy that is killed leaves nothing held.
if sys.platform == "win32":
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

else:
    import fcntl

    def _take(file: IO[str], *, exclusive: bool) -> None:
        fcntl.flock(file.fileno(), fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH)

    def _let_go(file: IO[str]) -> None:
        fcntl.flock(file.fileno(), fcntl.LOCK_UN)


#: The lock the checked-in schema tree is read and written under, named by
#: `repo-policy.toml`'s `supervisor.schema_lock`. One journey in the adapter's
#: suite rewrites a checked-in schema on disk while every other suite runs
#: beside it under `nx run-many`, and this suite copies that tree file by file:
#: a copy taken between that journey's truncate and its write carries an empty
#: schema, and a check over an empty schema finds nothing to refuse.
SCHEMA_LOCK = "printobserver-schemas.lock"


class SchemaTreeLock:
    """The lock a copy of the tree is taken under.

    Shared by default, so copies run beside each other and beside every suite
    that only reads the schema tree, and only the journey that changes it waits
    for them. Exclusive is what that journey takes, and what a test standing in
    for it takes here.
    """

    def __init__(self) -> None:
        """Open the one lock file, under `target`, which is per worktree."""
        directory = REPO_ROOT / "target"
        directory.mkdir(parents=True, exist_ok=True)
        self.file: IO[str] = (directory / SCHEMA_LOCK).open("a", encoding="utf-8")

    def lock(self, *, exclusive: bool = False) -> None:
        """Take the lock, shared unless asked for exclusively."""
        _take(self.file, exclusive=exclusive)

    def release(self) -> None:
        """Let it go, and close the file it was taken on."""
        _let_go(self.file)
        self.file.close()


def tracked_files(root: Path) -> list[str]:
    """Every file a clone would carry once this change lands.

    `--others --exclude-standard` includes files this change has added but not
    yet committed, and excludes everything `.gitignore` covers. Reading only the
    index would copy a tree missing exactly the files the change is about.
    """
    listing = run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root,
        check=True,
    ).stdout
    return [name for name in listing.split("\0") if name]


def copy_tree(destination: Path, *, omit: Iterable[str] = ()) -> Path:
    """Copy the committed tree, symlinks and all, into a fresh directory."""
    omitted = tuple(omit)
    destination.mkdir(parents=True, exist_ok=True)
    held = SchemaTreeLock()
    held.lock()
    try:
        for name in tracked_files(REPO_ROOT):
            if any(name.startswith(prefix) for prefix in omitted):
                continue
            source = REPO_ROOT / name
            if not source.exists() and not source.is_symlink():
                # A file the index still lists and the working tree no longer
                # has: a deletion nobody has staged yet. What a clone would
                # carry is what this copies, and a clone would not carry it.
                continue
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            if source.is_symlink():
                target.symlink_to(source.readlink())
            else:
                shutil.copy2(source, target)
    finally:
        held.release()
    return destination


class Tree:
    """A copy of the committed tree a test may break in exactly one way."""

    def __init__(self, root: Path) -> None:
        """Bind to a copied tree."""
        self.root = root

    @property
    def repo(self) -> Repo:
        """The checks' view of this tree."""
        return Repo(self.root)

    def read(self, relative: str) -> str:
        """Read a file of this tree."""
        return (self.root / relative).read_text(encoding="utf-8")

    def write(self, relative: str, text: str) -> None:
        """Replace a file of this tree."""
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def append(self, relative: str, text: str) -> None:
        """Add to a file of this tree."""
        self.write(relative, self.read(relative) + text)

    def edit(self, relative: str, old: str, new: str) -> None:
        """Replace one exact fragment of one file, refusing a no-op edit."""
        text = self.read(relative)
        if old not in text:
            msg = f"{relative} does not contain {old!r}; the fixture is stale"
            raise AssertionError(msg)
        self.write(relative, text.replace(old, new, 1))

    def remove(self, relative: str) -> None:
        """Delete a path of this tree."""
        path = self.root / relative
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        else:
            path.unlink()
