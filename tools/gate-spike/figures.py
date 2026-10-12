"""What one task's output says it cost, read into typed figures.

Three producers write what this reads: cargo and nextest (their own log lines),
`spike_pytest.py` (one JSON line per test and per pytest process) and the
journeys' timing sink and the timed linker (one JSON line per operation). A
line that is not the shape its producer writes is skipped and counted, so a
report says how much it could not read rather than reading it as zero.
"""

from __future__ import annotations

import json
import re
import statistics
from collections import Counter, defaultdict
from dataclasses import dataclass
from enum import StrEnum
from typing import TYPE_CHECKING

from spike_names import (
    Cost,
    JourneyTiming,
    LinkTiming,
    Outcome,
    PytestSession,
    PytestTest,
    RecordError,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

CARGO_FINISHED = re.compile(r"Finished `[^`]+` profile .* target\(s\) in (?:(\d+)m )?([\d.]+)s")
NEXTEST_STARTING = re.compile(r"Starting (\d+) tests? across (\d+) binar")
NEXTEST_SUMMARY = re.compile(r"Summary \[\s*([\d.]+)s\]")
#: A terminal colour sequence, which cargo and nextest write when the job asks
#: for colour (`CARGO_TERM_COLOR=always` on GitHub's runners).
COLOUR = re.compile(r"\x1b\[[0-9;]*m")


@dataclass(frozen=True)
class Lines[Record]:
    """The records a file held, and how many of its lines were not one."""

    records: list[Record]
    unreadable: int


def read_lines[Record](path: Path, parse: Callable[[dict[str, object]], Record]) -> Lines[Record]:
    """Every record in a JSON-lines file, read with `parse`."""
    if not path.is_file():
        return Lines([], 0)
    records: list[Record] = []
    unreadable = 0
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            document = json.loads(line)
            if not isinstance(document, dict):
                raise RecordError(line)
            records.append(parse(document))
        except json.JSONDecodeError, RecordError:
            unreadable += 1
    return Lines(records, unreadable)


class NextestStatus(StrEnum):
    """How nextest reported a test ended."""

    PASSED = "PASS"
    FAILED = "FAIL"
    FLAKY = "FLAKY"
    TIMEOUT = "TIMEOUT"
    SIGSEGV = "SIGSEGV"
    SIGABRT = "SIGABRT"
    SIGKILL = "SIGKILL"
    ABORT = "ABORT"
    LEAK = "LEAK"


#: One test nextest finished: its status, seconds, binary and name.
NEXTEST_RESULT = re.compile(
    r"^\s*(?:\S+:\s+)?(" + "|".join(NextestStatus) + r")\s+\[\s*([\d.]+)s\]\s+"
    r"(?:\(\s*\d+/\d+\)\s+)?(\S+)\s+(.+?)\s*$"
)


@dataclass(frozen=True)
class RustTest:
    """One test nextest ran, and how long it took."""

    binary: str
    test: str
    s: float
    status: NextestStatus


@dataclass(frozen=True)
class RustFigures:
    """A nextest task: cargo's compile time, nextest's run time and every test."""

    compile_s: float
    run_s: float | None
    tests: int
    binaries: int | None
    fastest_test_s: float | None
    median_test_s: float | None
    per_test: list[RustTest]


def rust_figures(log: str) -> RustFigures | None:
    """Read a nextest task's log, or nothing when nextest did not run."""
    log = COLOUR.sub("", log)
    if "nextest" not in log and "Summary [" not in log:
        return None
    compiled = [
        int(found.group(1) or 0) * 60 + float(found.group(2))
        for found in CARGO_FINISHED.finditer(log)
    ]
    starting = NEXTEST_STARTING.search(log)
    summary = NEXTEST_SUMMARY.findall(log)
    tests = [
        RustTest(
            binary=found.group(3),
            test=found.group(4),
            s=float(found.group(2)),
            status=NextestStatus(found.group(1)),
        )
        for found in map(NEXTEST_RESULT.match, log.splitlines())
        if found
    ]
    passed = [test.s for test in tests if test.status is NextestStatus.PASSED]
    return RustFigures(
        compile_s=round(sum(compiled), 3),
        run_s=float(summary[-1]) if summary else None,
        tests=int(starting.group(1)) if starting else len(tests),
        binaries=int(starting.group(2)) if starting else None,
        fastest_test_s=min(passed) if passed else None,
        median_test_s=statistics.median(passed) if passed else None,
        per_test=tests,
    )


@dataclass(frozen=True)
class PytestFigures:
    """A pytest task: every test and process `spike_pytest.py` recorded."""

    tests: int
    test_s: float
    outcomes: dict[str, int]
    failed: list[str]
    by_cost: dict[str, Cost]
    sessions: list[PytestSession]
    per_test: list[PytestTest]
    unreadable: int


def _pytest_record(document: dict[str, object]) -> PytestTest | PytestSession:
    """A session line carries `"session": true`; a test line carries no `session` at all."""
    match document.get("session", False):
        case True:
            return PytestSession.parse(document)
        case False if "session" not in document:
            return PytestTest.parse(document)
        case marker:
            msg = f"`session` is {marker!r}, neither true nor absent, in {document!r}"
            raise RecordError(msg)


def pytest_figures(directory: Path) -> PytestFigures | None:
    """Read every record the plugin wrote into a task's directory, or nothing when it wrote none."""
    files = sorted(directory.glob("pytest-*.jsonl"))
    if not files:
        return None
    read = [read_lines(path, _pytest_record) for path in files]
    records = [record for lines in read for record in lines.records]
    tests = [record for record in records if isinstance(record, PytestTest)]
    costs: dict[str, Cost] = defaultdict(Cost)
    for test in tests:
        for name, cost in test.costs.items():
            costs[name].count += cost.count
            costs[name].s += cost.s
    return PytestFigures(
        tests=len(tests),
        test_s=round(sum(test.s for test in tests), 3),
        outcomes=dict(sorted(Counter(str(test.outcome) for test in tests).items())),
        failed=[test.nodeid for test in tests if test.outcome is Outcome.FAILED],
        by_cost={name: Cost(cost.count, round(cost.s, 3)) for name, cost in sorted(costs.items())},
        sessions=[record for record in records if isinstance(record, PytestSession)],
        per_test=tests,
        unreadable=sum(lines.unreadable for lines in read),
    )


@dataclass(frozen=True)
class JourneyFigures:
    """The client-command journeys' operations, totalled by kind, in seconds."""

    operations: int
    by_kind: dict[str, Cost]
    unreadable: int


def journey_figures(path: Path) -> JourneyFigures | None:
    """Total what the journeys' timing sink wrote, or nothing when it wrote nothing."""
    lines = read_lines(path, JourneyTiming.parse)
    if not lines.records and not lines.unreadable:
        return None
    by_kind: dict[str, Cost] = defaultdict(Cost)
    for timing in lines.records:
        by_kind[timing.kind].add(timing.ms / 1000)
    return JourneyFigures(
        operations=len(lines.records),
        by_kind={
            kind: Cost(cost.count, round(cost.s, 3)) for kind, cost in sorted(by_kind.items())
        },
        unreadable=lines.unreadable,
    )


@dataclass(frozen=True)
class LinkFigures:
    """Every link the timed linker made during an experiment."""

    count: int
    s: float
    per_output: list[LinkTiming]
    unreadable: int


def link_figures(path: Path) -> LinkFigures | None:
    """Total what the timed linker wrote, or nothing when it linked nothing."""
    lines = read_lines(path, LinkTiming.parse)
    if not lines.records and not lines.unreadable:
        return None
    return LinkFigures(
        count=len(lines.records),
        s=round(sum(link.ms for link in lines.records) / 1000, 3),
        per_output=lines.records,
        unreadable=lines.unreadable,
    )
