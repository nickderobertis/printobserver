"""A `launchctl` for a host that has no launchd: the one boundary the launchd adapter stands in.

`test_service_manager_journey.py` drives its launchd adapter against real
launchd on the macOS cell. On Linux this program is put on that adapter's
`PATH` under the name `launchctl`, so the same walk — the documented
`bootstrap`, the manager's reports, a kill, a `bootout` — runs against a
manager that does to the property list what launchd does with it: it starts
the program the list names with the list's own arguments, environment, working
directory and error log, starts it again after it ends in a way `KeepAlive`
says it should come back from, no sooner than `ThrottleInterval` after the last
start, and stops it on `bootout`. The program it starts is the real
`printobserver`, so what answers the journey is the real service.

Only what the adapter asks of launchd is implemented, answered in launchd's
own words; any other verb is refused with launchd's usage exit rather than
guessed at. Every invocation is recorded, one JSON array per line, so the
journey can read back what it asked of the manager.

Two variables configure it:

- `LAUNCHCTL_STANDIN_ROOT`: the directory this stand-in's machine has at `/`.
  A property list is named by the path the operator's command names —
  `/Library/LaunchDaemons/<label>.plist` — and read from beneath this root, so
  the journey installs into a throwaway root and runs the documented command
  unchanged.
- `LAUNCHCTL_STANDIN_STATE`: where it keeps each loaded job's record, the
  services switched off, and the recording of its invocations.

It runs a job as the user that invoked it and no other, because it cannot
become another user: a property list naming anybody else is refused.
"""

from __future__ import annotations

import contextlib
import dataclasses
import getpass
import json
import os
import plistlib
import signal
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import IO

from repo_checks.shell import start

ROOT = "LAUNCHCTL_STANDIN_ROOT"
STATE = "LAUNCHCTL_STANDIN_STATE"
RECORDING = "invocations.jsonl"

#: Every label `launchctl disable` switched off, beneath the state directory.
DISABLED = "disabled.json"

#: Every property-list key this stand-in acts on. A list carrying any other is
#: refused rather than half-honoured, so an installer that starts writing a key
#: launchd would act on fails this walk until the stand-in implements it.
KEYS = frozenset(
    {
        "Label",
        "ProgramArguments",
        "UserName",
        "WorkingDirectory",
        "EnvironmentVariables",
        "StandardErrorPath",
        "RunAtLoad",
        "KeepAlive",
        "ThrottleInterval",
    }
)

#: The one domain a system daemon is loaded into.
DOMAIN = "system"

#: launchd's own exits for what this stand-in answers.
USAGE = 64
NO_SUCH_PROCESS = 3
INPUT_OUTPUT_ERROR = 5
NOT_FOUND = 113

#: launchd's default for a property list that leaves `ThrottleInterval` out.
DEFAULT_THROTTLE_SECONDS = 10
EXIT_TIMEOUT_SECONDS = 20
TICK_SECONDS = 0.2


@dataclass(frozen=True, slots=True)
class Job:
    """The keys of a property list this stand-in acts on, each checked for its type."""

    label: str
    arguments: tuple[str, ...]
    user: str
    working_directory: Path | None
    environment: dict[str, str]
    error_log: Path | None
    run_at_load: bool
    keep_alive: bool | dict[str, bool]
    throttle: float


def _strings(value: object) -> list[str] | None:
    """`value` as a list of strings, or `None` where it is anything else."""
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        return None
    return [str(item) for item in value]


