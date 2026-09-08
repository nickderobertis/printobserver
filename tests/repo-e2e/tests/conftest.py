"""Fixtures that hand a journey a real copy of the committed tree."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
from journey import GateCopy


@pytest.fixture(scope="session")
def shared_venv(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """One environment every gate copy resolves into, so a copy costs a copy."""
    return tmp_path_factory.mktemp("shared-venv")


@pytest.fixture
def gate_copy(tmp_path: Path, shared_venv: Path) -> Callable[..., GateCopy]:
    """A factory for copies of the committed tree the gate can run over."""
    counter = {"n": 0}

    def make(*, node_modules: bool = True) -> GateCopy:
        counter["n"] += 1
        return GateCopy(tmp_path / f"copy{counter['n']}", shared_venv, node_modules=node_modules)

    return make
