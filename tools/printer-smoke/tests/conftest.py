"""Fixtures that hand a test a real smoke run against a printer it controls."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from world import World, a_serial_device, a_world, build_the_program


@pytest.fixture(scope="session")
def program() -> Path:
    """The `printobserver` command the smoke drives, built from this workspace."""
    return build_the_program()


@pytest.fixture
def world(tmp_path: Path, program: Path) -> Iterator[World]:
    """A machine this test controls, on a serial device this test created."""
    with a_serial_device() as device:
        made = a_world(tmp_path, device=device)
        try:
            yield made
        finally:
            made.substitute.stop()
