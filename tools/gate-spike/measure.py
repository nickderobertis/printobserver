"""One measurement of the gate's cost: one experiment over one target set.

`spike-windows-gate-breakdown`'s harness. It runs each named Nx task's own
command — read from the project's `project.json`, so it is the command Nx would
run — one task at a time, under one experiment's levers (`levers.py`), and
writes one `Report` as JSON of what each task cost (`figures.py`):

* every task's wall time, exit status and the free disk around it;
* for a nextest task, cargo's compile time, nextest's run time, its test and
  binary counts and every test's own duration, and — where the timed linker
  could stand in front of the real one — every link cargo made;
* for a pytest task, every test's duration and what it spent it on, from
  `spike_pytest.py`;
* for the client-command journeys, every operation `support/spike.rs` timed.

Usage (from the repository root, after `just bootstrap`):

    just spike-measure baseline printobserver:test,repo-checks:test /tmp/spike-out

`EXPERIMENT` is one or more levers joined by `+`; `TARGETS` is `project:target`
tasks or pseudo-targets joined by `,`, where `*:<target>` is every project that
declares `<target>`; `--list` names every lever and pseudo-target. The report
is `<out>/measure.json`, `<out>` being outside the repository, and each task's
own output is beside it as `<out>/<task>.log`. Success prints one line naming
the report; a task that failed is named on standard error. Exit status: 0 when
every task succeeded, 1 when any failed (the report is written either way), 2
for an invocation naming a lever, target or output directory it cannot use.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import sys
import tempfile
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, NewType

import figures
import lookup_bench
from levers import (
    LEVERS,
    PYTEST,
    ROOT,
    RUST_LLD,
    WINDOWS,
    DefenderState,
    DiskSpace,
    HostError,
    Lever,
    Preparation,
    clear_pytest_temporaries,
    defender_state,
    disk,
    pytest_temporaries,
)
from repo_checks.shell import run
from spike_names import JOURNEY_TIMINGS, LINK_TIMINGS, PYTEST_OUT, REAL_LINKER

if TYPE_CHECKING:
    from collections.abc import Sequence

HERE = Path(__file__).resolve().parent
TASK = re.compile(r"^(\*|[\w.-]+):[\w-]+$")
PSEUDO_TARGETS = {
    "bench:octoprint-lookup": "time each way of reading which program a process id is",
    "host:defender": "record Defender's state",
}


#: A `project:target` task, or a pseudo-target, that `resolve` found declared.
TaskId = NewType("TaskId", str)


class UsageError(ValueError):
    """An invocation naming a lever, target or project that does not exist."""


@dataclass(frozen=True)
class Declared:
    """One Nx task as its project declares it."""

    task: TaskId
    command: str
    cwd: Path


def _projects() -> dict[str, dict[str, object]]:
    """Every project.json under the repository's project roots, by name."""
    found: dict[str, dict[str, object]] = {}
    for manifest in sorted(ROOT.glob("*/*/project.json")):
        document = json.loads(manifest.read_text(encoding="utf-8"))
        name = document.get("name") if isinstance(document, dict) else None
        targets = document.get("targets") if isinstance(document, dict) else None
        if isinstance(name, str) and isinstance(targets, dict):
            found[name] = targets
    return found


def _declared(project: str, target: str, targets: dict[str, object]) -> Declared:
    declared = targets.get(target)
    command = declared.get("command") if isinstance(declared, dict) else None
    if not isinstance(declared, dict) or not isinstance(command, str):
        msg = f"`{project}` declares no `{target}` command"
        raise UsageError(msg)
    options = declared.get("options")
    cwd = options.get("cwd") if isinstance(options, dict) else None
    if cwd is not None and not isinstance(cwd, str):
        msg = f"`{project}:{target}` names a cwd that is not a path: {cwd!r}"
        raise UsageError(msg)
    return Declared(TaskId(f"{project}:{target}"), command, ROOT / cwd if cwd else ROOT)