def _job(document: object) -> Job | None:
    """The job a loaded property list describes, or `None` where it is not one launchd runs."""
    if not isinstance(document, dict) or not set(document) <= KEYS:
        return None
    label = document.get("Label")
    arguments = _strings(document.get("ProgramArguments"))
    user = document.get("UserName", getpass.getuser())
    directory = document.get("WorkingDirectory")
    environment = document.get("EnvironmentVariables", {})
    log = document.get("StandardErrorPath")
    keep_alive = document.get("KeepAlive", False)
    run_at_load = document.get("RunAtLoad", False)
    throttle = document.get("ThrottleInterval", DEFAULT_THROTTLE_SECONDS)
    if not isinstance(label, str) or _label_of(f"{DOMAIN}/{label}") is None:
        return None
    if not arguments or not arguments[0] or not isinstance(user, str):
        return None
    if not isinstance(directory, str | None) or not isinstance(log, str | None):
        return None
    if not isinstance(environment, dict) or _strings([*environment, *environment.values()]) is None:
        return None
    if not isinstance(run_at_load, bool) or not isinstance(throttle, int) or throttle < 0:
        return None
    # The two shapes of `KeepAlive` this stand-in implements; any other key is
    # one launchd would act on and this would not, so it is refused.
    if not isinstance(keep_alive, bool) and (
        not isinstance(keep_alive, dict)
        or set(keep_alive) != {"SuccessfulExit"}
        or not isinstance(keep_alive["SuccessfulExit"], bool)
    ):
        return None
    return Job(
        label=label,
        arguments=tuple(arguments),
        user=user,
        working_directory=Path(directory) if directory is not None else None,
        environment={str(key): str(value) for key, value in environment.items()},
        error_log=Path(log) if log is not None else None,
        run_at_load=run_at_load,
        keep_alive=keep_alive
        if isinstance(keep_alive, bool)
        else {"SuccessfulExit": bool(keep_alive["SuccessfulExit"])},
        throttle=float(throttle),
    )


@dataclass(slots=True)
class Record:
    """What `launchctl print` reports of one loaded job, kept by the job's supervisor."""

    path: str
    program: str
    supervisor: int
    pid: int | None = None
    runs: int = 0
    last_exit_code: int | None = None
    last_signal: str | None = None


def _state() -> Path:
    return Path(os.environ[STATE])


def _record_path(label: str) -> Path:
    return _state() / f"{label}.json"


def _record(document: object) -> Record | None:
    """A job's record as its supervisor wrote it, or `None` where it is not one."""
    if not isinstance(document, dict):
        return None
    path, program = document.get("path"), document.get("program")
    supervisor, pid, runs = document.get("supervisor"), document.get("pid"), document.get("runs")
    code, killed = document.get("last_exit_code"), document.get("last_signal")
    if not isinstance(path, str) or not isinstance(program, str):
        return None
    if not isinstance(supervisor, int) or not isinstance(runs, int):
        return None
    if not isinstance(pid, int | None) or not isinstance(code, int | None):
        return None
    if not isinstance(killed, str | None) or supervisor <= 0 or (pid is not None and pid <= 0):
        return None
    return Record(path, program, supervisor, pid, runs, code, killed)


def _read_record(label: str) -> Record | None:
    """A loaded job's record, or `None` where no live supervisor holds one."""
    try:
        record = _record(json.loads(_record_path(label).read_text(encoding="utf-8")))
    except OSError, ValueError:  # the 3.14 form (PEP 758); ruff format writes it
        return None
    if record is None or not _alive(record.supervisor):
        return None
    return record


def _write_record(label: str, record: Record) -> None:
    """Replace a job's record whole, so a reader never sees half of one."""
    written = _record_path(label).with_suffix(".tmp")
    written.write_text(json.dumps(dataclasses.asdict(record)), encoding="utf-8")
    written.replace(_record_path(label))


def _disabled() -> list[str]:
    """Every label switched off in the system domain."""
    try:
        listed = json.loads((_state() / DISABLED).read_text(encoding="utf-8"))
    except OSError, ValueError:
        return []
    if not isinstance(listed, list):
        return []
    return [label for label in listed if isinstance(label, str) and _label_of(f"{DOMAIN}/{label}")]


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
    return label if domain == DOMAIN and label and "/" not in label else None


def _beneath_root(given: str) -> Path | None:
    """Where a path the machine names is beneath the stand-in's root, or `None` outside it."""
    root = Path(os.environ[ROOT]).resolve()
    resolved = (root / given.lstrip("/")).resolve()
    return resolved if resolved.is_relative_to(root) else None


