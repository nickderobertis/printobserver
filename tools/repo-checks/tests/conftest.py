"""Fixtures that hand a check a real tree on disk to read.

Every test here drives the committed check functions against a real copy of the
committed tree, with one defect introduced. Nothing is mocked: the check reads
files, and the files are the ones this repository ships.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.model import Repo
from treecopy import REPO_ROOT, Tree, copy_tree


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
