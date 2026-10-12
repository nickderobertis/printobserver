"""Time each way of reading which program a process id is, on this host.

`tools/octoprint-env`'s `command_of` answers a process's command line on
Windows by starting PowerShell for one `Get-CimInstance Win32_Process` query.
This starts one process with a distinctive command line, reads it back
`ROUNDS` times by each method, and writes `{method: {seconds per call, what it
answered, whether that is the command line the current read answers}}` as
JSON to the path given as the only argument. Off Windows it times `ps`, the
read `command_of` makes there.
"""

from __future__ import annotations

import ctypes
import json
import shutil
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any

from repo_checks.shell import run, start

if TYPE_CHECKING:
    from collections.abc import Callable

ROUNDS = 10
MARKER = "spike-lookup-bench-marker"


def _run(argv: list[str]) -> str:
    return run(argv, timeout=120).stdout


def _commands(pid: int) -> dict[str, list[str] | None]:
    powershell = shutil.which("powershell")
    pwsh = shutil.which("pwsh")
    cim = f"(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine"
    return {
        "current: powershell Get-CimInstance": [powershell, "-NoProfile", "-Command", cim]
        if powershell
        else None,
        "powershell startup alone": [powershell, "-NoProfile", "-Command", "exit"]
        if powershell
        else None,
        "powershell Get-Process .Path": [
            powershell,
            "-NoProfile",
            "-Command",
            f"(Get-Process -Id {pid}).Path",
        ]
        if powershell
        else None,
        "pwsh Get-CimInstance": [pwsh, "-NoProfile", "-Command", cim] if pwsh else None,
        "pwsh Get-Process .CommandLine": [
            pwsh,
            "-NoProfile",
            "-Command",
            f"(Get-Process -Id {pid}).CommandLine",
        ]
        if pwsh
        else None,
        "tasklist /FI": ["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
        "wmic process get CommandLine": [
            "wmic",
            "process",
            "where",
            f"ProcessId={pid}",
            "get",
            "CommandLine",
            "/value",
        ]
        if shutil.which("wmic")
        else None,
    }


def _win32_command_line(pid: int) -> str:
    """The command line through `NtQueryInformationProcess(ProcessCommandLineInformation)`."""
    kernel32 = ctypes.CDLL("kernel32", use_last_error=True)
    ntdll = ctypes.CDLL("ntdll")
    query_limited = 0x1000
    handle = kernel32.OpenProcess(query_limited, False, pid)
    if not handle:
        return ""
    try:
        size = ctypes.c_ulong(0)
        buffer = ctypes.create_string_buffer(65536)
        status = ntdll.NtQueryInformationProcess(
            handle, 60, buffer, len(buffer), ctypes.byref(size)
        )
        if status != 0:
            return ""

        class UnicodeString(ctypes.Structure):
            _fields_ = (
                ("Length", ctypes.c_ushort),
                ("MaximumLength", ctypes.c_ushort),
                ("Buffer", ctypes.c_void_p),
            )

        header = UnicodeString.from_buffer(buffer)
        return ctypes.wstring_at(header.Buffer, header.Length // 2)
    finally:
        kernel32.CloseHandle(handle)


def _win32_image(pid: int) -> str:
    """The image path through `QueryFullProcessImageNameW`."""
    kernel32 = ctypes.CDLL("kernel32", use_last_error=True)
    handle = kernel32.OpenProcess(0x1000, False, pid)
    if not handle:
        return ""
    try:
        length = ctypes.c_ulong(32768)
        buffer = ctypes.create_unicode_buffer(length.value)
        kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(length))
        return buffer.value
    finally:
        kernel32.CloseHandle(handle)


def _time(read: Callable[[], str]) -> dict[str, Any]:
    seconds = []
    answer = ""
    for _ in range(ROUNDS):
        started = time.perf_counter()
        answer = read()
        seconds.append(time.perf_counter() - started)
    seconds.sort()
    return {
        "median_s": round(seconds[len(seconds) // 2], 4),
        "min_s": round(seconds[0], 4),
        "max_s": round(seconds[-1], 4),
        "answered": answer.strip()[:300],
    }


def main() -> int:
    """Measure every method this host has, and write the result."""
    out = Path(sys.argv[1])
    child = start([sys.executable, "-c", "import time; time.sleep(900)", MARKER])
    try:
        time.sleep(1)
        results: dict[str, Any] = {"rounds": ROUNDS, "pid": child.pid, "methods": {}}
        if sys.platform == "win32":
            for name, argv in _commands(child.pid).items():
                if argv is None:
                    results["methods"][name] = {"absent": True}
                    continue
                results["methods"][name] = _time(lambda argv=argv: _run(argv))
            results["methods"]["win32 NtQueryInformationProcess"] = _time(
                lambda: _win32_command_line(child.pid)
            )
            results["methods"]["win32 QueryFullProcessImageNameW"] = _time(
                lambda: _win32_image(child.pid)
            )
        else:
            results["methods"]["ps -o command="] = _time(
                lambda: _run(["ps", "-o", "command=", "-p", str(child.pid)])
            )
        current = next(iter(results["methods"].values())).get("answered", "").replace('"', "")
        for method in results["methods"].values():
            answered = method.get("answered")
            if answered is not None:
                method["is_the_command_line"] = MARKER in answered
                method["same_as_current"] = answered.replace('"', "") == current
        out.write_text(json.dumps(results, indent=1), encoding="utf-8")
        print(json.dumps(results, indent=1))
    finally:
        child.kill()
        child.wait()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
