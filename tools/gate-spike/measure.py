"""One measurement of the gate's cost: one experiment over one target set.

`spike-windows-gate-breakdown`'s harness. It runs each named Nx task's own
command — read from the project's `project.json`, so it is the command Nx would
run — one task at a time, under one experiment's levers, and writes one JSON
document of what each task cost:

* every task's wall time and exit status;
* for a nextest task, cargo's compile time, nextest's run time, its test and
  binary counts, every test's own duration, and — where the timed linker could
  be put in front of the real one — every link cargo made;
* for a pytest task, every test's duration and what it spent it on (child
  processes by program, tree copies and removals, sleeps, HTTP requests), from
  `spike_pytest.py`;
* for the client-command journeys, every operation `support/spike.rs` timed.

Usage (from the repository root, after `just bootstrap`):

    uv run -q python tools/gate-spike/measure.py \
        --experiment baseline --targets printobserver:test,repo-checks:test \
        --out spike-out

`--experiment` takes one or more levers joined by `+`, and `--list` names every
lever and pseudo-target. The document is `<out>/measure.json`; each task's own
output is beside it as `<out>/<task>.log`. The exit status is 0 when every task
succeeded, 1 when any failed (the document is written either way), and 2 for an
invocation naming a lever or target that does not exist.
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
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Any

from repo_checks.shell import run

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence
    from subprocess import CompletedProcess

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
WINDOWS = sys.platform == "win32"


@dataclass(frozen=True)
class Lever:
    """One change to how a task runs, and nothing else."""

    description: str
    env: dict[str, str] = field(default_factory=dict)
    rewrite: Callable[[str, str], str] | None = None
    prepare: Callable[[Path], dict[str, Any]] | None = None


def _replace(old: str, new: str) -> Callable[[str, str], str]:
    return lambda command, _task: command.replace(old, new)


def _no_rust_coverage(command: str, _task: str) -> str:
    return command.replace("cargo llvm-cov nextest", "cargo nextest run").replace(
        " --no-report", ""
    )


def _compile_only(command: str, _task: str) -> str:
    """Build and list every test binary, and run none: nextest's `none()` filter."""
    if '-E "' in command:
        return re.sub(r'-E "([^"]*)"', r'-E "none() & (\1)"', command, count=1)
    return command.replace("--no-tests=pass", '--no-tests=pass -E "none()"', 1)


def _xdist(workers: int) -> Callable[[str, str], str]:
    def rewrite(command: str, _task: str) -> str:
        # `coverage run` alone would measure only the controller, which runs no
        # test under xdist; with SPIKE_WORKER_COVERAGE set, `spike_pytest.py`
        # measures each worker instead, the way pytest-cov does.
        command = command.replace("uv run -q coverage run -m pytest", "uv run -q python -m pytest")
        return command.replace("-m pytest ", f"-m pytest -n {workers} --dist load ", 1)

    return rewrite


def _powershell(script: str) -> CompletedProcess[str]:
    return run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script])


def defender_state(_out: Path | None = None) -> dict[str, Any]:
    """What Defender's real-time scanning is doing on this host, and what it excludes."""
    if not WINDOWS:
        return {"applies": False}
    status = _powershell(
        "$s = Get-MpComputerStatus; $p = Get-MpPreference; "
        "@{realtime=$s.RealTimeProtectionEnabled; antivirus=$s.AntivirusEnabled; "
        "mode=$s.AMRunningMode; excluded=@($p.ExclusionPath); "
        "excluded_processes=@($p.ExclusionProcess)} | ConvertTo-Json -Compress"
    )
    try:
        return {"applies": True, **json.loads(status.stdout)}
    except json.JSONDecodeError:
        return {"applies": True, "unreadable": status.stdout + status.stderr}


def _excluded_paths() -> list[str]:
    home = Path.home()
    uv_cache = run(["uv", "cache", "dir"]).stdout.strip()
    paths = [str(ROOT), str(home / ".cargo"), str(home / ".rustup"), os.environ.get("TEMP", "")]
    return [path for path in [*paths, uv_cache] if path]


