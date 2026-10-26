"""A real copy of the committed tree a check can be pointed at.

Every test here drives the committed check functions against a real copy of the
committed tree, with one defect introduced. Nothing is mocked: the check reads
files, and the files are the ones this repository ships.

A copy is of the files its check reads — the check's `Closure` — rather than of
all of them, because copying a thousand files to change one is what this suite
spent most of its time on: about 750 whole copies a run, which on a Windows
runner was three quarters of the suite. A closure that leaves out a file its
check opens is caught rather than trusted: every open under a closure copy is
watched, and a read of a committed file the copy does not carry fails the test
that made it, naming the file.
"""

from __future__ import annotations

import functools
import os
import shutil
import sys
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from repo_checks.model import Repo
from repo_checks.shell import run
from repo_checks.tree_lock import TreeLock

REPO_ROOT = Path(__file__).resolve().parents[3]

#: The lock the checked-in schema tree is read and written under, named by
#: `repo-policy.toml`'s `supervisor.schema_lock`. One journey in the adapter's
#: suite rewrites a checked-in schema on disk while every other suite runs
#: beside it under `nx run-many`, and this suite copies that tree file by file:
#: a copy taken between that journey's truncate and its write carries an empty
#: schema, and a check over an empty schema finds nothing to refuse.
SCHEMA_LOCK = "printobserver-schemas.lock"


class SchemaTreeLock(TreeLock):
    """The lock a copy of the tree is taken under.

    Shared by default, so copies run beside each other and beside every suite
    that only reads the schema tree, and only the journey that changes it waits
    for them. Exclusive is what that journey takes, and what a test standing in
    for it takes here.
    """

    def __init__(self, root: Path = REPO_ROOT) -> None:
        """Open the one lock file, under the tree's `target`, which is per worktree."""
        super().__init__(root, SCHEMA_LOCK)


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


@functools.cache
def listed(root: Path) -> tuple[str, ...]:
    """`tracked_files`, asked of git once per process for each tree.

    The listing is the same for every copy a session takes, and asking git for
    it once a copy was a tenth of this suite's time on a Windows runner. What a
    file holds is read when it is copied; only which files there are is kept.
    """
    return tuple(tracked_files(root))


@functools.cache
def _named(root: Path) -> frozenset[str]:
    """The same listing, as one set every closure copy of the tree looks names up in."""
    return frozenset(listed(root))


@dataclass(frozen=True, slots=True)
class Closure:
    """The files of the tree a check reads, as repository-relative patterns.

    A pattern ending in `/` is a directory and everything under it; one carrying
    `*`, `?` or `[` is a glob matched against the whole path, where `**` spans
    any number of directories, the tree's root included; anything else is one
    file.
    """

    patterns: tuple[str, ...]

    def carries(self, name: str) -> bool:
        """Whether a copy of this closure carries the file at `name`."""
        return any(_matches(name, pattern) for pattern in self.patterns)

    def carries_any(self, names: Iterable[str]) -> bool:
        """Whether a copy of this closure carries any of `names`."""
        return any(self.carries(name) for name in names)

    def __or__(self, other: Closure) -> Closure:
        """Both closures' files, for a test that drives more than one check."""
        return Closure(tuple(dict.fromkeys((*self.patterns, *other.patterns))))


def _matches(name: str, pattern: str) -> bool:
    """Whether one repository-relative file falls under one closure pattern."""
    if pattern.endswith("/"):
        return name.startswith(pattern)
    if any(wildcard in pattern for wildcard in "*?["):
        return PurePosixPath(name).full_match(pattern)
    return name == pattern


@dataclass(frozen=True, slots=True)
class _Watched:
    """A closure copy: the files it was cut from, and the ones it carries."""

    closure: Closure
    source: frozenset[str]


#: Every closure copy this process has made, by its root as an absolute path
#: ending in a separator; the read below is on every `open` in the process, so
#: it matches a path by a prefix of a string rather than by building one.
_WATCHED: dict[str, _Watched] = {}
_ROOTS: tuple[str, ...] = ()

#: Each committed file a test read under a closure copy that does not carry it.
_STRAYED: list[str] = []


def _reads(mode: object, flags: object) -> bool:
    """Whether an `open` event opens its file for reading.

    `open()` reports its mode as a string; `os.open` reports none, and its flags
    say instead.
    """
    if isinstance(mode, str):
        return "r" in mode or "+" in mode
    return isinstance(flags, int) and flags & (os.O_WRONLY | os.O_RDWR) != os.O_WRONLY


def _watch(event: str, arguments: tuple[object, ...]) -> None:
    """Record a read under a closure copy of a committed file the copy does not carry."""
    if event != "open" or not _ROOTS:
        return
    path, mode, flags = arguments
    if not isinstance(path, (str, bytes, os.PathLike)) or not _reads(mode, flags):
        return
    opened = os.fsdecode(path)
    if not opened.startswith(_ROOTS):
        return
    # The newest copy first: it is almost always the one the running test reads.
    for root in reversed(_WATCHED):
        if opened.startswith(root):
            watched = _WATCHED[root]
            name = opened.removeprefix(root).replace(os.sep, "/")
            if name in watched.source and not watched.closure.carries(name):
                _STRAYED.append(name)
            return


sys.addaudithook(_watch)


def _watched_under(destination: Path, watched: _Watched) -> None:
    """Watch reads under a closure copy, by both of the names its root goes by.

    The name it was made under and the one `Repo` resolves it to differ where a
    temporary directory is reached through a link, as on macOS, or through a
    short name, as on a Windows runner.
    """
    global _ROOTS
    for root in {destination.absolute(), destination.resolve()}:
        _WATCHED[f"{root}{os.sep}"] = watched
    _ROOTS = tuple(_WATCHED)


def strayed() -> list[str]:
    """Every read outside its copy's closure since this was last asked, and forget them."""
    found = list(dict.fromkeys(_STRAYED))
    _STRAYED.clear()
    return found


def copy_tree(
    destination: Path,
    *,
    omit: Iterable[str] = (),
    closure: Closure | None = None,
    source: Path = REPO_ROOT,
) -> Path:
    """Copy the committed tree, symlinks and all, into a fresh directory.

    With a `closure`, only the files it carries are copied, and every read under
    the copy is watched for a committed file it left out.
    """
    omitted = tuple(omit)
    names = listed(source)
    destination.mkdir(parents=True, exist_ok=True)
    if closure is not None:
        watched = _Watched(closure, _named(source))
        _watched_under(destination, watched)
    held = SchemaTreeLock(source)
    held.lock()
    try:
        for name in names:
            if any(name.startswith(prefix) for prefix in omitted):
                continue
            if closure is not None and not closure.carries(name):
                continue
            original = source / name
            if not original.exists() and not original.is_symlink():
                # A file the index still lists and the working tree no longer
                # has: a deletion nobody has staged yet. What a clone would
                # carry is what this copies, and a clone would not carry it.
                continue
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            if original.is_symlink():
                target.symlink_to(original.readlink())
            else:
                shutil.copy2(original, target)
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