def _load(resolved: Path) -> Job | None:
    try:
        with resolved.open("rb") as handle:
            return _job(plistlib.load(handle))
    except OSError, plistlib.InvalidFileException:
        return None


def _bootstrap(arguments: list[str]) -> int:
    if len(arguments) != 2 or arguments[0] != DOMAIN:
        _say(sys.stderr, "Usage: launchctl bootstrap <domain-target> <path>")
        return USAGE
    given = arguments[1]
    resolved = _beneath_root(given)
    job = _load(resolved) if resolved is not None else None
    if resolved is None or job is None or _read_record(job.label) is not None:
        _say(sys.stderr, f"Bootstrap failed: {INPUT_OUTPUT_ERROR}: Input/output error")
        return INPUT_OUTPUT_ERROR
    if job.user != getpass.getuser():
        _say(
            sys.stderr,
            f"launchctl stand-in: {given} runs as {job.user}, and this stand-in runs a job as "
            f"the user that invoked it ({getpass.getuser()}) and no other",
        )
        return INPUT_OUTPUT_ERROR
    # The supervisor outlives this command, as launchd outlives `launchctl`; its
    # streams are its own pipes, which close when this command exits, so a
    # caller reading this command's output is not held open by it.
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    supervisor = start([sys.executable, __file__, "--supervise", given, str(resolved)])
    deadline = time.monotonic() + EXIT_TIMEOUT_SECONDS
    while _read_record(job.label) is None:
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
    lines = [
        f"{DOMAIN}/{label} = {{",
        f"\tactive count = {1 if record.pid else 0}",
        f"\tpath = {record.path}",
        "\ttype = LaunchDaemon",
        f"\tstate = {'running' if record.pid else 'not running'}",
        f"\tprogram = {record.program}",
        f"\truns = {record.runs}",
    ]
    if record.pid:
        lines.append(f"\tpid = {record.pid}")
    code = record.last_exit_code
    lines.append(f"\tlast exit code = {'(never exited)' if code is None else code}")
    if record.last_signal:
        lines.append(f"\tlast terminating signal = {record.last_signal}")
    lines.append("}")
    _say(sys.stdout, "\n".join(lines))
    return 0


def _print_disabled(arguments: list[str]) -> int:
    if arguments != [DOMAIN]:
        _say(sys.stderr, "Usage: launchctl print-disabled <domain-target>")
        return USAGE
    listed = "".join(f'\t"{label}" => disabled\n' for label in _disabled())
    _say(sys.stdout, f"disabled services = {{\n{listed}}}")
    return 0


def _switch(arguments: list[str], *, off: bool, verb: str) -> int:
    """Add a service to the disabled set, or take it out."""
    label = _label_of(arguments[0]) if len(arguments) == 1 else None
    if label is None:
        _say(sys.stderr, f"Usage: launchctl {verb} <service-target>")
        return USAGE
    labels = [listed for listed in _disabled() if listed != label]
    if off:
        labels.append(label)
    (_state() / DISABLED).write_text(json.dumps(labels), encoding="utf-8")
    return 0


def _disable(arguments: list[str]) -> int:
    return _switch(arguments, off=True, verb="disable")


def _enable(arguments: list[str]) -> int:
    return _switch(arguments, off=False, verb="enable")


def _signal(named: str) -> signal.Signals | None:
    """A signal given by number or by name, with or without `SIG`, or `None` for neither."""
    try:
        if named.isdigit():
            return signal.Signals(int(named))
        return signal.Signals[f"SIG{named.removeprefix('SIG')}"]
    except ValueError, KeyError:
        return None


def _kill(arguments: list[str]) -> int:
    label = _label_of(arguments[1]) if len(arguments) == 2 else None
    chosen = _signal(arguments[0]) if len(arguments) == 2 else None
    if label is None or chosen is None:
        _say(sys.stderr, "Usage: launchctl kill <signal-name|signal-number> <service-target>")
        return USAGE
    record = _read_record(label)
    if record is None or record.pid is None:
        _say(sys.stderr, f"Could not kill service: {NO_SUCH_PROCESS}: No such process")
        return NO_SUCH_PROCESS
    os.kill(record.pid, chosen)
    return 0


