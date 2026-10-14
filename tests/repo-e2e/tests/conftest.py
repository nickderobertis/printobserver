"""Fixtures that hand a journey a real copy of the committed tree."""

from __future__ import annotations

import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

import pytest
import workers
from journey import GateCopy


@pytest.hookimpl(optionalhook=True)
def pytest_xdist_auto_num_workers(config: pytest.Config) -> int:
    """What `-n auto` means for this tier on this host, said once before it starts.

    `workers` reads the host and chooses; the line it says is the record of
    which figures a run's worker count came from.
    """
    del config
    host = workers.here(Path(tempfile.gettempdir()))
    print(host.said(), file=sys.stderr)
    return host.workers


@pytest.fixture(autouse=True)
def no_ambient_gate_base(monkeypatch: pytest.MonkeyPatch) -> None:
    """Drop the base and head the gate job exports before any test starts.

    `nx-set-shas` exports `NX_BASE` and `NX_HEAD` into the job, so every test of
    a gate cell inherits commits of *that* checkout — which none of the trees a
    test builds contains, and which the tier rightly refuses as naming no
    commit. A test that means a base names it itself.
    """
    monkeypatch.delenv("NX_BASE", raising=False)
    monkeypatch.delenv("NX_HEAD", raising=False)


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
