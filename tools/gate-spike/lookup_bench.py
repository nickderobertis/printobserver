"""Time each way of reading which program a process id is, on this host.

`tools/octoprint-env`'s `command_of` answers a process's command line on
Windows by starting Windows PowerShell for one `Get-CimInstance Win32_Process`
query, and with `ps` elsewhere; this calls that function itself as the current
read, so it measures whatever that read is. This starts one process with a
distinctive command line, reads it back `ROUNDS` times by each method, and
writes one `LookupReport` as JSON to the path given as its only argument:
each method's seconds per call, what it answered, and whether that is the
command line the current read answers.

Exit status: 0 when every method present on this host answered, 1 when one
failed (the report is written either way, naming the failure), 2 when it was
not given exactly one output path.
"""

from __future__ import annotations

import ctypes
import json
import math
import shutil
import statistics
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, replace
from pathlib import Path
from typing import TYPE_CHECKING, TypeGuard

from octoprint_env import command_of
from repo_checks.shell import run, start
from spike_names import ProcessId

if TYPE_CHECKING:
    from collections.abc import Callable

ROUNDS = 5
#: The method `measure` names the current read, `octoprint_env.command_of`.
CURRENT = "current: octoprint_env.command_of"
MARKER = "spike-lookup-bench-marker"
#: `PROCESS_QUERY_LIMITED_INFORMATION`, all either Win32 read needs.
QUERY_LIMITED = 0x1000
#: `ProcessCommandLineInformation`, the class `NtQueryInformationProcess` answers
#: a process's command line under.
COMMAND_LINE_CLASS = 60


class MethodError(RuntimeError):
    """One method could not answer."""


@dataclass(frozen=True)
class Method:
    """How one way of reading a process's program did."""

    name: str
    present: bool
    median_s: float | None = None
    min_s: float | None = None
    max_s: float | None = None
    answered: str = ""
    failure: str = ""
    is_the_command_line: bool = False
    same_as_current: bool = False


@dataclass(frozen=True)
class LookupReport:
    """Every method this host has, over one process."""

    rounds: int
    pid: ProcessId
    methods: list[Method]

    @classmethod
    def read(cls, path: Path) -> LookupReport:
        """Read a report this program wrote, refusing one of any other shape."""
        document = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(document, dict):
            msg = f"{path} is not a report this benchmark writes"
            raise MethodError(msg)
        rounds, pid, methods = document.get("rounds"), document.get("pid"), document.get("methods")
        if not (_whole(rounds) and _whole(pid) and isinstance(methods, list)):
            msg = f"{path} carries no rounds, pid and methods of the shapes this benchmark writes"
            raise MethodError(msg)
        return cls(rounds=rounds, pid=pid, methods=[_method(path, method) for method in methods])

    @property
    def failed(self) -> list[str]:
        """The methods present on this host that did not answer."""
        return [method.name for method in self.methods if method.present and method.failure]


def _whole(value: object) -> TypeGuard[int]:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


#: The JSON type each `Method` field is written as.
_FIELD_TYPES: dict[str, tuple[type, ...]] = {
    "name": (str,),
    "present": (bool,),
    "median_s": (float, int, type(None)),
    "min_s": (float, int, type(None)),
    "max_s": (float, int, type(None)),
    "answered": (str,),
    "failure": (str,),
    "is_the_command_line": (bool,),
    "same_as_current": (bool,),
}


def _method(path: Path, method: object) -> Method:
    """One method a report carries, refusing one of any other shape."""
    if not isinstance(method, dict) or set(method) != set(_FIELD_TYPES):
        msg = f"{path} carries a method of another shape: {method!r}"
        raise MethodError(msg)
    for name, types in _FIELD_TYPES.items():
        value = method[name]
        typed = isinstance(value, types) and not (float in types and isinstance(value, bool))
        timing = isinstance(value, (int, float)) and not isinstance(value, bool)
        if not typed or (timing and (not math.isfinite(value) or value < 0)):
            msg = f"{path} carries a method whose `{name}` is {value!r}"
            raise MethodError(msg)
    return Method(**method)


def _read(argv: list[str]) -> str:
    ran = run(argv, timeout=120)
    if ran.returncode != 0:
        msg = f"exited {ran.returncode}: {ran.stderr.strip()[:300]}"
        raise MethodError(msg)
    return ran.stdout


