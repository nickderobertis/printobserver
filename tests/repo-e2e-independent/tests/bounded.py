"""Run a drafting tool under bounds that do not move with the host's load.

A fixed wall-clock deadline cannot tell a draft a busy host slowed from one that
hung: the release-path journeys' drafts ran past a 600 s deadline while a
coverage build compiled beside them, and the draft of every crate finished in
149 s on the same host when it was quiet. So a draft here is held to two bounds instead, each
of which a slow host leaves where it was. The CPU time its process tree has
consumed catches a runaway, which spends CPU however busy the host is. The time
since the tree last did anything — said something, consumed CPU, or started or
ended a process — catches a hang, which does none of those however long it is
given, while a busy host still hands a working tool some CPU every second.
Output alone is no such signal for `release-plz`: it says nothing at all while
it semver-checks, 291-317 s of a 340-370 s draft beside a coverage build, so
its silence grows with the host's load exactly as a deadline does. Either bound
breached stops the whole tree and is reported naming the bound, the figure that
breached it and what the tool had said. A tool that ends has its tree stopped
too, so nothing it left running holds its output open.

What CPU time can be read differs by host, and every reading counts the tool's
live process tree. On Linux `/proc` also carries what each live process's
reaped children consumed, so the whole tree's spend is counted once. Elsewhere
the live processes alone are, which only ever reads lower. A compiler run on
the tool's behalf by a server it hands the work to — `sccache` — is not in the
tree on any host.
"""

from __future__ import annotations

import os
import signal
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import IO

from journey import plain
from repo_checks.shell import run as shell_run
from repo_checks.shell import start

#: How often the tree is read and the bounds are checked.
POLL_SECONDS = 1.0

#: How long the output readers are given to finish once the tree is stopped. A
#: descendant nothing could reach would otherwise hold a stream open for ever.
READER_GRACE_SECONDS = 5.0

#: How much of what the tool said a report of a stopped draft carries.
REPORTED_TAIL_CHARACTERS = 4000


@dataclass(frozen=True)
class Bounds:
    """The two bounds a draft is held to.

    Attributes:
        cpu_seconds: The most CPU time, user and system, its process tree may
            consume.
        idle_seconds: The longest its tree may go without writing to either
            stream, consuming CPU, or starting or ending a process.
    """

    cpu_seconds: float
    idle_seconds: float


@dataclass(frozen=True)
class Drafted:
    """A draft that ran to its own end.

    Attributes:
        returncode: Its exit status.
        stdout: What it wrote to standard output, terminal escapes removed.
        stderr: What it wrote to standard error, terminal escapes removed.
        cpu_seconds: The most CPU time its tree was read as having consumed.
        longest_idle: The longest its tree went without doing anything.
    """

    returncode: int
    stdout: str
    stderr: str
    cpu_seconds: float
    longest_idle: float


class DraftStoppedError(AssertionError):
    """A draft breached one of its bounds, and its tree was stopped."""


def bounded(
    command: list[str], cwd: Path, *, bounds: Bounds, env: dict[str, str], describing: str
) -> Drafted:
    """Run `command` to its end, or stop its whole tree when it breaches `bounds`.

    Raises:
        DraftStoppedError: When it consumed more CPU or sat idle for longer
            than `bounds` allow, naming which, by how much, and what it had said.
    """
    process = start(command, cwd=cwd, env=env, own_session=True)
    if process.stdin is not None:
        process.stdin.close()
    said: dict[str, list[str]] = {"stdout": [], "stderr": []}
    last_said = [time.monotonic()]
    readers = [
        threading.Thread(target=_read, args=(stream, said[name], last_said), daemon=True)
        for name, stream in (("stdout", process.stdout), ("stderr", process.stderr))
        if stream is not None
    ]
    for reader in readers:
        reader.start()
    longest_idle = 0.0
    cpu = 0.0
    last_tree: dict[int, tuple[int, float]] = {}
    last_worked = time.monotonic()
    breach = ""
    while process.poll() is None:
        time.sleep(POLL_SECONDS)
        tree = _tree(process.pid)
        if tree != last_tree:
            last_worked = time.monotonic()
        last_tree = tree
        cpu = max(cpu, sum(seconds for _, seconds in tree.values()))
        idle = time.monotonic() - max(last_worked, last_said[0])
        longest_idle = max(longest_idle, idle)
        if cpu > bounds.cpu_seconds:
            breach = (
                f"its process tree consumed {cpu:.1f} s of CPU, "
                f"over its bound of {bounds.cpu_seconds:.0f} s"
            )
        elif idle > bounds.idle_seconds:
            breach = (
                f"its process tree did nothing for {idle:.1f} s, "
                f"over its bound of {bounds.idle_seconds:.0f} s"
            )
        if breach:
            break
    # Whether it breached a bound or ended, nothing it started outlives it: a
    # descendant it left running would hold its output streams open, and the
    # readers would wait on them for ever.
    _stop(process.pid, set(last_tree))
    process.wait()
    for reader in readers:
        reader.join(READER_GRACE_SECONDS)
    stdout, stderr = (plain("".join(said[name])) for name in ("stdout", "stderr"))
    if breach:
        tail = (stdout + stderr)[-REPORTED_TAIL_CHARACTERS:]
        message = f"{describing} was stopped: {breach}. It had said:\n{tail}"
        raise DraftStoppedError(message)
    return Drafted(process.returncode, stdout, stderr, cpu, longest_idle)


