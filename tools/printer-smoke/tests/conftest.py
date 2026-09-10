"""Fixtures that hand a test a real smoke run against a printer it controls."""

from __future__ import annotations

import os
from collections.abc import Iterator
from pathlib import Path

import pytest
from world import World, a_world, build_the_program, open_a_serial_device


@pytest.fixture(scope="session")
def program() -> Path:
    """The `printobserver` command the smoke drives, built from this workspace."""
    return build_the_program()


@pytest.fixture
def world(tmp_path: Path, program: Path) -> Iterator[World]:
    """A machine this test controls, on a serial device this test created."""
    device, controller, handle = open_a_serial_device()
    made = a_world(tmp_path, device=device)
    try:
        yield made
    finally:
        made.substitute.stop()
        os.close(controller)
        os.close(handle)
