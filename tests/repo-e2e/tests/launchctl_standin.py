"""A `launchctl` for a host that has no launchd: the one boundary the launchd back end stands in.

`test_service_manager_journey.py` drives its launchd back end against real
launchd on the macOS cell. Everywhere else this program is put on that back
end's `PATH` under the name `launchctl`, so the same walk — the documented
`bootstrap`, the manager's reports, a kill, a `bootout` — runs against a
manager that does to the property list what launchd does with it: it starts
the program the list names with the list's own arguments, environment, working
directory and error log, starts it again after it ends in a way `KeepAlive`
says it should come back from, no sooner than `ThrottleInterval` after the last
start, and stops it on `bootout`. The program it starts is the real
`printobserver`, so what answers the journey is the real service.

Only what the back end asks of launchd is implemented, answered in launchd's
own words; any other verb is refused with launchd's usage exit rather than
guessed at. Every invocation is recorded, one JSON array per line, so the
journey can read back what it asked of the manager.

Two variables configure it:

- `LAUNCHCTL_STANDIN_ROOT`: the directory this stand-in's machine has at `/`.
  A property list is named by the path the operator's command names —
  `/Library/LaunchDaemons/<label>.plist` — and read from beneath this root, so
  the journey installs into a throwaway root and runs the documented command
  unchanged.
- `LAUNCHCTL_STANDIN_STATE`: where it keeps each loaded job's record and the
  recording of its invocations.

It runs a job as the user that invoked it and no other, because it cannot
become another user: a property list naming anybody else is refused.
"""

from __future__ import annotations

import contextlib
import getpass
import json
import os
import plistlib
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import IO

from repo_checks.shell import start

#: The variable naming the stand-in machine's root.
ROOT = "LAUNCHCTL_STANDIN_ROOT"

#: The variable naming where the stand-in keeps its records.
STATE = "LAUNCHCTL_STANDIN_STATE"

#: The file every invocation is recorded in, beneath the state directory.
RECORDING = "invocations.jsonl"

#: The one domain a system daemon is loaded into.
DOMAIN = "system"

#: launchd's own exits for what this stand-in answers.
USAGE = 64
NO_SUCH_PROCESS = 3
INPUT_OUTPUT_ERROR = 5
NOT_FOUND = 113

#: launchd's defaults for the keys a property list may leave out.
DEFAULT_THROTTLE_SECONDS = 10
EXIT_TIMEOUT_SECONDS = 20

#: How often the supervisor looks at its job.
TICK_SECONDS = 0.2


def _state() -> Path:
    return Path(os.environ[STATE])


def _record_path(label: str) -> Path:
    return _state() / f"{label}.json"


def _read_record(label: str) -> dict[str, object] | None:
    """A loaded job's record, or `None` where no live supervisor holds one."""
    try:
        record = json.loads(_record_path(label).read_text(encoding="utf-8"))
    except OSError, ValueError:  # the 3.14 form (PEP 758); ruff format writes it
        return None
    supervisor = record.get("supervisor")
    if not isinstance(supervisor, int) or not _alive(supervisor):
        return None
    return record


def _write_record(label: str, record: dict[str, object]) -> None:
    """Replace a job's record whole, so a reader never sees half of one."""
    written = _record_path(label).with_suffix(".tmp")
    written.write_text(json.dumps(record), encoding="utf-8")
    written.replace(_record_path(label))


def _alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def _say(stream: IO[str], text: str) -> None:
    stream.write(f"{text}\n")


def _label_of(target: str) -> str | None:
    """The label a `system/<label>` service target names, or `None` for any other domain."""
    domain, _, label = target.partition("/")
    return label if domain == DOMAIN and label else None


