"""A real copy of the committed tree the generator can be run over.

Nothing here is mocked. The generator reads the schemas this repository ships,
runs the three formatters the gate runs, and writes files; every test drives
that over a copy of the tree so the committed one is never written to.
"""

from __future__ import annotations

import shutil
from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: The JavaScript dependencies the Node client's own formatter lives in. A copy
#: carries a link to the committed tree's rather than a second install: the
#: generator runs the formatter the gate will hold the file to, and that is the
#: version the locked install put there.
NODE_MODULES = "node_modules"


def _tracked(root: Path) -> list[str]:
    """Every file a clone would carry once this change lands."""
    listing = run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root,
        check=True,
    ).stdout
    return [name for name in listing.split("\0") if name]


def copy_tree(destination: Path) -> Path:
    """Copy the committed tree into a fresh directory the generator may write to."""
    destination.mkdir(parents=True, exist_ok=True)
    for name in _tracked(REPO_ROOT):
        source = REPO_ROOT / name
        if not source.exists() and not source.is_symlink():
            # A file the index still lists and the working tree no longer has:
            # a deletion nobody has staged yet. The copy is of what a clone
            # would carry, and a clone would not carry it.
            continue
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            target.symlink_to(source.readlink())
        else:
            shutil.copy2(source, target)
    (destination / NODE_MODULES).symlink_to(REPO_ROOT / NODE_MODULES)
    return destination


@pytest.fixture
def scratch(tmp_path: Path) -> Callable[[], Path]:
    """A factory for fresh copies of the committed tree."""
    counter = {"n": 0}

    def make() -> Path:
        counter["n"] += 1
        return copy_tree(tmp_path / f"tree{counter['n']}")

    return make