def resolve(targets: Sequence[str]) -> list[TaskId]:
    """Each target as the tasks it names, refusing one that names nothing."""
    projects = _projects()
    tasks: list[TaskId] = []
    for target in targets:
        if target in PSEUDO_TARGETS:
            tasks.append(TaskId(target))
            continue
        if not TASK.match(target):
            msg = f"`{target}` is neither `project:target` nor a pseudo-target"
            raise UsageError(msg)
        project, name = target.split(":", 1)
        if project == "*":
            every = [found for found, declared in projects.items() if name in declared]
            if not every:
                msg = f"no project declares `{name}`"
                raise UsageError(msg)
            tasks.extend(TaskId(f"{found}:{name}") for found in every)
            continue
        if project not in projects:
            msg = f"no project.json names a project `{project}`"
            raise UsageError(msg)
        tasks.append(_declared(project, name, projects[project]).task)
    return tasks


def _with_spike_plugin(command: str) -> str:
    """Load the timing plugin into every pytest the command starts, and ask for all durations."""
    return PYTEST.sub(lambda found: f"{found.group(1)}-p spike_pytest --durations=0 ", command)


@dataclass(frozen=True)
class LinkerSetup:
    """Whether the timed linker stands in front of this target's linker, and why not."""

    used: bool
    variable: str = ""
    real: str = ""
    why: str = ""


def _linker_variable() -> str:
    host = run(["rustc", "-vV"], check=True).stdout
    target = next(line.split(": ", 1)[1] for line in host.splitlines() if line.startswith("host: "))
    return "CARGO_TARGET_" + re.sub(r"[-.]", "_", target).upper() + "_LINKER"


def _timed_linker(out: Path, env: dict[str, str]) -> LinkerSetup:
    """Put the timed linker in front of this target's linker, when a smoke link proves it works."""
    wrapper = out / "timed-linker"
    wrapper.mkdir(parents=True, exist_ok=True)
    binary = wrapper / ("timed-linker.exe" if WINDOWS else "timed-linker")
    built = run(
        ["rustc", "--edition", "2021", "-O", str(HERE / "timed_linker.rs"), "-o", str(binary)]
    )
    if built.returncode != 0:
        return LinkerSetup(used=False, why="the wrapper did not build: " + built.stderr[-2000:])
    real = env.get(REAL_LINKER) or os.environ.get(REAL_LINKER) or ("link.exe" if WINDOWS else "cc")
    variable = _linker_variable()
    timed = {variable: str(binary), REAL_LINKER: real}
    # Outside the repository: a crate made inside it would join its workspace.
    smoke = Path(tempfile.mkdtemp(prefix="spike-smoke-")) / "smoke"
    created = run(["cargo", "new", "--quiet", "--vcs", "none", str(smoke)])
    linked = run(
        ["cargo", "build", "--quiet", "--manifest-path", str(smoke / "Cargo.toml")],
        env={
            **os.environ,
            **env,
            **timed,
            LINK_TIMINGS: str(out / "smoke-links.jsonl"),
            "CARGO_TARGET_DIR": str(smoke / "target"),
        },
    )
    if created.returncode != 0 or linked.returncode != 0:
        why = "the smoke link failed: " + (created.stderr + linked.stderr)[-2000:]
        return LinkerSetup(used=False, why=why)
    env.update({**timed, LINK_TIMINGS: str(out / "links.jsonl")})
    return LinkerSetup(used=True, variable=variable, real=real)


@dataclass(frozen=True)
class TaskDisk:
    """Free disk before and after one task, and what removing its pytest temporaries freed."""

    before: tuple[DiskSpace, ...]
    after: tuple[DiskSpace, ...]
    freed_after_gb: float


