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

Everything the tool starts is owned from the moment it is started until the
draft is over, so nothing it leaves behind escapes either bound or the stop: a
child started as the tool exits, or one whose parent exited between two reads,
is still the draft's. On a POSIX host the tool leads a process group of its own,
which every descendant joins, and the group is what is read and what is killed;
on Linux `/proc` also carries what each member's reaped children consumed, so
the group's whole spend is counted, and elsewhere the live members' alone are,
which only ever reads lower. On Windows the tool is created suspended and
placed in a job object before it runs a single instruction, so every process it
ever starts is born in that job: the job's own accounting is what is read,
counting processes that have exited too, and terminating the job is the stop.
A compiler run on the tool's behalf by a server it hands the work to —
`sccache` — is in neither on any host.
"""

from __future__ import annotations

import ctypes
import os
import signal
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Protocol

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
    process, owned = _launch(command, cwd, env)
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
    breach = ""
    try:
        last_activity: object = None
        last_worked = time.monotonic()
        while process.poll() is None:
            time.sleep(POLL_SECONDS)
            spent, activity = owned.sample()
            if activity != last_activity:
                last_worked = time.monotonic()
            last_activity = activity
            cpu = max(cpu, spent)
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
    finally:
        # Whether it breached a bound, ended, or could not be read, nothing it
        # started outlives it: a descendant it left running would hold its
        # output streams open, and the readers would wait on them for ever.
        owned.stop()
        process.wait()
        owned.close()
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


class _Owned(Protocol):
    """Everything a draft's tool has started, owned for as long as the draft runs."""

    def sample(self) -> tuple[float, object]:
        """The CPU seconds it has consumed, and a reading that changes with any work."""
        ...

    def stop(self) -> None:
        """Stop every process in it."""
        ...

    def close(self) -> None:
        """Let go of whatever held it."""
        ...


def _launch(
    command: list[str], cwd: Path, env: dict[str, str]
) -> tuple[subprocess.Popen[str], _Owned]:
    """Start the tool, owning it and everything it will start before it runs."""
    if sys.platform == "win32":
        job = _Job()
        process = start(command, cwd=cwd, env=env, suspended=True)
        try:
            job.adopt(process.pid)
        except BaseException:
            process.kill()
            process.wait()
            job.close()
            raise
        owned: _Owned = job
    else:
        process = start(command, cwd=cwd, env=env, own_session=True)
        owned = _ProcessGroup(process.pid)
    if process.stdin is not None:
        process.stdin.close()
    return process, owned


class _ProcessGroup:
    """A POSIX process group the tool leads, which every descendant joins."""

    def __init__(self, group: int) -> None:
        """Own the group the tool leads, whose id is the tool's own."""
        self.group = group

    def sample(self) -> tuple[float, object]:
        """The members' CPU seconds, and each member's own."""
        members = _group_members(self.group)
        return sum(members.values()), members

    def stop(self) -> None:
        """Kill every member, one whose parent already exited included."""
        if sys.platform != "win32":
            try:
                os.killpg(self.group, signal.SIGKILL)
            except ProcessLookupError:
                return

    def close(self) -> None:
        """Nothing is held for a process group."""


def _group_members(group: int) -> dict[int, float]:
    """The CPU seconds of each live process in `group`."""
    if sys.platform != "win32" and Path("/proc/self/stat").exists():
        return _proc_members(group, os.sysconf("SC_CLK_TCK"))
    return _ps_members(group)


def _proc_members(group: int, ticks: int) -> dict[int, float]:
    """Each member `/proc` lists, its own CPU and its reaped children's.

    `ticks` is the host's clock ticks per second, the unit `/proc` counts in.
    """
    members: dict[int, float] = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        # The command name is parenthesised and may hold spaces or parentheses,
        # so the fields are read after its last closing one: state, ppid, pgrp,
        # then utime, stime, cutime and cstime at 11-14.
        fields = stat[stat.rfind(")") + 2 :].split()
        if int(fields[2]) == group:
            members[int(entry.name)] = sum(int(field) for field in fields[11:15]) / ticks
    return members


