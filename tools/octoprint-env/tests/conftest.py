"""Fixtures that hand a journey real state directories and take them down again.

Every state directory a journey asks for is stopped when the journey ends,
whether it passed or not, so a failing test leaves no OctoPrint behind on the
machine that ran it.
"""

from __future__ import annotations

from collections.abc import Callable, Iterator
from pathlib import Path

import pytest
from environment import script


@pytest.fixture
def state_dir(tmp_path: Path) -> Iterator[Callable[[str], str]]:
    """A factory for state directories, each stopped when the journey ends."""
    handed: list[Path] = []

    def make(name: str) -> str:
        path = tmp_path / name
        handed.append(path)
        return str(path)

    yield make

    for path in handed:
        if (path / "instance.json").is_file():
            script("down", "--state-dir", str(path), timeout=120)