def _commands(pid: ProcessId) -> dict[str, list[str] | None]:
    powershell = shutil.which("powershell")
    pwsh = shutil.which("pwsh")
    cim = f"(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine"

    def shell(program: str | None, script: str) -> list[str] | None:
        return [program, "-NoProfile", "-Command", script] if program else None

    return {
        "powershell startup alone": shell(powershell, "exit"),
        "powershell Get-Process .Path": shell(powershell, f"(Get-Process -Id {pid}).Path"),
        "pwsh Get-CimInstance": shell(pwsh, cim),
        "pwsh Get-Process .CommandLine": shell(pwsh, f"(Get-Process -Id {pid}).CommandLine"),
        "tasklist /FI": ["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
        "wmic process get CommandLine": (
            ["wmic", "process", "where", f"ProcessId={pid}", "get", "CommandLine", "/value"]
            if shutil.which("wmic")
            else None
        ),
    }


def _last_error() -> int:
    """The calling thread's last Win32 error, which only Windows keeps."""
    return ctypes.get_last_error() if sys.platform == "win32" else 0


def _opened(pid: ProcessId) -> tuple[ctypes.CDLL, int]:
    kernel32 = ctypes.CDLL("kernel32", use_last_error=True)
    handle = kernel32.OpenProcess(QUERY_LIMITED, 0, pid)
    if not handle:
        msg = f"OpenProcess refused {pid}: error {_last_error()}"
        raise MethodError(msg)
    return kernel32, handle


def _win32_command_line(pid: ProcessId, capacity: int = 65536) -> str:
    """The command line through `NtQueryInformationProcess(ProcessCommandLineInformation)`."""

    class UnicodeString(ctypes.Structure):
        _fields_ = (
            ("Length", ctypes.c_ushort),
            ("MaximumLength", ctypes.c_ushort),
            ("Buffer", ctypes.c_void_p),
        )

    kernel32, handle = _opened(pid)
    try:
        written = ctypes.c_ulong(0)
        buffer = ctypes.create_string_buffer(capacity)
        status = ctypes.CDLL("ntdll").NtQueryInformationProcess(
            handle, COMMAND_LINE_CLASS, buffer, len(buffer), ctypes.byref(written)
        )
        # llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
        if status != 0 or not ctypes.sizeof(UnicodeString) <= written.value <= len(buffer):
            msg = f"NtQueryInformationProcess answered status {status:#x}, {written.value} bytes"
            raise MethodError(msg)
        header = UnicodeString.from_buffer(buffer)
        start_of = ctypes.addressof(buffer)
        # llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
        if (
            header.Buffer is None
            or header.Length % 2
            or header.Length > header.MaximumLength
            or not start_of
            <= header.Buffer
            <= header.Buffer + header.Length
            <= start_of + written.value
        ):
            msg = "NtQueryInformationProcess answered a string outside its own buffer"
            raise MethodError(msg)
        return ctypes.wstring_at(header.Buffer, header.Length // 2)
    finally:
        kernel32.CloseHandle(handle)


def _win32_image(pid: ProcessId, capacity: int = 32768) -> str:
    """The image path through `QueryFullProcessImageNameW`."""
    kernel32, handle = _opened(pid)
    try:
        length = ctypes.c_ulong(capacity)
        buffer = ctypes.create_unicode_buffer(length.value)
        if not kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(length)):
            msg = f"QueryFullProcessImageNameW failed: error {_last_error()}"
            raise MethodError(msg)
        return buffer.value
    finally:
        kernel32.CloseHandle(handle)


def _time(name: str, read: Callable[[], str]) -> Method:
    seconds = []
    answer = ""
    try:
        for _ in range(ROUNDS):
            started = time.perf_counter()
            answer = read()
            seconds.append(time.perf_counter() - started)
    except MethodError as error:
        return Method(name, present=True, failure=str(error))
    return Method(
        name,
        present=True,
        median_s=round(statistics.median(seconds), 4),
        min_s=round(min(seconds), 4),
        max_s=round(max(seconds), 4),
        answered=answer.strip()[:300],
        is_the_command_line=MARKER in answer,
    )


def _current(pid: ProcessId) -> str:
    """The read `tools/octoprint-env` makes today: its own `command_of`, called as it is."""
    try:
        return command_of(pid)
    except (OSError, subprocess.SubprocessError) as error:
        raise MethodError(str(error)) from error


def measure(pid: ProcessId) -> list[Method]:
    """Every method this host has, over the process `pid`, the current read first."""
    methods = [_time(CURRENT, lambda: _current(pid))]
    if sys.platform != "win32":
        return methods
    methods += [
        _time(name, lambda argv=argv: _read(argv)) if argv else Method(name, present=False)
        for name, argv in _commands(pid).items()
    ]
    methods.append(_time("win32 NtQueryInformationProcess", lambda: _win32_command_line(pid)))
    methods.append(_time("win32 QueryFullProcessImageNameW", lambda: _win32_image(pid)))
    return methods


def main(argv: list[str]) -> int:
    """Measure every method this host has, and write the report."""
    if len(argv) != 1:
        print("usage: lookup_bench.py <report.json>", file=sys.stderr)
        return 2
    out = Path(argv[0])
    child = start([sys.executable, "-c", "import time; time.sleep(900)", MARKER])
    try:
        time.sleep(1)
        methods = measure(child.pid)
    finally:
        child.kill()
        child.wait()
    current = methods[0].answered.replace('"', "")
    methods = [
        replace(
            method,
            same_as_current=bool(method.answered) and method.answered.replace('"', "") == current,
        )
        for method in methods
    ]
    report = LookupReport(rounds=ROUNDS, pid=child.pid, methods=methods)
    out.write_text(json.dumps(asdict(report), indent=1), encoding="utf-8")
    for name in report.failed:
        print(f"lookup_bench: `{name}` did not answer; {out} says why", file=sys.stderr)
    return 1 if report.failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
