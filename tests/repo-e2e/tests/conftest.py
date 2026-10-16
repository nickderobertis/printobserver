"""Fixtures that hand a journey a real copy of the committed tree."""

from __future__ import annotations

import os
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import REPO_ROOT, GateCopy
from repo_checks import gate_shards, workers


@pytest.hookimpl(optionalhook=True)
def pytest_xdist_auto_num_workers(config: pytest.Config) -> int:
    """What `-n auto` means for this tier on this host, said once before it starts.

    `repo_checks.workers` reads the host and chooses, for this tier and the two
    suites that can run beside it alike; the line it says is the record of
    which figures a run's worker count came from.
    """
    del config
    return workers.chosen(workers.Suite("repo-e2e", "test-e2e"), REPO_ROOT)


#: How many tests the part's own deselection took out of this session.
LEFT_TO_ANOTHER_PART = pytest.StashKey[int]()


def _part() -> gate_shards.Shard | None:
    """The part of this tier's test files a sharded gate asked this run to collect."""
    named = os.environ.get(gate_shards.END_TO_END_PART_VARIABLE, "")
    return gate_shards.Shard.named(named) if named else None


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Deselect every test another part of a split tier collects.

    A gate split across runners runs this tier in parts, each named in
    `PRINTOBSERVER_E2E_PART`, and `repo_checks.gate_shards` decides which test
    file each part collects, so every test is collected by exactly one part.
    """
    part = _part()
    if part is None:
        return
    kept: list[pytest.Item] = []
    dropped: list[pytest.Item] = []
    for item in items:
        file_part = gate_shards.end_to_end_part(item.path.name, part.count)
        (kept if file_part == part.index else dropped).append(item)
    config.stash[LEFT_TO_ANOTHER_PART] = len(dropped)
    if dropped:
        config.hook.pytest_deselected(items=dropped)
        items[:] = kept


def pytest_sessionfinish(session: pytest.Session, exitstatus: int) -> None:
    """A part whose share of the selection is empty ran what it was asked to.

    The tier's second invocation selects the `alone` journeys, which one part's
    files carry and the other's do not, so that other part leaves every test
    the marker selected to the part that runs it. That is its share of the
    selection rather than a selection matching nothing; a part that collected
    nothing at all is still the failure it was.
    """
    if (
        exitstatus == pytest.ExitCode.NO_TESTS_COLLECTED
        and session.config.stash.get(LEFT_TO_ANOTHER_PART, 0) > 0
    ):
        session.exitstatus = pytest.ExitCode.OK


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
    # The part of this tier a sharded gate collects is read at collection, and
    # a tier a journey drives in a copy is never one of its parts.
    for name in gate_shards.PARTS:
        monkeypatch.delenv(name, raising=False)


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