def _read(stream: IO[str], into: list[str], last_said: list[float]) -> None:
    """Collect one stream line by line, noting when each line arrived."""
    for line in iter(stream.readline, ""):
        into.append(line)
        last_said[0] = time.monotonic()


def _tree(root: int) -> dict[int, tuple[int, float]]:
    """The root's live process tree: each process's parent and CPU seconds."""
    table = _processes()
    tree: dict[int, tuple[int, float]] = {}
    frontier = [root]
    while frontier:
        pid = frontier.pop()
        if pid in table and pid not in tree:
            tree[pid] = table[pid]
            frontier.extend(child for child, (parent, _) in table.items() if parent == pid)
    return tree


def _processes() -> dict[int, tuple[int, float]]:
    """Every process on the host: its parent and the CPU seconds it has consumed."""
    if sys.platform == "win32":
        return _windows_processes()
    if Path("/proc/self/stat").exists():
        return _proc_processes(os.sysconf("SC_CLK_TCK"))
    return _ps_processes()


def _proc_processes(ticks: int) -> dict[int, tuple[int, float]]:
    """Every process `/proc` lists, its own CPU and its reaped children's.

    `ticks` is the host's clock ticks per second, the unit `/proc` counts in.
    """
    table: dict[int, tuple[int, float]] = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        # The command name is parenthesised and may hold spaces or parentheses,
        # so the fields are read after its last closing one: state, ppid, then
        # utime, stime, cutime and cstime at 11-14.
        fields = stat[stat.rfind(")") + 2 :].split()
        spent = sum(int(field) for field in fields[11:15])
        table[int(entry.name)] = (int(fields[1]), spent / ticks)
    return table


def _ps_processes() -> dict[int, tuple[int, float]]:
    """Every process `ps` lists, with the CPU time it reports for each."""
    listed = shell_run(["ps", "-A", "-o", "pid=,ppid=,time="], check=True).stdout
    table: dict[int, tuple[int, float]] = {}
    for line in listed.splitlines():
        parts = line.split()
        if len(parts) == 3:
            table[int(parts[0])] = (int(parts[1]), _clock_seconds(parts[2]))
    return table


def _clock_seconds(clock: str) -> float:
    """Seconds in `ps`'s `[[dd-]hh:]mm:ss[.ss]`."""
    days, _, rest = clock.rpartition("-")
    seconds = 0.0
    for part in rest.split(":"):
        seconds = seconds * 60 + float(part)
    return seconds + (int(days) * 86400 if days else 0)


def _windows_processes() -> dict[int, tuple[int, float]]:
    """Every process `Win32_Process` lists, with its user and kernel time."""
    listed = shell_run(
        [
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_Process | ForEach-Object "
            '{ "$($_.ProcessId) $($_.ParentProcessId) $($_.UserModeTime + $_.KernelModeTime)" }',
        ],
        check=True,
    ).stdout
    table: dict[int, tuple[int, float]] = {}
    for line in listed.splitlines():
        parts = line.split()
        if len(parts) == 3:
            # The two times are counted in units of 100 ns.
            table[int(parts[0])] = (int(parts[1]), int(parts[2]) / 10_000_000)
    return table


def _stop(root: int, seen: set[int]) -> None:
    """Stop every process the root started, and the root if it is still running.

    On a POSIX host the root leads a process group of its own, which every
    descendant joins, so one signal to the group reaches all of them — one
    whose parent already exited included. Windows has no such group, so there
    the root's tree and every process last seen in it are stopped.
    """
    if sys.platform == "win32":
        for pid in {root, *seen}:
            shell_run(["taskkill", "/PID", str(pid), "/T", "/F"])
        return
    try:
        os.killpg(root, signal.SIGKILL)
    except ProcessLookupError:
        return