def _exclude_from_defender(_out: Path) -> dict[str, Any]:
    if not WINDOWS:
        return {"applies": False}
    paths = _excluded_paths()
    quoted = ",".join("'" + path.replace("'", "''") + "'" for path in paths)
    added = _powershell(f"Add-MpPreference -ExclusionPath {quoted}")
    return {
        "asked": paths,
        "exit": added.returncode,
        "said": added.stderr.strip(),
        "after": defender_state(),
    }


def _defender_off(_out: Path) -> dict[str, Any]:
    if not WINDOWS:
        return {"applies": False}
    off = _powershell("Set-MpPreference -DisableRealtimeMonitoring $true")
    return {"exit": off.returncode, "said": off.stderr.strip(), "after": defender_state()}


def _rust_target() -> str:
    host = run(["rustc", "-vV"], check=True).stdout
    return next(line.split(": ", 1)[1] for line in host.splitlines() if line.startswith("host: "))


def _linker_variable() -> str:
    return "CARGO_TARGET_" + re.sub(r"[-.]", "_", _rust_target()).upper() + "_LINKER"


LEVERS: dict[str, Lever] = {
    "baseline": Lever("each task's own command, unchanged"),
    "no-rust-coverage": Lever(
        "`cargo llvm-cov nextest` replaced by `cargo nextest run`: no instrumentation",
        rewrite=_no_rust_coverage,
    ),
    "no-py-coverage": Lever(
        "`coverage run -m pytest` replaced by `python -m pytest`",
        rewrite=_replace("uv run -q coverage run -m pytest", "uv run -q python -m pytest"),
    ),
    "sysmon": Lever("Python coverage on sys.monitoring", env={"COVERAGE_CORE": "sysmon"}),
    "defender-excluded": Lever(
        "Defender excludes the workspace, ~/.cargo, ~/.rustup, %TEMP% and the uv cache",
        prepare=_exclude_from_defender,
    ),
    "defender-off": Lever("Defender real-time scanning off", prepare=_defender_off),
    "compile-only": Lever("build and list every test binary, run no test", rewrite=_compile_only),
    "line-tables-only": Lever(
        "dev/test debuginfo line tables only", env={"CARGO_PROFILE_DEV_DEBUG": "line-tables-only"}
    ),
    "debug-0": Lever("dev/test debuginfo off", env={"CARGO_PROFILE_DEV_DEBUG": "0"}),
    "incremental-on": Lever("cargo incremental compilation on", env={"CARGO_INCREMENTAL": "1"}),
    "split-unpacked": Lever(
        "split-debuginfo unpacked (not supported on windows-msvc, which only packs)",
        env={"CARGO_PROFILE_DEV_SPLIT_DEBUGINFO": "unpacked"},
    ),
    "rust-lld": Lever("link with the toolchain's own rust-lld"),
}
for _workers in (2, 3, 4):
    LEVERS[f"xdist-{_workers}"] = Lever(
        f"pytest-xdist at {_workers} workers, each worker under Python coverage",
        env={"SPIKE_WORKER_COVERAGE": "1"},
        rewrite=_xdist(_workers),
    )
    LEVERS[f"xdist-{_workers}-nocov"] = Lever(
        f"pytest-xdist at {_workers} workers, no Python coverage", rewrite=_xdist(_workers)
    )

PSEUDO_TARGETS = {
    "bench:octoprint-lookup": "time each way of reading which program a process id is (Windows)",
    "host:defender": "record Defender's state",
}

NEXTEST_RESULT = re.compile(
    r"^\s*(?:\S+:\s+)?(PASS|FAIL|FLAKY|TIMEOUT|SIGSEGV|SIGABRT|SIGKILL|ABORT|LEAK)\s+\[\s*([\d.]+)s\]\s+"
    r"(?:\(\s*\d+/\d+\)\s+)?(\S+)\s+(.+?)\s*$"
)
CARGO_FINISHED = re.compile(r"Finished `[^`]+` profile .* target\(s\) in (?:(\d+)m )?([\d.]+)s")
NEXTEST_STARTING = re.compile(r"Starting (\d+) tests? across (\d+) binar")
NEXTEST_SUMMARY = re.compile(r"Summary \[\s*([\d.]+)s\]")