@dataclass
class TaskResult:
    """What one task cost, with whichever figures its output carried."""

    task: TaskId
    s: float
    exit: int
    command: str = ""
    cwd: str = ""
    disk: TaskDisk | None = None
    rust: figures.RustFigures | None = None
    pytest: figures.PytestFigures | None = None
    journey: figures.JourneyFigures | None = None
    lookup: lookup_bench.LookupReport | None = None
    defender: DefenderState | None = None


def _slug(task: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]", "_", task)


def _bench(task: TaskId, out: Path) -> TaskResult:
    report = out / _slug(task) / "lookup.json"
    report.parent.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    bench = run([sys.executable, str(HERE / "lookup_bench.py"), str(report)])
    (out / f"{_slug(task)}.log").write_text(bench.stdout + bench.stderr, encoding="utf-8")
    result = TaskResult(task, round(time.perf_counter() - started, 3), bench.returncode)
    if report.is_file():
        result.lookup = lookup_bench.LookupReport.read(report)
    return result


def run_task(task: TaskId, levers: Sequence[Lever], env: dict[str, str], out: Path) -> TaskResult:
    """Run one task under the levers, and answer what it cost."""
    if task == "bench:octoprint-lookup":
        return _bench(task, out)
    if task == "host:defender":
        return TaskResult(task, 0.0, 0, defender=defender_state())
    project, target = task.split(":", 1)
    declared = _declared(project, target, _projects()[project])
    command = declared.command
    for lever in levers:
        if lever.rewrite is not None:
            command = lever.rewrite(command)
    command = _with_spike_plugin(command)
    task_dir = out / _slug(task)
    task_dir.mkdir(parents=True, exist_ok=True)
    timings = task_dir / "journey-timings.jsonl"
    log = out / f"{_slug(task)}.log"
    log.write_text(f"$ {command}\n", encoding="utf-8")
    task_env = {
        **os.environ,
        **env,
        PYTEST_OUT: str(task_dir),
        JOURNEY_TIMINGS: str(timings),
        "PYTHONPATH": os.pathsep.join(filter(None, [str(HERE), os.environ.get("PYTHONPATH", "")])),
        # Shown live in the job's log as well as kept: `tee` under `pipefail`,
        # so the exit status is the command's own.
        "SPIKE_COMMAND": command,
        "SPIKE_LOG": log.as_posix(),
    }
    if "llvm-cov" in command:
        # The clean `just test` makes before a covered run.
        run(["cargo", "llvm-cov", "clean", "--workspace"], cwd=ROOT, capture=False)
    started = time.perf_counter()
    ran = run(
        ["bash", "-o", "pipefail", "-c", 'eval "$SPIKE_COMMAND" 2>&1 | tee -a "$SPIKE_LOG"'],
        cwd=declared.cwd,
        env=task_env,
        capture=False,
    )
    seconds = round(time.perf_counter() - started, 3)
    text = log.read_text(encoding="utf-8", errors="replace")
    return TaskResult(
        task=task,
        s=seconds,
        exit=ran.returncode,
        command=command,
        cwd=str(declared.cwd),
        rust=figures.rust_figures(text),
        pytest=figures.pytest_figures(task_dir),
        journey=figures.journey_figures(timings),
    )


@dataclass(frozen=True)
class Host:
    """The machine a report was measured on."""

    platform: str
    machine: str
    processors: int | None
    runner: str | None


@dataclass
class Report:
    """One experiment over one target set."""

    experiment: list[str]
    levers: dict[str, str]
    env: dict[str, str]
    host: Host
    defender_before: DefenderState
    prepared: list[Preparation] = field(default_factory=list)
    linker: LinkerSetup = field(default_factory=lambda: LinkerSetup(used=False))
    tasks: list[TaskResult] = field(default_factory=list)
    links: figures.LinkFigures | None = None
    wall_s: float = 0.0


