"""A real copy of the committed tree a check can be pointed at.

Every test here drives the committed check functions against a real copy of the
committed tree, with one defect introduced. Nothing is mocked: the check reads
files, and the files are the ones this repository ships.
"""

from __future__ import annotations

import shutil
from collections.abc import Iterable
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]


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
    for name in tracked_files(REPO_ROOT):
        if any(name.startswith(prefix) for prefix in omitted):
            continue
        source = REPO_ROOT / name
        if not source.exists() and not source.is_symlink():
            # A file the index still lists and the working tree no longer has:
            # a deletion nobody has staged yet. What a clone would carry is
            # what this copies, and a clone would not carry it.
            continue
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            target.symlink_to(source.readlink())
        else:
            shutil.copy2(source, target)
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