def _project(name: str) -> tuple[Path, dict[str, Any]]:
    for manifest in ROOT.glob("*/*/project.json"):
        if "node_modules" in manifest.parts:
            continue
        document = json.loads(manifest.read_text(encoding="utf-8"))
        if document.get("name") == name:
            return manifest.parent, document
    msg = f"no project.json names a project `{name}`"
    raise LookupError(msg)


def _command_of(task: str) -> tuple[str, Path]:
    project, target = task.split(":", 1)
    _, document = _project(project)
    declared = document.get("targets", {}).get(target)
    if declared is None or "command" not in declared:
        msg = f"`{project}` declares no `{target}` command"
        raise LookupError(msg)
    cwd = declared.get("options", {}).get("cwd")
    return declared["command"], ROOT / cwd if cwd else ROOT


def _with_spike_plugin(command: str) -> str:
    """Load the timing plugin into every pytest the command starts, and ask for all durations."""
    loaded = "-p spike_pytest --durations=0 "
    command = re.sub(r"(-m pytest )", r"\1" + loaded, command)
    return re.sub(r"(uv run -q pytest )", r"\1" + loaded, command)


def _timed_linker(out: Path, env: dict[str, str]) -> dict[str, Any]:
    """Put a timing wrapper in front of this target's linker, when a smoke link proves it works."""
    wrapper_dir = out / "timed-linker"
    wrapper_dir.mkdir(parents=True, exist_ok=True)
    binary = wrapper_dir / ("timed-linker.exe" if WINDOWS else "timed-linker")
    built = run(
        ["rustc", "--edition", "2021", "-O", str(HERE / "timed_linker.rs"), "-o", str(binary)]
    )
    if built.returncode != 0:
        return {"used": False, "why": "the wrapper did not build: " + built.stderr[-2000:]}
    real = env.get("SPIKE_REAL_LINKER") or ("link.exe" if WINDOWS else "cc")
    variable = _linker_variable()
    trial = {
        **os.environ,
        **env,
        variable: str(binary),
        "SPIKE_REAL_LINKER": real,
        "SPIKE_LINK_TIMINGS": str(out / "smoke-links.jsonl"),
    }
    # Outside the repository: a crate made inside it would join its workspace.
    smoke = Path(tempfile.mkdtemp(prefix="spike-smoke-")) / "smoke"
    created = run(["cargo", "new", "--quiet", "--vcs", "none", str(smoke)])
    linked = run(
        ["cargo", "build", "--quiet", "--manifest-path", str(smoke / "Cargo.toml")],
        env={**trial, "CARGO_TARGET_DIR": str(smoke / "target")},
    )
    if created.returncode != 0 or linked.returncode != 0:
        return {
            "used": False,
            "why": "the smoke link failed: " + (created.stderr + linked.stderr)[-2000:],
        }
    env.update(
        {
            variable: str(binary),
            "SPIKE_REAL_LINKER": real,
            "SPIKE_LINK_TIMINGS": str(out / "links.jsonl"),
        }
    )
    return {"used": True, "variable": variable, "real": real}


def _jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    lines = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            lines.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return lines


def _by_kind(
    lines: Sequence[dict[str, Any]], key: str, seconds: str, scale: float = 1.0
) -> dict[str, Any]:
    grouped: dict[str, dict[str, float]] = {}
    for line in lines:
        bucket = grouped.setdefault(str(line.get(key)), {"count": 0, "s": 0.0})
        bucket["count"] += 1
        bucket["s"] += float(line.get(seconds, 0.0)) * scale
    return {
        name: {"count": int(v["count"]), "s": round(v["s"], 3)}
        for name, v in sorted(grouped.items())
    }


