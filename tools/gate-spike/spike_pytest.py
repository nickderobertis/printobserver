"""Spike-only pytest plugin: what each test spent its time on.

Loaded by `measure.py` with `-p spike_pytest` and `SPIKE_PYTEST_OUT` naming a
directory. Each pytest process (an xdist worker included) appends one JSON line
per test to a file of its own there: the test's phases and outcome, and the
seconds it spent in child processes (by program), in `shutil.copytree` and
`shutil.rmtree`, in `time.sleep`, and in `urllib.request.urlopen`. It changes no
test, assertion or timeout: every wrapper calls the real function and only
records how long it took.
"""

from __future__ import annotations

import functools
import json
import os
import shutil
import subprocess
import threading
import time
import urllib.request
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Any

import pytest

if TYPE_CHECKING:
    from collections.abc import Callable

#: Where costs charged outside any test are kept.
OUTSIDE = "<outside a test>"


@dataclass
class State:
    """Everything this plugin keeps for one pytest process."""

    sink: Path | None = None
    current: str | None = None
    controller: bool = False
    started: float = 0.0
    collect_started: float = 0.0
    collect_s: float = 0.0
    coverage: Any = None
    patches: pytest.MonkeyPatch = field(default_factory=pytest.MonkeyPatch)
    costs: dict[str, dict[str, list[float]]] = field(
        default_factory=lambda: defaultdict(lambda: defaultdict(lambda: [0.0, 0.0]))
    )
    phases: dict[str, dict[str, Any]] = field(default_factory=lambda: defaultdict(dict))
    children: dict[int, tuple[float, str]] = field(default_factory=dict)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def charge(self, category: str, seconds: float) -> None:
        """Add one occurrence of `category` lasting `seconds` to the running test."""
        with self.lock:
            cost = self.costs[self.current or OUTSIDE][category]
            cost[0] += 1
            cost[1] += seconds

    def costs_of(self, key: str) -> dict[str, dict[str, float]]:
        """The costs charged to `key`, forgotten once read."""
        with self.lock:
            charged = self.costs.pop(key, {})
        return {name: {"count": int(c), "s": round(s, 3)} for name, (c, s) in charged.items()}


STATE = State()


def _program(args: object) -> str:
    """The bare program name a `Popen` was asked to run."""
    if isinstance(args, (str, bytes, os.PathLike)):
        words = os.fsdecode(args).split()
        first = words[0] if words else ""
    elif isinstance(args, (list, tuple)):
        first = os.fsdecode(args[0]) if args else ""
    else:
        first = ""
    name = Path(first.strip('"')).name.lower()
    for suffix in (".exe", ".cmd", ".bat"):
        name = name.removesuffix(suffix)
    return name or "<unnamed>"


def _ended(process: subprocess.Popen[Any]) -> None:
    """Charge a child process the first time it is seen to have ended."""
    if process.returncode is None:
        return
    started = STATE.children.pop(id(process), None)
    if started is not None:
        STATE.charge(f"spawn:{started[1]}", time.perf_counter() - started[0])


def _wrap_popen() -> None:
    """Time every child process from its start until it is first seen ended."""
    original_init = subprocess.Popen.__init__
    original_wait = subprocess.Popen.wait
    original_poll = subprocess.Popen.poll

    def init(self: subprocess.Popen[Any], args: object, *rest: object, **kwargs: object) -> None:
        STATE.children[id(self)] = (time.perf_counter(), _program(args))
        original_init(self, args, *rest, **kwargs)

    def wait(self: subprocess.Popen[Any], timeout: float | None = None) -> int:
        answer = original_wait(self, timeout)
        _ended(self)
        return answer

    def poll(self: subprocess.Popen[Any]) -> int | None:
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


def pytest_configure(config: pytest.Config) -> None:
    """Install the wrappers and name this process's own output file."""
    out = os.environ.get("SPIKE_PYTEST_OUT")
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
        (shutil, "copytree", "copytree"),
        (shutil, "rmtree", "rmtree"),
        (time, "sleep", "sleep"),
        (urllib.request, "urlopen", "http"),
    ):
        STATE.patches.setattr(owner, name, _timed(category, getattr(owner, name)))
    if os.environ.get("SPIKE_WORKER_COVERAGE") and worker is not None:
        # Each xdist worker measured on its own, as pytest-cov measures them;
        # the configuration is the project's own, read from `pyproject.toml`.
        import coverage

        STATE.coverage = coverage.Coverage(data_suffix=True)
        STATE.coverage.start()


def pytest_unconfigure(config: pytest.Config) -> None:
    """Save a measured worker's coverage and put every wrapped function back."""
    del config
    if STATE.coverage is not None:
        STATE.coverage.stop()
        STATE.coverage.save()
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
    """Keep each phase's duration and the test's outcome."""
    phases = STATE.phases[report.nodeid]
    phases[report.when] = report.duration
    if report.failed or "outcome" not in phases or report.when == "call":
        phases["outcome"] = report.outcome


def _write(line: dict[str, Any]) -> None:
    if STATE.sink is not None:
        with STATE.sink.open("a", encoding="utf-8") as sink:
            sink.write(json.dumps(line) + "\n")


def pytest_runtest_logfinish(nodeid: str, location: object) -> None:
    """Write the test's line and stop charging to it."""
    del location
    STATE.current = None
    phases = STATE.phases.pop(nodeid, {})
    costs = STATE.costs_of(nodeid)
    if STATE.controller:
        return
    when = ("setup", "call", "teardown")
    _write(
        {
            "nodeid": nodeid,
            "worker": os.environ.get("PYTEST_XDIST_WORKER", "main"),
            "outcome": phases.get("outcome", "unknown"),
            "s": round(sum(phases.get(phase, 0.0) for phase in when), 3),
            "phases": {phase: round(phases[phase], 3) for phase in when if phase in phases},
            "costs": costs,
        }
    )


def pytest_sessionfinish(session: pytest.Session, exitstatus: int) -> None:
    """Write the process's own line: its wall time, collection and exit status."""
    _write(
        {
            "session": True,
            "worker": os.environ.get("PYTEST_XDIST_WORKER", "main"),
            "wall_s": round(time.perf_counter() - STATE.started, 3),
            "collect_s": round(STATE.collect_s, 3),
            "exitstatus": int(exitstatus),
            "tests": session.testscollected,
            "outside_tests": STATE.costs_of(OUTSIDE),
        }
    )
