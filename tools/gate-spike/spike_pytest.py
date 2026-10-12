"""Spike-only pytest plugin: what each test spent its time on.

Loaded by `measure.py` with `-p spike_pytest`, with `SPIKE_PYTEST_OUT` naming a
directory. Each pytest process (an xdist worker included) appends one JSON line
per test to a file of its own there — the test's phases and outcome, and the
seconds it spent in child processes (by program), in `shutil.copy2`,
`shutil.copytree` and `shutil.rmtree`, in `time.sleep`, and in
`urllib.request.urlopen` — and one line for the process itself. A copy
`copytree` makes is counted under `copytree` alone: it binds `copy2` as a
default argument when the module is imported, before this wraps it.

It changes no test, assertion or timeout: every wrapper calls the real function
and only records how long it took. With `SPIKE_STABLE_IDS` set, each
parametrized case is named by its index within its function instead of its
parameters' rendering, so xdist workers that render a generated parameter
differently still collect one list.
"""

from __future__ import annotations

import functools
import json
import os
import shlex
import shutil
import subprocess
import threading
import time
import urllib.request
from collections import defaultdict
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest
from spike_names import (
    PHASES,
    PYTEST_OUT,
    STABLE_IDS,
    WORKER_COVERAGE,
    Cost,
    Outcome,
    PytestSession,
    PytestTest,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    import coverage

OUTSIDE_A_TEST = "<outside a test>"


@dataclass
class Phases:
    """A test's phase durations and its outcome, as pytest reported them."""

    seconds: dict[str, float] = field(default_factory=dict)
    outcome: Outcome = Outcome.UNKNOWN


@dataclass
class Child:
    """A child process this plugin is timing."""

    started: float
    program: str


@dataclass
class State:
    """Everything this plugin keeps for one pytest process."""

    sink: Path | None = None
    current: str | None = None
    controller: bool = False
    started: float = 0.0
    collect_started: float = 0.0
    collect_s: float = 0.0
    measured: coverage.Coverage | None = None
    patches: pytest.MonkeyPatch = field(default_factory=pytest.MonkeyPatch)
    costs: dict[str, dict[str, Cost]] = field(
        default_factory=lambda: defaultdict(lambda: defaultdict(Cost))
    )
    phases: dict[str, Phases] = field(default_factory=lambda: defaultdict(Phases))
    children: dict[int, Child] = field(default_factory=dict)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def charge(self, category: str, seconds: float) -> None:
        """Add one occurrence of `category` lasting `seconds` to the running test."""
        with self.lock:
            self.costs[self.current or OUTSIDE_A_TEST][category].add(seconds)

    def costs_of(self, key: str) -> dict[str, Cost]:
        """The costs charged to `key`, forgotten once read."""
        with self.lock:
            return dict(self.costs.pop(key, {}))


STATE = State()


def _program(args: object) -> str:
    """The bare program name a `Popen` was asked to run, without its suffix."""
    match args:
        case str() | bytes() | os.PathLike():
            words = shlex.split(os.fsdecode(args), posix=os.name != "nt")
            first = words[0] if words else ""
        case [str() | bytes() | os.PathLike() as head, *_]:
            first = os.fsdecode(head)
        case _:
            first = ""
    name = Path(first.strip('"')).name.lower()
    for suffix in (".exe", ".cmd", ".bat"):
        name = name.removesuffix(suffix)
    return name or "<unnamed>"


def _ended(process: subprocess.Popen[str] | subprocess.Popen[bytes]) -> None:
    """Charge a child process the first time it is seen to have ended."""
    if process.returncode is None:
        return
    child = STATE.children.pop(id(process), None)
    if child is not None:
        STATE.charge(f"spawn:{child.program}", time.perf_counter() - child.started)


def _wrap_popen() -> None:
    """Time every child process from its start until it is first seen ended."""
    # Popen's overloads are its callers' to satisfy; this forwards whatever they passed.
    original_init = cast("Callable[..., None]", subprocess.Popen.__init__)
    original_wait = subprocess.Popen.wait
    original_poll = subprocess.Popen.poll

    def init(self: subprocess.Popen[str], *args: object, **kwargs: object) -> None:
        STATE.children[id(self)] = Child(
            time.perf_counter(), _program(kwargs.get("args", args[0] if args else None))
        )
        original_init(self, *args, **kwargs)

    def wait(self: subprocess.Popen[str], timeout: float | None = None) -> int:
        answer = original_wait(self, timeout)
        _ended(self)
        return answer

    def poll(self: subprocess.Popen[str]) -> int | None:
        answer = original_poll(self)
        _ended(self)
        return answer

    STATE.patches.setattr(subprocess.Popen, "__init__", init)
    STATE.patches.setattr(subprocess.Popen, "wait", wait)
    STATE.patches.setattr(subprocess.Popen, "poll", poll)


def _timed(category: str, function: Callable[..., object]) -> Callable[..., object]:
    """`function`, charging how long each call took to `category`."""

    @functools.wraps(function)
    def timed(*args: object, **kwargs: object) -> object:
        started = time.perf_counter()
        try:
            return function(*args, **kwargs)
        finally:
            STATE.charge(category, time.perf_counter() - started)

    return timed


def _write(record: dict[str, object]) -> None:
    if STATE.sink is not None:
        with STATE.sink.open("a", encoding="utf-8") as sink:
            sink.write(json.dumps(record) + "\n")


def pytest_configure(config: pytest.Config) -> None:
    """Install the wrappers and name this process's own output file."""
    out = os.environ.get(PYTEST_OUT)
    if not out:
        return
    worker = os.environ.get("PYTEST_XDIST_WORKER")
    # An xdist controller hears every worker's reports again: its lines would
    # count each test twice, so it writes only its own session line.
    STATE.controller = worker is None and bool(getattr(config.option, "numprocesses", None))
    directory = Path(out)
    directory.mkdir(parents=True, exist_ok=True)
    STATE.sink = directory / f"pytest-{worker or 'main'}-{os.getpid()}.jsonl"
    STATE.started = time.perf_counter()
    _wrap_popen()
    for owner, name, category in (
        (shutil, "copy2", "copy2"),
        (shutil, "copytree", "copytree"),
        (shutil, "rmtree", "rmtree"),
        (time, "sleep", "sleep"),
        (urllib.request, "urlopen", "http"),
    ):
        STATE.patches.setattr(owner, name, _timed(category, getattr(owner, name)))
    if os.environ.get(WORKER_COVERAGE) and worker is not None:
        # Each worker under the project's own coverage configuration, which
        # `pyproject.toml` declares; the controller runs no test to measure.
        import coverage

        STATE.measured = coverage.Coverage(data_suffix=True)
        STATE.measured.start()


def pytest_collection_modifyitems(items: list[pytest.Item]) -> None:
    """Under `SPIKE_STABLE_IDS`, name each parametrized case by its index alone."""
    if not os.environ.get(STABLE_IDS):
        return
    seen: dict[str, int] = defaultdict(int)
    for item in items:
        base, bracket, _ = item.nodeid.partition("[")
        if bracket:
            item._nodeid = f"{base}[{seen[base]}]"
            seen[base] += 1


def pytest_unconfigure(config: pytest.Config) -> None:
    """Save a measured worker's coverage and put every wrapped function back."""
    del config
    if STATE.measured is not None:
        STATE.measured.stop()
        STATE.measured.save()
    STATE.patches.undo()


def pytest_sessionstart(session: pytest.Session) -> None:
    """Note when collection, which is mostly importing test modules, began."""
    del session
    STATE.collect_started = time.perf_counter()


def pytest_collection_finish(session: pytest.Session) -> None:
    """Note how long collection took."""
    del session
    STATE.collect_s = time.perf_counter() - STATE.collect_started


def pytest_runtest_logstart(nodeid: str, location: object) -> None:
    """Charge everything from here on to this test."""
    del location
    STATE.current = nodeid


def pytest_runtest_logreport(report: pytest.TestReport) -> None:
    """Keep each phase's duration, and the outcome that decides the test's."""
    phases = STATE.phases[report.nodeid]
    phases.seconds[str(report.when)] = report.duration
    if report.failed or phases.outcome is Outcome.UNKNOWN or report.when == "call":
        phases.outcome = Outcome(report.outcome)


def pytest_runtest_logfinish(nodeid: str, location: object) -> None:
    """Write the test's line and stop charging to it."""
    del location
    STATE.current = None
    phases = STATE.phases.pop(nodeid, Phases())
    costs = STATE.costs_of(nodeid)
    if STATE.controller:
        return
    record = PytestTest(
        nodeid=nodeid,
        worker=os.environ.get("PYTEST_XDIST_WORKER", "main"),
        outcome=phases.outcome,
        s=round(sum(phases.seconds.get(phase, 0.0) for phase in PHASES), 3),
        phases={
            phase: round(phases.seconds[phase], 3) for phase in PHASES if phase in phases.seconds
        },
        costs=costs,
    )
    _write(asdict(record))


def pytest_sessionfinish(session: pytest.Session, exitstatus: int) -> None:
    """Write the process's own line: its wall time, collection and exit status."""
    record = PytestSession(
        worker=os.environ.get("PYTEST_XDIST_WORKER", "main"),
        wall_s=round(time.perf_counter() - STATE.started, 3),
        collect_s=round(STATE.collect_s, 3),
        exitstatus=int(exitstatus),
        tests=session.testscollected,
        outside_tests=STATE.costs_of(OUTSIDE_A_TEST),
    )
    _write({"session": True, **asdict(record)})
