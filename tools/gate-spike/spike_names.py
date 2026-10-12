"""The names the spike's harness and the producers it reads agree on.

One source for every environment variable that switches a producer on and every
field of the records they write, so `measure.py`, `spike_pytest.py` and the
tests read the same spelling. The two compiled producers — the journeys' timing
sink (`crates/printobserver/tests/support/spike.rs`) and the timed linker
(`timed_linker.rs`) — are held to these names by `tests/test_records.py`, which
runs each one and reads what it wrote with the parsers below.
"""

from __future__ import annotations

from dataclasses import dataclass

#: Names the directory each pytest process writes its records into.
PYTEST_OUT = "SPIKE_PYTEST_OUT"
#: Set, each xdist worker measures its own coverage, as pytest-cov does.
WORKER_COVERAGE = "SPIKE_WORKER_COVERAGE"
#: Set, each parametrized case is named by its index within its function.
STABLE_IDS = "SPIKE_STABLE_IDS"
#: Names the file the client-command journeys append their timings to.
JOURNEY_TIMINGS = "PRINTOBSERVER_SPIKE_TIMINGS"
#: Names the linker the timed linker stands in front of.
REAL_LINKER = "SPIKE_REAL_LINKER"
#: Names the file the timed linker appends one line per link to.
LINK_TIMINGS = "SPIKE_LINK_TIMINGS"


class RecordError(ValueError):
    """A record line that is not the shape its producer writes."""


def _number(record: dict[str, object], key: str) -> float:
    value = record.get(key)
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        msg = f"`{key}` is not a number in {record!r}"
        raise RecordError(msg)
    return float(value)


def _text(record: dict[str, object], key: str) -> str:
    value = record.get(key)
    if not isinstance(value, str):
        msg = f"`{key}` is not a string in {record!r}"
        raise RecordError(msg)
    return value


@dataclass(frozen=True)
class JourneyTiming:
    """One operation a client-command journey timed."""

    kind: str
    ms: float
    pid: int
    thread: str

    @classmethod
    def parse(cls, record: dict[str, object]) -> JourneyTiming:
        """Read one line `spike.rs` wrote."""
        return cls(
            kind=_text(record, "kind"),
            ms=_number(record, "ms"),
            pid=int(_number(record, "pid")),
            thread=_text(record, "thread"),
        )


@dataclass(frozen=True)
class LinkTiming:
    """One link the timed linker made."""

    linker: str
    output: str
    ms: float

    @classmethod
    def parse(cls, record: dict[str, object]) -> LinkTiming:
        """Read one line `timed_linker.rs` wrote."""
        return cls(
            linker=_text(record, "linker"),
            output=_text(record, "output"),
            ms=_number(record, "ms"),
        )


@dataclass
class Cost:
    """How many times one kind of thing happened during a test, and for how long."""

    count: int = 0
    s: float = 0.0

    def add(self, seconds: float) -> None:
        """Count one more occurrence lasting `seconds`."""
        self.count += 1
        self.s += seconds

    @classmethod
    def parse(cls, record: object) -> Cost:
        """Read one cost a pytest record carries."""
        if not isinstance(record, dict):
            msg = f"a cost is not an object: {record!r}"
            raise RecordError(msg)
        return cls(count=int(_number(record, "count")), s=_number(record, "s"))


def _costs(record: dict[str, object], key: str) -> dict[str, Cost]:
    value = record.get(key)
    if not isinstance(value, dict):
        msg = f"`{key}` is not an object in {record!r}"
        raise RecordError(msg)
    return {str(name): Cost.parse(cost) for name, cost in value.items()}


@dataclass(frozen=True)
class PytestTest:
    """One test, as `spike_pytest.py` recorded it."""

    nodeid: str
    worker: str
    outcome: str
    s: float
    phases: dict[str, float]
    costs: dict[str, Cost]

    @classmethod
    def parse(cls, record: dict[str, object]) -> PytestTest:
        """Read one test line."""
        phases = record.get("phases")
        if not isinstance(phases, dict):
            msg = f"`phases` is not an object in {record!r}"
            raise RecordError(msg)
        return cls(
            nodeid=_text(record, "nodeid"),
            worker=_text(record, "worker"),
            outcome=_text(record, "outcome"),
            s=_number(record, "s"),
            phases={str(phase): _number(phases, phase) for phase in phases},
            costs=_costs(record, "costs"),
        )


@dataclass(frozen=True)
class PytestSession:
    """One pytest process, as `spike_pytest.py` recorded it."""

    worker: str
    wall_s: float
    collect_s: float
    exitstatus: int
    tests: int
    outside_tests: dict[str, Cost]

    @classmethod
    def parse(cls, record: dict[str, object]) -> PytestSession:
        """Read one session line."""
        return cls(
            worker=_text(record, "worker"),
            wall_s=_number(record, "wall_s"),
            collect_s=_number(record, "collect_s"),
            exitstatus=int(_number(record, "exitstatus")),
            tests=int(_number(record, "tests")),
            outside_tests=_costs(record, "outside_tests"),
        )
