"""The recipes that bracket the printer integration tier, run for real.

`just octoprint-up` starts a real OctoPrint under a copy of the committed tree,
`just test-integration` runs the tier against what it started, and `just
octoprint-down` stops it. The assertion at the end is the one that matters on a
machine that runs this repeatedly: nothing the bring-up recipe started is still
there afterwards.
"""

from __future__ import annotations

import json
import os
import sys
from collections.abc import Callable

import pytest
from journey import GateCopy
from repo_checks.expect import equal, passing, truth
from repo_checks.shell import start

STATE = ".octoprint-env"

#: The access a Windows process handle is opened with: enough to read its exit
#: code, and nothing that could change it.
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000

#: The exit code Windows reports for a process that has not exited.
STILL_ACTIVE = 259


def running(pid: int) -> bool:
    """Whether a process the bring-up recipe started is still there.

    On POSIX, signal 0 asks the kernel whether the process exists and delivers
    nothing. On Windows `os.kill` has no such signal: any number but the two
    console events **terminates** the process — so probing with it there would
    be the very bring-down this journey is checking the recipe for. The Windows
    answer asks the process for its exit code instead, which only reads.
    """
    if sys.platform == "win32":
        return _windows_exit_code(pid) == STILL_ACTIVE
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def _windows_exit_code(pid: int) -> int | None:
    """The exit code Windows reports for `pid`, or None where it has no such process.

    Raises:
        OSError: On a host that is not Windows, which has no process handles.
    """
    if sys.platform != "win32":
        message = f"only Windows answers an exit code for process {pid} by handle"
        raise OSError(message)
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.GetExitCodeProcess.argtypes = (wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD))
    kernel32.GetExitCodeProcess.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel32.CloseHandle.restype = wintypes.BOOL

    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return None
    try:
        code = wintypes.DWORD()
        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)):
            return None
        return int(code.value)
    finally:
        kernel32.CloseHandle(handle)


def test_the_tier_runs_between_the_bring_up_and_bring_down_recipes(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Up, the tier, down — and no process left behind."""
    copy = gate_copy()

    brought_up = copy.just("octoprint-up", timeout=1200)

    passing(brought_up, describing="`just octoprint-up`")
    record = json.loads(copy.read(f"{STATE}/instance.json"))
    pid = int(record["pid"])
    truth(running(pid), describing=f"the process {pid} the bring-up recipe started")

    tier = copy.just("test-integration", timeout=2400)
    brought_down = copy.just("octoprint-down", timeout=600)

    passing(tier, describing="`just test-integration` against what the bring-up started")
    passing(brought_down, describing="`just octoprint-down`")
    truth(
        not running(pid),
        describing=f"process {pid} to be gone once the bring-down recipe has run",
    )


def test_the_probe_tells_a_running_process_from_an_exited_one_and_stops_neither() -> None:
    """A process asked about is still running afterwards, and one that exited is gone.

    Nothing is substituted: this host's own answer, whichever host this is. On
    Windows a probe that signalled the process would end it right here, and the
    child would not be there to be asked to exit.
    """
    child = start([sys.executable, "-c", "import sys; sys.stdin.read()"])
    try:
        truth(running(child.pid), describing=f"the running process {child.pid} to be running")
        equal(child.poll(), None, describing="the exit of a process the probe only asked about")
    finally:
        child.communicate(input="", timeout=60)

    equal(child.returncode, 0, describing="the exit of a process that was left to finish")
    truth(not running(child.pid), describing=f"the exited process {child.pid} to be gone")


def test_on_windows_the_probe_reads_an_exit_code_and_sends_no_signal(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """`os.kill` terminates a Windows process, so the Windows answer never calls it."""
    signalled: list[tuple[int, int]] = []
    exit_codes = {10: STILL_ACTIVE, 11: 0}
    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(os, "kill", lambda pid, signal: signalled.append((pid, signal)))
    monkeypatch.setattr(sys.modules[__name__], "_windows_exit_code", exit_codes.get)

    equal(running(10), True, describing="a process Windows reports as still active")
    equal(running(11), False, describing="a process Windows reports as exited")
    equal(running(12), False, describing="a process Windows has no handle for")
    equal(signalled, [], describing="the signals the probe sent")
