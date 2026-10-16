"""How many xdist workers each of this repository's xdist suites runs on, read off the host.

Three suites run under pytest-xdist — the end-to-end tier, `repo-checks` and
`release-artifacts` — and each asks `pytest_xdist_auto_num_workers` what
`-n auto` is. This is the one answer all three take. It starts from the
host's logical processors, bounded by its memory at `MEMORY_PER_WORKER_GIB` a
worker:

- The end-to-end tier is its own Nx invocation, and keeps that whole budget.
- Where the root `justfile` starts it beside the test tier — everywhere but
  Windows, by its `E2E_BESIDE_TESTS` — the test tier's suites run on one worker
  each, as they ran before they were parallelised. The end-to-end tier is the
  longest thing those hosts run, so a worker given to the test tier there is
  taken from it: on the paired runs 38556419344 and 38566907789, giving the two
  suites two workers each lengthened the end-to-end tier on Linux from 631 to
  779 and 534 to 655 seconds. Dividing the budget with the end-to-end tier
  instead left it one worker on the 3-processor macOS runner for the whole of
  its run, and that gate went from 1834 to 2816 seconds (runs 38480620735 and
  38508602716), because xdist cannot grow a suite's workers once it has
  started. These suites wait on the subprocesses they start more than they
  compute, so the two tiers together may hold more workers than processors.
- On Windows the test tier's suites run on one worker each too, though the
  tiers run in turn there. The Rust client journeys in the same invocation
  trace the whole system with the NT Kernel Logger, and with the two suites on
  two and one workers beside them `logman` refused that session ("No more data
  is available") in every Windows x86 run of this change and on the re-run of
  run 38591562924 on Windows aarch64 as well, and in neither before run. Those
  journeys, not these suites, are the longest part of Windows' test tier, so the
  extra workers bought it about a minute.
- Elsewhere, where the tiers run in turn, `repo-checks:test` and
  `release-artifacts:test` divide the budget between them whenever Nx's
  parallelism lets one invocation run both, and what an even share leaves over
  goes to the suite with the most serial work, in `SUITES`' order. No suite is
  given none.

This is allocation arithmetic: it measures no suite's actual peak.

On Windows one processor is left to the host. A worker on every processor of a
4-processor Windows runner keeps the host's own services from answering inside
the deadlines the suites set on them: OctoPrint's bring-down asks Windows which
program a process is through a PowerShell CIM query bounded at 30 seconds.

Free disk is read and reported but bounds nothing: every suite keeps its
temporary directories until the session ends, so what a suite leaves on disk is
the same whichever worker ran which test.

The workers of a suite run under coverage exactly when the suite's own process
does: `pytest_configure_node` tells each worker, and the worker measures itself
into a data file of its own, which `just coverage` combines with every other.
"""

from __future__ import annotations

import ctypes
import json
import os
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Protocol

import coverage
import pytest

from repo_checks.shell import run

#: What one worker's heaviest test — a `cargo` build in a copy of the tree —
#: needs of the host's memory.
MEMORY_PER_WORKER_GIB = 2.0

#: The processors left to a Windows host's own services rather than to a worker.
RESERVED_ON_WINDOWS = 1

GIB = 1024**3


@dataclass(frozen=True, slots=True)
class Suite:
    """One Nx task this repository runs under xdist."""

    project: str
    target: str

    def __str__(self) -> str:
        """The task as Nx names it."""
        return f"{self.project}:{self.target}"


#: The target whose tier `E2E_BESIDE_TESTS` starts beside the test tier.
END_TO_END = "test-e2e"

#: Every suite run under xdist, the one with the most serial work first: the
#: order the processors an even share leaves over are handed out in.
SUITES = (
    Suite("repo-e2e", "test-e2e"),
    Suite("repo-checks", "test"),
    Suite("release-artifacts", "test"),
)


