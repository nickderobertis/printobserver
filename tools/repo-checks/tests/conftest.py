"""Fixtures that hand a check a real tree on disk to read.

Every test here drives the committed check functions against a real copy of the
committed tree, with one defect introduced. Nothing is mocked: the check reads
files, and the files are the ones this repository ships.
"""

from __future__ import annotations

import shutil
import subprocess
from collections.abc import Callable, Iterable
from pathlib import Path

import pytest
from repo_checks.model import Repo

REPO_ROOT = Path(__file__).resolve().parents[3]


def tracked_files(root: Path) -> list[str]:
    """Every file git tracks, which is exactly what a clone would carry."""
    listing = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=root,
        capture_output=True,
        text=True,
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


@pytest.fixture
def committed() -> Repo:
    """The committed tree, exactly as this repository leaves it."""
    return Repo(REPO_ROOT)


@pytest.fixture
def tree(tmp_path: Path) -> Callable[[], Tree]:
    """A factory for fresh copies of the committed tree."""
    counter = {"n": 0}

    def make() -> Tree:
        counter["n"] += 1
        return Tree(copy_tree(tmp_path / f"tree{counter['n']}"))

    return make