def _rust_figures(log: str) -> dict[str, Any] | None:
    if "nextest" not in log and "Summary [" not in log:
        return None
    compiled = [int(m.group(1) or 0) * 60 + float(m.group(2)) for m in CARGO_FINISHED.finditer(log)]
    starting = NEXTEST_STARTING.search(log)
    summary = NEXTEST_SUMMARY.findall(log)
    tests = []
    for line in log.splitlines():
        match = NEXTEST_RESULT.match(line)
        if match:
            status, seconds, binary, name = match.groups()
            tests.append({"binary": binary, "test": name, "s": float(seconds), "status": status})
    trivial = sorted(test["s"] for test in tests if test["status"] == "PASS")
    return {
        "compile_s": round(sum(compiled), 3),
        "run_s": float(summary[-1]) if summary else None,
        "tests": int(starting.group(1)) if starting else len(tests),
        "binaries": int(starting.group(2)) if starting else None,
        "fastest_test_s": trivial[0] if trivial else None,
        "median_test_s": trivial[len(trivial) // 2] if trivial else None,
        "per_test": tests,
    }


def _pytest_figures(directory: Path) -> dict[str, Any] | None:
    lines = [line for path in sorted(directory.glob("pytest-*.jsonl")) for line in _jsonl(path)]
    if not lines:
        return None
    tests = [line for line in lines if "nodeid" in line]
    sessions = [line for line in lines if line.get("session")]
    costs: dict[str, dict[str, float]] = {}
    for test in tests:
        for name, cost in test.get("costs", {}).items():
            bucket = costs.setdefault(name, {"count": 0, "s": 0.0})
            bucket["count"] += cost["count"]
            bucket["s"] += cost["s"]
    total = sum(test["s"] for test in tests)
    return {
        "tests": len(tests),
        "test_s": round(total, 3),
        "outcomes": {
            o: sum(1 for t in tests if t["outcome"] == o)
            for o in sorted({t["outcome"] for t in tests})
        },
        "failed": [t["nodeid"] for t in tests if t["outcome"] == "failed"],
        "by_cost": {
            n: {"count": int(c["count"]), "s": round(c["s"], 3)} for n, c in sorted(costs.items())
        },
        "sessions": sessions,
        "per_test": tests,
    }


def run_task(task: str, levers: Sequence[Lever], env: dict[str, str], out: Path) -> dict[str, Any]:
    """Run one task under the levers, and answer what it cost."""
    slug = re.sub(r"[^A-Za-z0-9_.-]", "_", task)
    task_dir = out / slug
    task_dir.mkdir(parents=True, exist_ok=True)
    if task == "bench:octoprint-lookup":
        started = time.perf_counter()
        bench = run([sys.executable, str(HERE / "lookup_bench.py"), str(task_dir / "lookup.json")])
        (out / f"{slug}.log").write_text(bench.stdout + bench.stderr, encoding="utf-8")
        result = (
            json.loads((task_dir / "lookup.json").read_text())
            if (task_dir / "lookup.json").exists()
            else None
        )
        return {
            "task": task,
            "s": round(time.perf_counter() - started, 3),
            "exit": bench.returncode,
            "lookup": result,
        }
    if task == "host:defender":
        return {"task": task, "s": 0.0, "exit": 0, "defender": defender_state()}
    command, cwd = _command_of(task)
    for lever in levers:
        if lever.rewrite is not None:
            command = lever.rewrite(command, task)
    command = _with_spike_plugin(command)
    timings = task_dir / "journey-timings.jsonl"
    task_env = {
        **os.environ,
        **env,
        "SPIKE_PYTEST_OUT": str(task_dir),
        "PRINTOBSERVER_SPIKE_TIMINGS": str(timings),
        "PYTHONPATH": os.pathsep.join(filter(None, [str(HERE), os.environ.get("PYTHONPATH", "")])),
    }
    if "llvm-cov" in command:
        # The clean `just test` makes before a covered run.
        run(["cargo", "llvm-cov", "clean", "--workspace"], cwd=ROOT, capture=False)
    log = out / f"{slug}.log"
    log.write_text(f"$ {command}\n", encoding="utf-8")
    # Shown live in the job's log as well as kept: `tee` under `pipefail`, so the
    # exit status is the command's own.
    task_env["SPIKE_COMMAND"] = command
    task_env["SPIKE_LOG"] = log.as_posix()
    started = time.perf_counter()
    ran = run(
        ["bash", "-o", "pipefail", "-c", 'eval "$SPIKE_COMMAND" 2>&1 | tee -a "$SPIKE_LOG"'],
        cwd=cwd,
        env=task_env,
        capture=False,
    )
    seconds = time.perf_counter() - started
    text = log.read_text(encoding="utf-8", errors="replace")
    journey = _jsonl(timings)
    result: dict[str, Any] = {
        "task": task,
        "command": command,
        "cwd": str(cwd),
        "s": round(seconds, 3),
    }
    result["exit"] = ran.returncode
    rust = _rust_figures(text)
    if rust is not None:
        result["rust"] = rust
    pytest = _pytest_figures(task_dir)
    if pytest is not None:
        result["pytest"] = pytest
    if journey:
        result["journey"] = {
            "by_kind": _by_kind(journey, "kind", "ms", 0.001),
            "operations": len(journey),
        }
    return result


def main(argv: Sequence[str] | None = None) -> int:
    """Run one experiment over one target set and write `<out>/measure.json`."""
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--experiment", default="baseline", help="levers joined by `+`")
    parser.add_argument(
        "--targets", default="", help="`project:target` or pseudo-targets, comma-separated"
    )
    parser.add_argument("--out", type=Path, default=Path("spike-out"))
    parser.add_argument(
        "--list", action="store_true", help="name every lever and pseudo-target, and exit"
    )
    arguments = parser.parse_args(argv)
    if arguments.list:
        for name, lever in LEVERS.items():
            print(f"lever  {name:20} {lever.description}")
        for name, description in PSEUDO_TARGETS.items():
            print(f"target {name:30} {description}")
        return 0
    names = [name for name in arguments.experiment.split("+") if name]
    unknown = [name for name in names if name not in LEVERS]
    targets = [target.strip() for target in arguments.targets.split(",") if target.strip()]
    if unknown or not targets:
        print(
            f"measure: unknown levers {unknown} or no targets; `--list` names them", file=sys.stderr
        )
        return 2
    try:
        for target in targets:
            if target not in PSEUDO_TARGETS:
                _command_of(target)
    except LookupError as error:
        print(f"measure: {error}; `--list` names the pseudo-targets", file=sys.stderr)
        return 2
    out = arguments.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    levers = [LEVERS[name] for name in names]
    env: dict[str, str] = {}
    prepared: dict[str, Any] = {"defender_before": defender_state()}
    for name, lever in zip(names, levers, strict=True):
        env.update(lever.env)
        if lever.prepare is not None:
            prepared[name] = lever.prepare(out)
    if "rust-lld" in names:
        env["SPIKE_REAL_LINKER"] = "rust-lld"
    prepared["timed_linker"] = _timed_linker(out, env)
    if "rust-lld" in names and not prepared["timed_linker"]["used"]:
        env[_linker_variable()] = "rust-lld"
    document: dict[str, Any] = {
        "experiment": names,
        "levers": {name: LEVERS[name].description for name in names},
        "env": env,
        "host": {
            "platform": os.environ.get("PRINTOBSERVER_PLATFORM", sys.platform),
            "machine": platform.machine(),
            "processors": os.cpu_count(),
            "runner": os.environ.get("RUNNER_NAME"),
        },
        "prepared": prepared,
        "tasks": [],
    }
    started = time.perf_counter()
    for target in targets:
        print(f"measure: {target} …", flush=True)
        result = run_task(target, levers, env, out)
        document["tasks"].append(result)
        print(f"measure: {target} took {result['s']:.1f}s, exit {result['exit']}", flush=True)
    links = _jsonl(out / "links.jsonl")
    if links:
        document["links"] = {
            "count": len(links),
            "s": round(sum(line["ms"] for line in links) / 1000, 3),
            "per_output": links,
        }
    document["wall_s"] = round(time.perf_counter() - started, 3)
    (out / "measure.json").write_text(json.dumps(document, indent=1), encoding="utf-8")
    print(f"measure: wrote {out / 'measure.json'}")
    return 0 if all(task["exit"] == 0 for task in document["tasks"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
