"""How many xdist workers each of this repository's xdist suites runs on, read off the host.

Three suites run under pytest-xdist — the end-to-end tier, `repo-checks` and
`release-artifacts` — and each asks `pytest_xdist_auto_num_workers` what
`-n auto` is. This is the one answer all three take. It is the host's logical
processors, bounded by its memory at `MEMORY_PER_WORKER_GIB` a worker, divided
among the xdist suites that can run at the same moment:

- Nx runs one target's tasks together, as many at once as its parallelism, so
  `repo-checks:test` and `release-artifacts:test` share the host whenever the
  parallelism is at least two;
- and the root `justfile` starts the end-to-end tier beside the test tier
  wherever its `E2E_BESIDE_TESTS` says so, which is everywhere but Windows.

What an even share leaves over goes to the suites with the most serial work,
in `SUITES`' order, and no suite is given none. This is allocation arithmetic:
it keeps the workers of the suites that can run together within the host's
processors and memory, and it measures no suite's actual peak.

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

#: How many tasks Nx runs at once when `nx.json` and the environment say nothing.
NX_DEFAULT_PARALLELISM = 3

GIB = 1024**3


@dataclass(frozen=True, slots=True)
class Suite:
    """One Nx task this repository runs under xdist."""

    project: str
    target: str

    def __str__(self) -> str:
        """The task as Nx names it."""
        return f"{self.project}:{self.target}"


#: Every suite run under xdist, the one with the most serial work first: the
#: order the processors an even share leaves over are handed out in.
SUITES = (
    Suite("repo-e2e", "test-e2e"),
    Suite("repo-checks", "test"),
    Suite("release-artifacts", "test"),
)

#: The target whose tier `E2E_BESIDE_TESTS` starts beside the test tier.
END_TO_END = "test-e2e"


@dataclass(frozen=True, slots=True)
class Host:
    """What the worker counts are chosen from."""

    cpus: int
    memory_gib: float
    free_disk_gib: float
    #: The processors left to the host's own services.
    reserved: int = 0

    @property
    def budget(self) -> int:
        """Workers for every suite at once: a processor not left to the host, as memory holds."""
        processors = self.cpus - self.reserved
        return max(1, min(processors, int(self.memory_gib // MEMORY_PER_WORKER_GIB)))


def together(suite: Suite, *, parallelism: int, beside: bool) -> tuple[Suite, ...]:
    """The most suites that can be running while `suite` is, it among them, in `SUITES`' order.

    Nx runs at most `parallelism` of one target's tasks at once, so where fewer
    slots than suites are open the ones assumed beside `suite` are those with
    the most work, which is the share that leaves `suite` the least.

    Raises:
        ValueError: If `suite` is not one `SUITES` names.
    """
    if suite not in SUITES:
        msg = f"{suite} is not one of the xdist suites {', '.join(map(str, SUITES))}"
        raise ValueError(msg)
    slots = max(1, parallelism)
    running = {suite}
    targets = {suite.target}
    if beside and suite.target in {END_TO_END, "test"}:
        targets |= {END_TO_END, "test"}
    for target in targets:
        peers = [other for other in SUITES if other.target == target and other != suite]
        room = slots - (1 if target == suite.target else 0)
        running.update(peers[:room])
    return tuple(other for other in SUITES if other in running)


def share(host: Host, suite: Suite, *, parallelism: int, beside: bool) -> int:
    """How many workers `suite` runs on: its share of the host among the suites beside it."""
    running = together(suite, parallelism=parallelism, beside=beside)
    even, left_over = divmod(host.budget, len(running))
    return max(1, even + (1 if running.index(suite) < left_over else 0))


def said(host: Host, suite: Suite, *, parallelism: int, beside: bool) -> str:
    """The one line a run states its choice in, with the figures it was chosen from."""
    running = together(suite, parallelism=parallelism, beside=beside)
    return (
        f"{suite.project}: {share(host, suite, parallelism=parallelism, beside=beside)} xdist "
        f"workers: {host.cpus} logical processors ({host.reserved} left to the host), "
        f"{host.memory_gib:.1f} GiB memory ({MEMORY_PER_WORKER_GIB:g} GiB a worker), shared "
        f"with {len(running) - 1} other xdist suites that can run at once "
        f"({', '.join(str(other) for other in running if other != suite) or 'none'}) at Nx "
        f"parallelism {parallelism}; {host.free_disk_gib:.1f} GiB free disk"
    )


def nx_parallelism(root: Path, environ: dict[str, str] | None = None) -> int:
    """How many tasks Nx runs at once in the workspace at `root`.

    `NX_PARALLEL` wins, as it does for Nx itself; then `nx.json`'s `parallel`;
    then Nx's own default.
    """
    environment = os.environ if environ is None else environ
    named = environment.get("NX_PARALLEL", "").strip()
    if named.isdigit() and int(named) > 0:
        return int(named)
    declared: Any = json.loads((root / "nx.json").read_text(encoding="utf-8")).get("parallel")
    if isinstance(declared, int) and not isinstance(declared, bool) and declared > 0:
        return declared
    return NX_DEFAULT_PARALLELISM


def beside_tests(root: Path) -> bool:
    """Whether the root `justfile` starts the end-to-end tier beside the test tier on this host.

    The justfile's own answer, asked of `just`, so the overlap is stated once.
    Where it cannot answer, the tiers are taken to overlap, which is the answer
    that gives each suite fewer workers rather than more.
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