@dataclass(frozen=True, slots=True)
class Host:
    """What the worker counts are chosen from."""

    cpus: int
    memory_gib: float
    free_disk_gib: float
    #: The processors left to the host's own services.
    reserved: int = 0
    #: Whether the test tier traces the whole system beside its suites, as the
    #: Rust client journeys do with the NT Kernel Logger on Windows.
    traced: bool = False

    @property
    def budget(self) -> int:
        """Workers for every suite at once: a processor not left to the host, as memory holds."""
        processors = self.cpus - self.reserved
        return max(1, min(processors, int(self.memory_gib // MEMORY_PER_WORKER_GIB)))


def together(suite: Suite, *, parallelism: int) -> tuple[Suite, ...]:
    """The most suites one Nx invocation can be running while `suite` is, it among them.

    Nx runs at most `parallelism` of one target's tasks at once, so where fewer
    slots than suites are open the ones assumed beside `suite` are those with
    the most work, which is the share that leaves `suite` the least. They come
    in `SUITES`' order.

    Raises:
        ValueError: If `suite` is not one `SUITES` names.
    """
    if suite not in SUITES:
        msg = f"{suite} is not one of the xdist suites {', '.join(map(str, SUITES))}"
        raise ValueError(msg)
    peers = [other for other in SUITES if other.target == suite.target and other != suite]
    running = {suite, *peers[: max(1, parallelism) - 1]}
    return tuple(other for other in SUITES if other in running)


def share(host: Host, suite: Suite, *, parallelism: int, beside: bool) -> int:
    """How many workers `suite` runs on.

    `beside` is whether the end-to-end tier runs beside the test tier on this
    host; there, and on a host whose test tier is traced, the test tier's suites
    take one worker each.
    """
    if suite.target != END_TO_END and (beside or host.traced):
        return 1
    running = together(suite, parallelism=parallelism)
    even, left_over = divmod(host.budget, len(running))
    return max(1, even + (1 if running.index(suite) < left_over else 0))


def said(host: Host, suite: Suite, *, parallelism: int, beside: bool) -> str:
    """The one line a run states its choice in, with the figures it was chosen from."""
    running = together(suite, parallelism=parallelism)
    if suite.target != END_TO_END and beside:
        divided = f"one, the end-to-end tier running beside the {suite.target} tier on this host"
    elif suite.target != END_TO_END and host.traced:
        divided = f"one, the {suite.target} tier tracing the whole system on this host"
    else:
        divided = (
            f"shared with {len(running) - 1} other xdist suites its Nx invocation can run at "
            f"once ({', '.join(str(other) for other in running if other != suite) or 'none'}) "
            f"at Nx parallelism {parallelism}"
        )
    return (
        f"{suite.project}: {share(host, suite, parallelism=parallelism, beside=beside)} xdist "
        f"workers: {host.cpus} logical processors ({host.reserved} left to the host), "
        f"{host.memory_gib:.1f} GiB memory ({MEMORY_PER_WORKER_GIB:g} GiB a worker), "
        f"{divided}; {host.free_disk_gib:.1f} GiB free disk"
    )


def nx_parallelism(root: Path, environ: dict[str, str] | None = None) -> int:
    """How many tasks Nx runs at once in the workspace at `root`.

    `NX_PARALLEL` wins, as it does for Nx itself; otherwise `nx.json`'s own
    `parallel`, which this workspace declares rather than leaving to Nx's
    default, so that the number is written in one place both read.

    Raises:
        ValueError: If neither names a whole number of tasks above none.
    """
    environment = os.environ if environ is None else environ
    named = environment.get("NX_PARALLEL", "").strip()
    if named.isdigit() and int(named) > 0:
        return int(named)
    graph: Any = json.loads((root / "nx.json").read_text(encoding="utf-8"))
    declared = graph.get("parallel") if isinstance(graph, dict) else None
    if isinstance(declared, int) and not isinstance(declared, bool) and declared > 0:
        return declared
    msg = f"{root / 'nx.json'} declares no `parallel` above zero, and NX_PARALLEL names none"
    raise ValueError(msg)


def beside_tests(root: Path) -> bool:
    """Whether the root `justfile` starts the end-to-end tier beside the test tier on this host.

    The justfile's own answer, asked of `just`, so the overlap is stated once.
    Where it cannot answer, the tiers are taken to overlap, which is the answer
    that gives the test tier's suites fewer workers rather than more.
    """
    answered = run(["just", "--evaluate", "E2E_BESIDE_TESTS"], cwd=root)
    return answered.returncode != 0 or answered.stdout.strip() != "false"


def memory_bytes() -> int:
    """This host's physical memory, in bytes."""
    if sys.platform == "win32":
        return _windows_memory_bytes(  # pragma: unreached on linux - only Windows asks Windows
        )  # pragma: unreached on darwin - only Windows asks Windows
    return os.sysconf("SC_PHYS_PAGES") * os.sysconf(  # pragma: unreached on win32 - no sysconf
        "SC_PAGE_SIZE"
    )


def _windows_memory_bytes(  # pragma: unreached on linux - only Windows asks Windows
) -> int:  # pragma: unreached on darwin - only Windows asks Windows
    """Windows' own answer for its installed memory.

    `GetPhysicallyInstalledSystemMemory` writes one unsigned 64-bit count of
    kilobytes through the one pointer it takes, so no structure's layout is
    restated here.

    Raises:
        OSError: On a host that is not Windows, or where Windows refused the query.
    """
    if sys.platform != "win32":
        raise OSError("only Windows answers it")  # pragma: unreached on win32 - Windows does
    kilobytes = ctypes.c_ulonglong()
    if not ctypes.windll.kernel32.GetPhysicallyInstalledSystemMemory(ctypes.byref(kilobytes)):
        raise ctypes.WinError()  # pragma: unreached on win32 - no test can make Windows refuse
    return kilobytes.value * 1024


def here(temporary: Path) -> Host:
    """This host, with the free disk read where the suites' temporary directories go."""
    return Host(
        cpus=os.process_cpu_count() or 1,
        memory_gib=memory_bytes() / GIB,
        free_disk_gib=shutil.disk_usage(temporary).free / GIB,
        reserved=RESERVED_ON_WINDOWS if sys.platform == "win32" else 0,
        traced=sys.platform == "win32",
    )


def chosen(suite: Suite, root: Path) -> int:
    """What `-n auto` means for `suite` on this host, said on standard error before it starts."""
    host = here(Path(tempfile.gettempdir()))
    parallelism = nx_parallelism(root)
    beside = beside_tests(root)
    print(said(host, suite, parallelism=parallelism, beside=beside), file=sys.stderr)
    return share(host, suite, parallelism=parallelism, beside=beside)


#: Where a worker keeps the coverage it measures itself under.
_MEASURING = pytest.StashKey[coverage.Coverage]()


class _Starting(Protocol):
    """The part of xdist's `WorkerController` a worker about to start is told through."""

    workerinput: dict[str, Any]


@pytest.hookimpl(optionalhook=True)
def pytest_configure_node(node: _Starting) -> None:
    """Tell a worker about to start whether the suite's own process runs under coverage."""
    node.workerinput["measured"] = coverage.Coverage.current() is not None


def pytest_configure(config: pytest.Config) -> None:
    """In a worker of a suite run under coverage, measure this worker too."""
    started: dict[str, Any] = getattr(config, "workerinput", {})
    if not started.get("measured") or coverage.Coverage.current() is not None:
        return
    measuring = coverage.Coverage(data_suffix=True)
    measuring.start()
    config.stash[_MEASURING] = measuring


def pytest_unconfigure(config: pytest.Config) -> None:
    """Write what a measured worker measured, beside every other suite's data file."""
    measuring = config.stash.get(_MEASURING, None)
    if measuring is not None:
        measuring.stop()
        measuring.save()