def experiment(names: Sequence[str], tasks: Sequence[TaskId], out: Path) -> Report:
    """Pull the levers, run every task, and answer the report.

    A lever with a `restore` (`base-tree`) is put back however the run ended;
    the host levers (`defender-*`, `temp-on-d`) change a CI cell's own runner,
    which the job discards, and are left as they are.
    """
    levers = [LEVERS[name] for name in names]
    env: dict[str, str] = {}
    report = Report(
        experiment=list(names),
        levers={name: LEVERS[name].description for name in names},
        env=env,
        host=Host(
            platform=os.environ.get("PRINTOBSERVER_PLATFORM", sys.platform),
            machine=platform.machine(),
            processors=os.cpu_count(),
            runner=os.environ.get("RUNNER_NAME"),
        ),
        defender_before=defender_state(),
    )
    started = time.perf_counter()
    try:
        for name, lever in zip(names, levers, strict=True):
            env.update(lever.env)
            if lever.prepare is not None:
                report.prepared.append(lever.prepare(name))
        report.linker = _timed_linker(out, env)
        if os.environ.get(REAL_LINKER) == RUST_LLD and not report.linker.used:
            # Untimed, rather than not at all: the lever's build is still measured.
            env[_linker_variable()] = RUST_LLD
        for task in tasks:
            temporaries = pytest_temporaries()
            before = disk()
            result = run_task(task, levers, env, out)
            after = disk()
            result.disk = TaskDisk(before, after, clear_pytest_temporaries(temporaries, out))
            report.tasks.append(result)
            if result.exit != 0:
                log = out / f"{_slug(task)}.log"
                print(f"measure: {task} exited {result.exit}; see {log}", file=sys.stderr)
    finally:
        for lever in levers:
            if lever.restore is not None:
                lever.restore()
    report.links = figures.link_figures(out / "links.jsonl")
    report.wall_s = round(time.perf_counter() - started, 3)
    return report


def main(argv: Sequence[str] | None = None) -> int:
    """Run one experiment over one target set and write `<out>/measure.json`."""
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--experiment", default="baseline", help="levers joined by `+`")
    parser.add_argument("--targets", default="", help="tasks or pseudo-targets joined by `,`")
    parser.add_argument("--out", type=Path, default=Path(tempfile.gettempdir()) / "spike-out")
    parser.add_argument("--list", action="store_true", help="name every lever and pseudo-target")
    arguments = parser.parse_args(argv)
    if arguments.list:
        for name, lever in LEVERS.items():
            print(f"lever  {name:20} {lever.description}")
        for name, description in PSEUDO_TARGETS.items():
            print(f"target {name:30} {description}")
        return 0
    names = [name for name in arguments.experiment.split("+") if name]
    try:
        unknown = [name for name in names if name not in LEVERS]
        if unknown or not names:
            msg = f"no lever is named {unknown or names}"
            raise UsageError(msg)
        named = [target.strip() for target in arguments.targets.split(",") if target.strip()]
        tasks = resolve(named)
        if not tasks:
            msg = "no target was named"
            raise UsageError(msg)
    except UsageError as error:
        print(f"measure: {error}; `--list` names every lever and pseudo-target", file=sys.stderr)
        return 2
    out = arguments.out.resolve()
    if out.is_relative_to(ROOT.resolve()):
        print(
            f"measure: {out} is inside the repository, whose tree the suites measured copy "
            "and read; name a directory outside it",
            file=sys.stderr,
        )
        return 2
    out.mkdir(parents=True, exist_ok=True)
    try:
        report = experiment(names, tasks, out)
    except HostError as error:
        print(
            f"measure: the host could not be prepared: {error}. Run the experiment without that "
            "lever, or on a host that has the program it names (a GitHub-hosted runner of the "
            "platform the lever is for)",
            file=sys.stderr,
        )
        return 1
    (out / "measure.json").write_text(json.dumps(asdict(report), indent=1), encoding="utf-8")
    failed = sum(1 for task in report.tasks if task.exit != 0)
    print(
        f"measure: {len(report.tasks)} tasks in {report.wall_s:.0f}s, {failed} failed: "
        f"{out / 'measure.json'}"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
