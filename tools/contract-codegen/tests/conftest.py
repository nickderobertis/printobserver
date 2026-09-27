"""A real copy of the committed tree the generator can be run over.

Nothing here is mocked. The generator reads the schemas this repository ships,
runs the three formatters the gate runs, and writes files; every test drives
that over a copy of the tree so the committed one is never written to.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
from release_artifacts import bumping
from repo_checks import scratch as scratch_trees

REPO_ROOT = Path(__file__).resolve().parents[3]


def copy_tree(destination: Path) -> Path:
    """Copy the committed tree into a fresh directory the generator may write to.

    The copy carries a link to the committed tree's JavaScript dependencies
    rather than a second install: the generator runs the formatter the gate
    will hold the file to, and that is the version the locked install put there.
    """
    return scratch_trees.copy_tree(REPO_ROOT, destination)


@pytest.fixture
def scratch(tmp_path: Path) -> Callable[[], Path]:
    """A factory for fresh copies of the committed tree."""
    counter = {"n": 0}

    def make() -> Path:
        counter["n"] += 1
        return copy_tree(tmp_path / f"tree{counter['n']}")

    return make


@pytest.fixture
def bumped(scratch: Callable[[], Path]) -> tuple[Path, str]:
    """A fresh copy whose workspace version was moved as a release pull request moves it.

    Answered beside the version it now declares, which no committed file names.
    """
    copy = scratch()
    version = bumping.next_minor(bumping.workspace_version(copy))
    bumping.release_plz_bump(copy, version)
    return copy, version
