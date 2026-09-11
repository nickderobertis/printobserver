"""What the artifact journeys are driven against.

Nothing is mocked. The real tool assembles real wheels and real packages from
the committed tree; what stands in for the `printobserver` program is a small
one of this suite's own, because what these journeys are about is the artifact
around it rather than the program inside it.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
from release_artifacts import targets
from repo_checks.model import Repo

REPO_ROOT = Path(__file__).resolve().parents[3]

#: A program every route's artifact carries, in place of the real one. It runs,
#: and it says which version it is, which is what a route's own assertion is.
#: The version is the workspace's own rather than a number written here: release
#: automation moves that one, and a copy kept by hand is stale the first time it
#: does.
STAND_IN = """#!/bin/sh
if [ "${{1:-}}" = "--version" ]; then
    echo "printobserver {version}"
    exit 0
fi
echo "printobserver: a stand-in program, which does nothing" >&2
exit 1
"""


@pytest.fixture
def repo() -> Repo:
    """The committed tree the tool builds from."""
    return Repo(REPO_ROOT)


@pytest.fixture
def version() -> str:
    """The version the workspace declares, which is the one every artifact carries."""
    return targets.workspace(REPO_ROOT)["version"]


@pytest.fixture
def program(tmp_path: Path, version: str) -> Path:
    """A runnable program a route's artifact can carry, reporting the tree's version."""
    path = tmp_path / "printobserver"
    path.write_text(STAND_IN.format(version=version), encoding="utf-8")
    path.chmod(0o755)
    return path


@pytest.fixture
def into(tmp_path: Path) -> Callable[[str], Path]:
    """A factory for directories the tool writes artifacts into."""

    def make(name: str) -> Path:
        made = tmp_path / name
        made.mkdir(parents=True, exist_ok=True)
        return made

    return make
