"""The recipes that bracket the printer integration tier, run for real.

`just octoprint-up` starts a real OctoPrint under a copy of the committed tree,
and `just octoprint-down` stops it. The assertion at the end is the one that
matters on a machine that runs this repeatedly: nothing the bring-up recipe
started is still there afterwards.

The tier itself, `just test-integration`, is not run here: the required
`integration (<platform>)` jobs run it between these same two recipes on every
pull request, and a second run inside the gate would only repeat theirs. What
is run here is what a failing tier leaves behind: made to fail on purpose once
the instance has written its logs, the tier — and this walk — keep those logs
before the bring-down, where they are still readable after it.
"""

from __future__ import annotations

import json
import os
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import REPO_ROOT, GateCopy
from repo_checks.expect import equal, passing, truth
from repo_checks.shell import start

STATE = ".octoprint-env"

#: Where a walk that failed keeps the instance's logs: in this checkout rather
#: than in the copy, which goes when the journey's own directory does.
KEPT = REPO_ROOT / ".octoprint-diagnostics" / "repo-e2e"

#: The variable naming where `octoprint-retain` keeps what it keeps.
RETAIN_DIR = "OCTOPRINT_ENV_RETAIN_DIR"

#: The logs a failed tier keeps, by where each sits under the state directory.
REQUIRED_LOGS = {
    "server.log": "server.log",
    "logs/octoprint.log": "instance/logs/octoprint.log",
    "logs/serial.log": "instance/logs/serial.log",
}

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


# Alone, after the parallel session: on Windows the bring-down reads the process
# table through a PowerShell CIM query it bounds at 30 seconds, which takes
# about 23 seconds on a Windows ARM runner with nothing else running, so any
# journey beside it pushes it past its own bound.
def walk(copy: GateCopy, check: Callable[[int], None], kept: Path) -> int:
    """Bring the environment up, `check` the process it started, and bring it down.

    Anything that fails between the two keeps the instance's own logs under
    `kept` before the bring-down runs, so the first failing walk is the one
    that can be diagnosed; the bring-down runs whatever happened.
    """
    try:
        brought_up = copy.just("octoprint-up", timeout=1200)
        passing(brought_up, describing="`just octoprint-up`")
        pid = int(json.loads(copy.read(f"{STATE}/instance.json"))["pid"])
        check(pid)
    except BaseException:
        copy.just("octoprint-retain", timeout=300, environment={RETAIN_DIR: str(kept)})
        raise
    finally:
        brought_down = copy.just("octoprint-down", timeout=600)
        passing(brought_down, describing="`just octoprint-down`")
    return pid


def holds_every_log_the_instance_wrote(state: Path, kept: Path) -> None:
    """`kept` names the instance and holds every required log as the instance wrote it.

    Read after the bring-down: the copy kept is still readable, and the log it
    was kept from begins with exactly what was kept — the instance may have
    written more while it stopped, and nothing else.
    """
    retained = json.loads((kept / "retained.json").read_text(encoding="utf-8"))
    equal(
        retained["instance"]["state_dir"],
        str(state.resolve()),
        describing="the instance the kept logs name",
    )
    truth(
        isinstance(retained["instance"]["record"], dict)
        and int(retained["instance"]["record"]["pid"]) > 0,
        describing=f"the record of the running instance, not {retained['instance']['record']}",
    )
    equal(retained["retained"], list(REQUIRED_LOGS), describing="the logs kept")
    equal(retained["absent"], [], describing="the logs reported absent")
    for name, written in REQUIRED_LOGS.items():
        text = (kept / name).read_text(encoding="utf-8")
        truth(bool(text), describing=f"the kept {name} to carry what the instance wrote")
        truth(
            (state / written).read_text(encoding="utf-8", errors="replace").startswith(text),
            describing=f"the kept {name} to be what the instance wrote to {written}",
        )


def wrote_every_log(state: Path) -> None:
    """The instance has written every log a failed tier keeps."""
    for written in REQUIRED_LOGS.values():
        truth((state / written).is_file(), describing=f"the running instance to have {written}")


@pytest.mark.alone
def test_the_bring_down_recipe_stops_what_the_bring_up_recipe_started(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Up, running, down — and no process left behind."""
    copy = gate_copy()

    pid = walk(
        copy,
        lambda pid: truth(running(pid), describing=f"the process {pid} the bring-up started"),
        KEPT,
    )

    truth(
        not running(pid),
        describing=f"process {pid} to be gone once the bring-down recipe has run",
    )


@pytest.mark.alone
def test_a_walk_that_fails_keeps_every_log_past_the_bring_down(
    gate_copy: Callable[[], GateCopy], tmp_path: Path
) -> None:
    """Failed on purpose once the instance has written its logs, the walk keeps them."""
    copy = gate_copy()
    kept = tmp_path / "kept"
    state = copy.root / STATE

    def fails(pid: int) -> None:
        wrote_every_log(state)
        msg = f"a deliberate failure of the walk over process {pid}"
        raise AssertionError(msg)

    with pytest.raises(AssertionError, match="a deliberate failure"):
        walk(copy, fails, kept)

    truth(not (state / "instance.json").exists(), describing="the bring-down to have run")
    holds_every_log_the_instance_wrote(state, kept)


@pytest.mark.alone
def test_a_tier_that_fails_keeps_every_log_before_the_bring_down(
    gate_copy: Callable[[], GateCopy], tmp_path: Path
) -> None:
    """The tier's own recipe, failed on purpose, keeps the instance's logs before teardown.

    The graph runner the recipe hands the tier to is one that fails at once, on
    a path of this journey's own: everything else — the recipe, the bring-up
    before it and the bring-down after it — is the committed one.
    """
    copy = gate_copy()
    kept = tmp_path / "kept"
    state = copy.root / STATE
    failing = tmp_path / "failing-runner"
    failing.mkdir()
    runner = failing / "bunx"
    runner.write_text(
        "#!/bin/sh\necho 'a deliberately failing tier' >&2\nexit 1\n", encoding="utf-8"
    )
    runner.chmod(0o755)

    brought_up = copy.just("octoprint-up", timeout=1200)
    try:
        passing(brought_up, describing="`just octoprint-up`")
        wrote_every_log(state)
        tier = copy.just(
            "test-integration",
            timeout=600,
            environment={
                "PATH": f"{failing}{os.pathsep}{os.environ['PATH']}",
                RETAIN_DIR: str(kept),
            },
        )
    finally:
        brought_down = copy.just("octoprint-down", timeout=600)
    passing(brought_down, describing="`just octoprint-down`")

    truth(tier.returncode != 0, describing="the deliberately failed tier to fail")
    truth(
        "a deliberately failing tier" in tier.stdout + tier.stderr,
        describing="the tier to have failed where it was made to",
    )
    holds_every_log_the_instance_wrote(state, kept)


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