def _bootstrap(arguments: list[str]) -> int:
    if len(arguments) != 2 or arguments[0] != DOMAIN:
        _say(sys.stderr, "Usage: launchctl bootstrap <domain-target> <path>")
        return USAGE
    given = arguments[1]
    resolved = Path(os.environ[ROOT]) / given.lstrip("/")
    try:
        with resolved.open("rb") as handle:
            job = plistlib.load(handle)
    except OSError, plistlib.InvalidFileException:
        _say(sys.stderr, f"Bootstrap failed: {INPUT_OUTPUT_ERROR}: Input/output error")
        return INPUT_OUTPUT_ERROR
    label = job.get("Label")
    if not isinstance(label, str) or _read_record(label) is not None:
        _say(sys.stderr, f"Bootstrap failed: {INPUT_OUTPUT_ERROR}: Input/output error")
        return INPUT_OUTPUT_ERROR
    user = job.get("UserName", getpass.getuser())
    if user != getpass.getuser():
        _say(
            sys.stderr,
            f"launchctl stand-in: {given} runs as {user}, and this stand-in runs a job as the "
            f"user that invoked it ({getpass.getuser()}) and no other",
        )
        return INPUT_OUTPUT_ERROR
    # The supervisor outlives this command, as launchd outlives `launchctl`; its
    # streams are its own pipes, which close when this command exits, so a
    # caller reading this command's output is not held open by it.
    supervisor = start([sys.executable, __file__, "--supervise", given, str(resolved)])
    deadline = time.monotonic() + EXIT_TIMEOUT_SECONDS
    while _read_record(label) is None:
        if supervisor.poll() is not None or time.monotonic() > deadline:
            _say(sys.stderr, f"Bootstrap failed: {INPUT_OUTPUT_ERROR}: Input/output error")
            return INPUT_OUTPUT_ERROR
        time.sleep(TICK_SECONDS)
    return 0


def _print(arguments: list[str]) -> int:
    label = _label_of(arguments[0]) if len(arguments) == 1 else None
    if label is None:
        _say(sys.stderr, "Usage: launchctl print <service-target>")
        return USAGE
    record = _read_record(label)
    if record is None:
        _say(sys.stderr, f'Could not find service "{label}" in domain for system')
        return NOT_FOUND
    pid = record.get("pid")
    lines = [
        f"{DOMAIN}/{label} = {{",
        f"\tactive count = {1 if pid else 0}",
        f"\tpath = {record['path']}",
        "\ttype = LaunchDaemon",
        f"\tstate = {'running' if pid else 'not running'}",
        f"\tprogram = {record['program']}",
        f"\truns = {record['runs']}",
    ]
    if pid:
        lines.append(f"\tpid = {pid}")
    exit_code = record.get("last_exit_code")
    lines.append(f"\tlast exit code = {'(never exited)' if exit_code is None else exit_code}")
    if record.get("last_signal"):
        lines.append(f"\tlast terminating signal = {record['last_signal']}")
    lines.append("}")
    _say(sys.stdout, "\n".join(lines))
    return 0


def _print_disabled(arguments: list[str]) -> int:
    if arguments != [DOMAIN]:
        _say(sys.stderr, "Usage: launchctl print-disabled <domain-target>")
        return USAGE
    # Nothing is ever switched off here: `launchctl disable` is no verb of this stand-in.
    _say(sys.stdout, "disabled services = {\n}")
    return 0


def _kill(arguments: list[str]) -> int:
    label = _label_of(arguments[1]) if len(arguments) == 2 else None
    if label is None:
        _say(sys.stderr, "Usage: launchctl kill <signal-name|signal-number> <service-target>")
        return USAGE
    name = arguments[0].removeprefix("SIG")
    number = int(name) if name.isdigit() else getattr(signal, f"SIG{name}", None)
    record = _read_record(label)
    pid = record.get("pid") if record is not None else None
    if number is None or not isinstance(pid, int):
        _say(sys.stderr, f"Could not kill service: {NO_SUCH_PROCESS}: No such process")
        return NO_SUCH_PROCESS
    os.kill(pid, number)
    return 0


