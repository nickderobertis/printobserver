"""What both end-to-end projects' sessions share: their hooks and their fixtures.

The end-to-end tier is two Nx projects — `repo-e2e`, whose journeys drive what
behaves differently by platform, and `repo-e2e-independent`, whose journeys
drive only this repository's own tooling, workflows and records — and each
project's `conftest.py` takes its hooks and fixtures from here, so the two
sessions are one tier's sessions rather than two that drift apart.

A hook here is made for one project's directory: pytest calls a conftest's
collection hooks with every item of the session, so a session collecting both
directories, as the shard planner's own test does, has each project's conftest
rule on its own files alone.
"""

from __future__ import annotations

import os
from collections import defaultdict
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import REPO_ROOT, GateCopy
from repo_checks import gate_shards, windows_lint, workers

#: How many tests the part's own deselection took out of this session.
LEFT_TO_ANOTHER_PART = pytest.StashKey[int]()

#: How many test files the per-file summary names, slowest first.
SLOWEST_FILES = 25


def worker_count(project: str) -> Callable[[pytest.Config], int]:
    """The `pytest_xdist_auto_num_workers` hook of `project`'s session.

    `repo_checks.workers` reads the host and chooses, for this tier and the
    suites that can run beside it alike; the line it says is the record of
    which figures a run's worker count came from.
    """

    def pytest_xdist_auto_num_workers(config: pytest.Config) -> int:
        del config
        return workers.chosen(workers.Suite(project, gate_shards.END_TO_END), REPO_ROOT)

    return pytest_xdist_auto_num_workers


def _part() -> gate_shards.Shard | None:
    """The part of this tier's test files a sharded gate asked this run to collect."""
    named = os.environ.get(gate_shards.END_TO_END_PART_VARIABLE, "")
    return gate_shards.Shard.named(named) if named else None


def other_parts_deselected(
    directory: Path,
) -> Callable[[pytest.Config, list[pytest.Item]], None]:
    """The `pytest_collection_modifyitems` hook deselecting `directory`'s files another part runs.

    A gate split across runners runs this tier in parts, each named in
    `PRINTOBSERVER_E2E_PART`, and `repo_checks.gate_shards` decides which test
    file each part collects, by its name, so every test is collected by exactly
    one part. Items outside `directory` are another project's, and left to it.
    """

    def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
        part = _part()
        if part is None:
            return
        kept: list[pytest.Item] = []
        dropped: list[pytest.Item] = []
        for item in items:
            ours = item.path.is_relative_to(directory)
            elsewhere = gate_shards.end_to_end_part(item.path.name, part.count) != part.index
            (dropped if ours and elsewhere else kept).append(item)
        config.stash[LEFT_TO_ANOTHER_PART] = config.stash.get(LEFT_TO_ANOTHER_PART, 0) + len(
            dropped
        )
        if dropped:
            config.hook.pytest_deselected(items=dropped)
            items[:] = kept

    return pytest_collection_modifyitems


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


def pytest_terminal_summary(terminalreporter: pytest.TerminalReporter) -> None:
    """Where the session's time went, one line a test file, slowest first.

    `--durations` names single tests; what a cut is chosen by is a file's
    whole share — its setup, calls and teardowns summed over every test it
    holds — so this prints that, for the files that spent the most.
    """
    spent: defaultdict[str, float] = defaultdict(float)
    tests: defaultdict[str, set[str]] = defaultdict(set)
    for reports in terminalreporter.stats.values():
        for report in reports:
            if isinstance(report, pytest.TestReport):
                file = report.nodeid.split("::", 1)[0]
                spent[file] += report.duration
                tests[file].add(report.nodeid)
    if not spent:
        return
    terminalreporter.write_sep("=", f"slowest {SLOWEST_FILES} test files, summed over their tests")
    for file, seconds in sorted(spent.items(), key=lambda one: -one[1])[:SLOWEST_FILES]:
        terminalreporter.write_line(f"{seconds:9.2f}s {len(tests[file]):4d} tests  {file}")
    terminalreporter.write_line(f"{sum(spent.values()):9.2f}s in all, across every worker")


@pytest.fixture(autouse=True)
def no_ambient_host_independence(monkeypatch: pytest.MonkeyPatch) -> None:
    """Drop the opt-out the macOS gate job sets before any test starts.

    That job leaves the steps reading only the committed tree to the Linux
    cells by `PRINTOBSERVER_HOST_INDEPENDENT`, and every test it runs would
    inherit it, skipping the very pass a test drives. A test that means it
    sets it itself.
    """
    monkeypatch.delenv(windows_lint.HOST_INDEPENDENT, raising=False)


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