def _bootout(arguments: list[str]) -> int:
    label = _label_of(arguments[0]) if len(arguments) == 1 else None
    if label is None:
        _say(sys.stderr, "Usage: launchctl bootout <service-target>")
        return USAGE
    record = _read_record(label)
    if record is None:
        _say(sys.stderr, f"Boot-out failed: {NO_SUCH_PROCESS}: No such process")
        return NO_SUCH_PROCESS
    os.kill(record.supervisor, signal.SIGTERM)
    deadline = time.monotonic() + 2 * EXIT_TIMEOUT_SECONDS
    while _alive(record.supervisor):
        if time.monotonic() > deadline:
            _say(sys.stderr, f"Boot-out failed: {INPUT_OUTPUT_ERROR}: Input/output error")
            return INPUT_OUTPUT_ERROR
        time.sleep(TICK_SECONDS)
    return 0


def _restarts(keep_alive: bool | dict[str, bool], exit_code: int) -> bool:
    """Whether launchd starts a job again after it ended with `exit_code`.

    A process killed by a signal ends with a negative code here, which launchd
    counts as an unsuccessful exit.
    """
    match keep_alive:
        case bool():
            return keep_alive
        case {"SuccessfulExit": successful}:
            return (exit_code == 0) == successful
        case _:
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


def _start_job(job: Job) -> subprocess.Popen[str]:
    """Start the program a job names, with its own arguments, directory and environment."""
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    return start(list(job.arguments), cwd=job.working_directory, env=dict(job.environment))


def _supervise(given: str, resolved: Path) -> int:
    """Be launchd for one job until it is booted out."""
    job = _load(resolved) if _beneath_root(given) == resolved else None
    if job is None:
        _say(
            sys.stderr, f"launchctl stand-in: {resolved} is not {given} beneath the machine's root"
        )
        return INPUT_OUTPUT_ERROR
    stopping = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stopping.set())
    record = Record(path=given, program=job.arguments[0], supervisor=os.getpid())
    child: subprocess.Popen[str] | None = None
    last_start = float("-inf")
    wanted = job.run_at_load
    try:
        _write_record(job.label, record)
        while not stopping.is_set():
            if child is not None and (code := child.poll()) is not None:
                record.pid = None
                record.last_exit_code = code if code >= 0 else None
                record.last_signal = f"{signal.strsignal(-code)}: {-code}" if code < 0 else None
                _write_record(job.label, record)
                child = None
                wanted = _restarts(job.keep_alive, code)
            if child is None and wanted and time.monotonic() - last_start >= job.throttle:
                child = _start_job(job)
                last_start = time.monotonic()
                for stream, log in ((child.stdout, None), (child.stderr, job.error_log)):
                    if stream is not None:
                        threading.Thread(target=_drain, args=(stream, log), daemon=True).start()
                record.pid = child.pid
                record.runs += 1
                _write_record(job.label, record)
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
            _record_path(job.label).unlink()
    return 0


VERBS = {
    "bootstrap": _bootstrap,
    "bootout": _bootout,
    "disable": _disable,
    "enable": _enable,
    "print": _print,
    "print-disabled": _print_disabled,
    "kill": _kill,
}


def main(argv: list[str]) -> int:
    """Answer one `launchctl` invocation, recording it first."""
    for variable in (ROOT, STATE):
        if not Path(os.environ.get(variable, "")).is_absolute():
            _say(sys.stderr, f"launchctl stand-in: {variable} must name an absolute directory")
            return USAGE
    if argv[:1] == ["--supervise"]:
        if len(argv) != 3:
            _say(sys.stderr, "Usage: launchctl_standin.py --supervise <given-path> <plist>")
            return USAGE
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