def _ps_members(group: int) -> dict[int, float]:
    """Each member `ps` lists, with the CPU time it reports for each."""
    listed = shell_run(["ps", "-A", "-o", "pid=,pgid=,time="], check=True).stdout
    members: dict[int, float] = {}
    for line in listed.splitlines():
        parts = line.split()
        if len(parts) == 3 and int(parts[1]) == group:
            members[int(parts[0])] = _clock_seconds(parts[2])
    return members


def _clock_seconds(clock: str) -> float:
    """Seconds in `ps`'s `[[dd-]hh:]mm:ss[.ss]`."""
    days, _, rest = clock.rpartition("-")
    seconds = 0.0
    for part in rest.split(":"):
        seconds = seconds * 60 + float(part)
    return seconds + (int(days) * 86400 if days else 0)


if sys.platform == "win32":
    from ctypes import wintypes

    _KERNEL32 = ctypes.WinDLL("kernel32", use_last_error=True)

    #: `JobObjectBasicAccountingInformation` and `JobObjectExtendedLimitInformation`.
    _JOB_ACCOUNTING = 1
    _JOB_EXTENDED_LIMITS = 9
    #: Every process in the job is killed when its last handle is closed.
    _KILL_ON_JOB_CLOSE = 0x2000
    #: The rights `adopt` opens the tool with, and each of its threads.
    _PROCESS_SET_QUOTA_AND_TERMINATE = 0x0100 | 0x0001
    _THREAD_SUSPEND_RESUME = 0x0002
    _SNAPSHOT_THREADS = 0x0004
    _INVALID_HANDLE = wintypes.HANDLE(-1).value
    #: The job's times are counted in units of 100 ns.
    _TICKS_PER_SECOND = 10_000_000

    class _BasicLimits(ctypes.Structure):
        _fields_ = (
            ("PerProcessUserTimeLimit", ctypes.c_int64),
            ("PerJobUserTimeLimit", ctypes.c_int64),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.c_size_t),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        )

    class _IoCounters(ctypes.Structure):
        _fields_ = tuple(
            (name, ctypes.c_uint64)
            for name in (
                "ReadOperationCount",
                "WriteOperationCount",
                "OtherOperationCount",
                "ReadTransferCount",
                "WriteTransferCount",
                "OtherTransferCount",
            )
        )

    class _ExtendedLimits(ctypes.Structure):
        _fields_ = (
            ("BasicLimitInformation", _BasicLimits),
            ("IoInfo", _IoCounters),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t),
        )

    class _Accounting(ctypes.Structure):
        _fields_ = (
            ("TotalUserTime", ctypes.c_int64),
            ("TotalKernelTime", ctypes.c_int64),
            ("ThisPeriodTotalUserTime", ctypes.c_int64),
            ("ThisPeriodTotalKernelTime", ctypes.c_int64),
            ("TotalPageFaultCount", wintypes.DWORD),
            ("TotalProcesses", wintypes.DWORD),
            ("ActiveProcesses", wintypes.DWORD),
            ("TotalTerminatedProcesses", wintypes.DWORD),
        )

    class _ThreadEntry(ctypes.Structure):
        _fields_ = (
            ("dwSize", wintypes.DWORD),
            ("cntUsage", wintypes.DWORD),
            ("th32ThreadID", wintypes.DWORD),
            ("th32OwnerProcessID", wintypes.DWORD),
            ("tpBasePri", wintypes.LONG),
            ("tpDeltaPri", wintypes.LONG),
            ("dwFlags", wintypes.DWORD),
        )

    for _name, _arguments, _answer in (
        ("CreateJobObjectW", (wintypes.LPVOID, wintypes.LPCWSTR), wintypes.HANDLE),
        (
            "SetInformationJobObject",
            (wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD),
            wintypes.BOOL,
        ),
        ("AssignProcessToJobObject", (wintypes.HANDLE, wintypes.HANDLE), wintypes.BOOL),
        (
            "QueryInformationJobObject",
            (
                wintypes.HANDLE,
                ctypes.c_int,
                wintypes.LPVOID,
                wintypes.DWORD,
                ctypes.POINTER(wintypes.DWORD),
            ),
            wintypes.BOOL,
        ),
        ("TerminateJobObject", (wintypes.HANDLE, wintypes.UINT), wintypes.BOOL),
        ("OpenProcess", (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD), wintypes.HANDLE),
        ("OpenThread", (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD), wintypes.HANDLE),
        ("ResumeThread", (wintypes.HANDLE,), wintypes.DWORD),
        ("CreateToolhelp32Snapshot", (wintypes.DWORD, wintypes.DWORD), wintypes.HANDLE),
        ("Thread32First", (wintypes.HANDLE, ctypes.POINTER(_ThreadEntry)), wintypes.BOOL),
        ("Thread32Next", (wintypes.HANDLE, ctypes.POINTER(_ThreadEntry)), wintypes.BOOL),
        ("CloseHandle", (wintypes.HANDLE,), wintypes.BOOL),
    ):
        _function = getattr(_KERNEL32, _name)
        _function.argtypes = _arguments
        _function.restype = _answer

    def _checked(answer: object) -> object:
        """The answer of a call that signals failure with a null or false one."""
        if not answer:
            raise ctypes.WinError(ctypes.get_last_error())
        return answer

    class _Job:
        """A job object the tool is placed in before it runs a single instruction."""

        def __init__(self) -> None:
            """Create the job, killing everything in it once it is let go of."""
            self.handle = _checked(_KERNEL32.CreateJobObjectW(None, None))
            limits = _ExtendedLimits()
            limits.BasicLimitInformation.LimitFlags = _KILL_ON_JOB_CLOSE
            _checked(
                _KERNEL32.SetInformationJobObject(
                    self.handle, _JOB_EXTENDED_LIMITS, ctypes.byref(limits), ctypes.sizeof(limits)
                )
            )

        def adopt(self, pid: int) -> None:
            """Place the suspended tool in the job, and only then let it run."""
            tool = _checked(_KERNEL32.OpenProcess(_PROCESS_SET_QUOTA_AND_TERMINATE, False, pid))
            try:
                _checked(_KERNEL32.AssignProcessToJobObject(self.handle, tool))
            finally:
                _KERNEL32.CloseHandle(tool)
            snapshot = _KERNEL32.CreateToolhelp32Snapshot(_SNAPSHOT_THREADS, 0)
            if snapshot == _INVALID_HANDLE:
                raise ctypes.WinError(ctypes.get_last_error())
            try:
                entry = _ThreadEntry()
                entry.dwSize = ctypes.sizeof(entry)
                listed = _KERNEL32.Thread32First(snapshot, ctypes.byref(entry))
                while listed:
                    if entry.th32OwnerProcessID == pid:
                        thread = _checked(
                            _KERNEL32.OpenThread(_THREAD_SUSPEND_RESUME, False, entry.th32ThreadID)
                        )
                        try:
                            if _KERNEL32.ResumeThread(thread) == 0xFFFFFFFF:
                                raise ctypes.WinError(ctypes.get_last_error())
                        finally:
                            _KERNEL32.CloseHandle(thread)
                    listed = _KERNEL32.Thread32Next(snapshot, ctypes.byref(entry))
            finally:
                _KERNEL32.CloseHandle(snapshot)

        def sample(self) -> tuple[float, object]:
            """The job's CPU seconds, its exited processes' included, and its counts."""
            accounting = _Accounting()
            _checked(
                _KERNEL32.QueryInformationJobObject(
                    self.handle,
                    _JOB_ACCOUNTING,
                    ctypes.byref(accounting),
                    ctypes.sizeof(accounting),
                    None,
                )
            )
            ticks = accounting.TotalUserTime + accounting.TotalKernelTime
            return ticks / _TICKS_PER_SECOND, (
                ticks,
                accounting.TotalProcesses,
                accounting.ActiveProcesses,
            )

        def stop(self) -> None:
            """Terminate every process in the job."""
            _KERNEL32.TerminateJobObject(self.handle, 1)

        def close(self) -> None:
            """Let go of the job, which kills anything still in it."""
            _KERNEL32.CloseHandle(self.handle)
