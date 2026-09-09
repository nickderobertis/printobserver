"""Fixtures that hand a journey the one state directory, and take it down again.

Every live journey shares one state directory, because the alternative is a
second clone of Obico's sources and a second build of its images per journey.
Whatever a journey leaves running is stopped when the session ends, so a failing
run leaves no stack behind on the machine that ran it.
"""

from __future__ import annotations

import os
from collections.abc import Iterator
from pathlib import Path

import pytest
from harness import environment

DEFAULT_STATE_DIR = ".obico-env"


@pytest.fixture(scope="session")
def state_dir() -> Iterator[str]:
    """The state directory every live journey shares, stopped when the session ends."""
    where = os.environ.get("OBICO_ENV_STATE_DIR", DEFAULT_STATE_DIR)
    yield where
    if (Path(where) / "compose-override.yml").is_file():
        environment("down", "--state-dir", where, timeout=1800)
