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
from repo_checks.model import Repo

REPO_ROOT = Path(__file__).resolve().parents[3]

#: A program every route's artifact carries, in place of the real one. It runs,
#: and it says which version it is, which is what a route's own assertion is.
STAND_IN = """#!/bin/sh
if [ "${1:-}" = "--version" ]; then
    echo "printobserver 0.1.0"
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
def program(tmp_path: Path) -> Path:
    """A runnable program a route's artifact can carry."""
    path = tmp_path / "printobserver"
    path.write_text(STAND_IN, encoding="utf-8")
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