def _bootout(arguments: list[str]) -> int:
    label = _label_of(arguments[0]) if len(arguments) == 1 else None
    if label is None:
        _say(sys.stderr, "Usage: launchctl bootout <service-target>")
        return USAGE
    record = _read_record(label)
    supervisor = record.get("supervisor") if record is not None else None
    if not isinstance(supervisor, int):
        _say(sys.stderr, f"Boot-out failed: {NO_SUCH_PROCESS}: No such process")
        return NO_SUCH_PROCESS
    os.kill(supervisor, signal.SIGTERM)
    deadline = time.monotonic() + 2 * EXIT_TIMEOUT_SECONDS
    while _alive(supervisor) and time.monotonic() < deadline:
        time.sleep(TICK_SECONDS)
    return 0


def _restarts(keep_alive: object, exit_code: int) -> bool:
    """Whether launchd starts a job again after it ended with `exit_code`.

    A process killed by a signal ends with a negative code here, which launchd
    counts as an unsuccessful exit.
    """
    if isinstance(keep_alive, bool):
        return keep_alive
    if isinstance(keep_alive, dict) and "SuccessfulExit" in keep_alive:
        return (exit_code == 0) == bool(keep_alive["SuccessfulExit"])
    return False


def _drain(stream: IO[str], log: Path | None) -> None:
    """Copy a job's stream into the file its property list names, as launchd does."""
    if log is None:
        for _ in stream:
            pass
        return
    with log.open("a", encoding="utf-8") as sink:
        for line in stream:
            sink.write(line)
            sink.flush()


def _supervise(given: str, resolved: Path) -> int:
    """Be launchd for one job until it is booted out."""
    with resolved.open("rb") as handle:
        job = plistlib.load(handle)
    label: str = job["Label"]
    arguments: list[str] = job["ProgramArguments"]
    throttle = float(job.get("ThrottleInterval", DEFAULT_THROTTLE_SECONDS))
    error_log = job.get("StandardErrorPath")
    stopping = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stopping.set())
    record: dict[str, object] = {
        "path": given,
        "program": arguments[0],
        "supervisor": os.getpid(),
        "pid": None,
        "runs": 0,
        "last_exit_code": None,
        "last_signal": None,
    }
    child = None
    last_start = float("-inf")
    wanted = bool(job.get("RunAtLoad", False))
    try:
        _write_record(label, record)
        while not stopping.is_set():
            if child is not None and (code := child.poll()) is not None:
                record["pid"] = None
                record["last_exit_code"] = code if code >= 0 else None
                record["last_signal"] = f"{signal.strsignal(-code)}: {-code}" if code < 0 else None
                _write_record(label, record)
                child = None
                wanted = _restarts(job.get("KeepAlive", False), code)
            if child is None and wanted and time.monotonic() - last_start >= throttle:
                child = start(
                    list(arguments),
                    cwd=Path(job["WorkingDirectory"]) if "WorkingDirectory" in job else None,
                    env=dict(job.get("EnvironmentVariables", {})),
                )
                last_start = time.monotonic()
                for stream, log in ((child.stdout, None), (child.stderr, error_log)):
                    if stream is not None:
                        threading.Thread(
                            target=_drain,
                            args=(stream, Path(log) if isinstance(log, str) else None),
                            daemon=True,
                        ).start()
                record["pid"] = child.pid
                record["runs"] = int(str(record["runs"])) + 1
                _write_record(label, record)
            stopping.wait(TICK_SECONDS)
        if child is not None:
            child.terminate()
            try:
                child.wait(EXIT_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
    finally:
        with contextlib.suppress(OSError):
            _record_path(label).unlink()
    return 0


VERBS = {
    "bootstrap": _bootstrap,
    "bootout": _bootout,
    "print": _print,
    "print-disabled": _print_disabled,
    "kill": _kill,
}


def main(argv: list[str]) -> int:
    """Answer one `launchctl` invocation, recording it first."""
    if argv[:1] == ["--supervise"]:
        return _supervise(argv[1], Path(argv[2]))
    _state().mkdir(parents=True, exist_ok=True)
    with (_state() / RECORDING).open("a", encoding="utf-8") as recording:
        recording.write(json.dumps(argv) + "\n")
    verb = VERBS.get(argv[0]) if argv else None
    if verb is None:
        _say(sys.stderr, f"Unrecognized subcommand: {argv[0] if argv else ''}")
        return USAGE
    return verb(argv[1:])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
