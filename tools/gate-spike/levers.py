"""The levers one experiment pulls: how a task's command, environment or host changes.

A lever rewrites a task's command, adds to its environment, or prepares the host
before the experiment's tasks run; one with a `restore` (`base-tree`) is put
back afterwards, and the host levers are left in place on the disposable runner
they changed. Each preparation answers a `Preparation` saying whether it
applied and what it did, so a report records which of them took effect.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

from repo_checks.shell import run
from spike_names import REAL_LINKER, STABLE_IDS, WORKER_COVERAGE

if TYPE_CHECKING:
    from collections.abc import Callable
    from subprocess import CompletedProcess

ROOT = Path(__file__).resolve().parents[2]
WINDOWS = sys.platform == "win32"

#: The files this spike's own branch changes that the suites read as the
#: committed tree: its cut-down workflow and its justfile recipes.
SPIKE_CHANGED = (".github/workflows/ci.yml", "justfile")


class HostError(RuntimeError):
    """A host program a lever needs answered something a lever cannot act on."""


@dataclass(frozen=True)
class Preparation:
    """What one lever did to the host before the experiment's tasks ran."""

    lever: str
    applied: bool
    detail: str


@dataclass(frozen=True)
class DefenderState:
    """Defender's real-time scanning on this host, and what it already excludes."""

    applies: bool
    realtime: bool | None = None
    antivirus: bool | None = None
    mode: str | None = None
    excluded: tuple[str, ...] = ()
    unreadable: str | None = None


@dataclass(frozen=True)
class Lever:
    """One change to how a task runs, and nothing else."""

    description: str
    env: dict[str, str] = field(default_factory=dict)
    rewrite: Callable[[str], str] | None = None
    prepare: Callable[[str], Preparation] | None = None
    restore: Callable[[], None] | None = None


def _checked(argv: list[str], *, cwd: Path | None = None) -> CompletedProcess[str]:
    """Run a host program, refusing a non-zero exit with what it said."""
    ran = run(argv, cwd=cwd)
    if ran.returncode != 0:
        msg = f"`{' '.join(argv[:3])}` exited {ran.returncode}: {ran.stderr.strip()}"
        raise HostError(msg)
    return ran


def _powershell(script: str) -> CompletedProcess[str]:
    return _checked(["powershell", "-NoProfile", "-NonInteractive", "-Command", script])


def _flag(document: dict[str, object], key: str) -> bool | None:
    value = document.get(key)
    return value if isinstance(value, bool) else None


#: What `defender_state` asks PowerShell, one JSON object back.
DEFENDER_QUERY = (
    "$s = Get-MpComputerStatus; $p = Get-MpPreference; "
    "@{realtime=$s.RealTimeProtectionEnabled; antivirus=$s.AntivirusEnabled; "
    "mode=$s.AMRunningMode; excluded=@($p.ExclusionPath)} | ConvertTo-Json -Compress"
)


def read_defender_state(answer: str) -> DefenderState:
    """Read what `DEFENDER_QUERY` answered, keeping an answer of any other shape as unreadable."""
    try:
        document = json.loads(answer)
    except json.JSONDecodeError:
        return DefenderState(applies=True, unreadable=answer)
    if not isinstance(document, dict):
        return DefenderState(applies=True, unreadable=answer)
    excluded = document.get("excluded")
    mode = document.get("mode")
    return DefenderState(
        applies=True,
        realtime=_flag(document, "realtime"),
        antivirus=_flag(document, "antivirus"),
        mode=mode if isinstance(mode, str) else None,
        excluded=tuple(path for path in excluded if isinstance(path, str) and path)
        if isinstance(excluded, list)
        else (),
    )


def defender_state() -> DefenderState:
    """What Defender's real-time scanning is doing on this host, and what it excludes."""
    if not WINDOWS:
        return DefenderState(applies=False)
    try:
        return read_defender_state(_powershell(DEFENDER_QUERY).stdout)
    except HostError as error:
        return DefenderState(applies=True, unreadable=str(error))


def _excluded_paths() -> list[str]:
    home = Path.home()
    uv_cache = _checked(["uv", "cache", "dir"]).stdout.strip()
    paths = [str(ROOT), str(home / ".cargo"), str(home / ".rustup"), tempfile.gettempdir()]
    return [path for path in [*paths, uv_cache] if path and Path(path).is_dir()]


def _exclude_from_defender(name: str) -> Preparation:
    if not WINDOWS:
        return Preparation(name, applied=False, detail="no Defender off Windows")
    paths = _excluded_paths()
    quoted = ",".join("'" + path.replace("'", "''") + "'" for path in paths)
    _powershell(f"Add-MpPreference -ExclusionPath {quoted}")
    return Preparation(name, applied=True, detail=f"excluded {paths}; now {defender_state()}")


def _defender_off(name: str) -> Preparation:
    if not WINDOWS:
        return Preparation(name, applied=False, detail="no Defender off Windows")
    _powershell("Set-MpPreference -DisableRealtimeMonitoring $true")
    return Preparation(name, applied=True, detail=f"now {defender_state()}")


#: What the spike's changed files held before `base-tree` replaced them, put
#: back byte for byte afterwards: uncommitted work in them included.
_HELD: dict[str, bytes] = {}


def _base_tree(name: str) -> Preparation:
    """Put the base branch's copies of the spike's changed files in their place."""
    base = _checked(["git", "merge-base", "HEAD", "origin/main"], cwd=ROOT).stdout.strip()
    shown = {
        path: _checked(["git", "show", f"{base}:{path}"], cwd=ROOT).stdout for path in SPIKE_CHANGED
    }
    for path, text in shown.items():
        _HELD[path] = (ROOT / path).read_bytes()
        (ROOT / path).write_text(text, encoding="utf-8", newline="")
    return Preparation(name, applied=True, detail=f"{list(SPIKE_CHANGED)} from {base}")


def _spike_tree() -> None:
    for path, held in _HELD.items():
        (ROOT / path).write_bytes(held)
    _HELD.clear()


def _temp_on_d(name: str) -> Preparation:
    if not WINDOWS or not Path("D:/").is_dir():
        return Preparation(name, applied=False, detail="this host has no D: drive")
    temp = Path("D:/spike-temp")
    temp.mkdir(parents=True, exist_ok=True)
    # Every task's environment is built from this process's, and `tempfile`
    # caches the directory it found first, so both are pointed there.
    os.environ["TEMP"] = os.environ["TMP"] = str(temp)
    tempfile.tempdir = None
    return Preparation(name, applied=True, detail=f"TEMP and TMP are {temp}")


#: The linker the `rust-lld` lever names, which rustc puts on the linker's path.
RUST_LLD = "rust-lld"


def _rust_lld(name: str) -> Preparation:
    """Name rust-lld as the linker on Windows, where its `link` flavor is a drop-in for link.exe."""
    if not WINDOWS:
        return Preparation(
            name, applied=False, detail="rust-lld is this lever's linker on windows-msvc alone"
        )
    os.environ[REAL_LINKER] = RUST_LLD
    return Preparation(name, applied=True, detail=f"{REAL_LINKER}={RUST_LLD}")


def _replace(old: str, new: str) -> Callable[[str], str]:
    return lambda command: command.replace(old, new)


def _no_rust_coverage(command: str) -> str:
    return command.replace("cargo llvm-cov nextest", "cargo nextest run").replace(
        " --no-report", ""
    )


class LeverError(ValueError):
    """A lever that cannot be pulled on the task it was asked of."""


#: Where a task's command starts nextest, which is where `compile-only` filters.
NEXTEST = re.compile(r"(cargo llvm-cov nextest|cargo nextest run)")


def _compile_only(command: str) -> str:
    """Build and list every test binary, and run none: nextest's `none()` filter.

    A command's own `-E` filter is kept inside it; a command with none is given
    one, and told that a run of no tests passes.

    Raises:
        LeverError: If the command starts no nextest run to filter.
    """
    if not NEXTEST.search(command):
        msg = f"compile-only filters a nextest run, and `{command}` starts none"
        raise LeverError(msg)
    if '-E "' in command:
        return re.sub(r'-E "([^"]*)"', r'-E "none() & (\1)"', command, count=1)
    passing = "" if "--no-tests=" in command else " --no-tests=pass"
    return NEXTEST.sub(lambda found: f'{found.group(1)}{passing} -E "none()"', command, count=1)


#: Every way a task's command starts pytest.
PYTEST = re.compile(r"(-m pytest |uv run -q pytest )")


def _xdist(workers: int) -> Callable[[str], str]:
    def rewrite(command: str) -> str:
        # `coverage run` alone would measure only the controller, which runs no
        # test under xdist; with SPIKE_WORKER_COVERAGE set, `spike_pytest.py`
        # measures each worker instead, the way pytest-cov does.
        command = command.replace("uv run -q coverage run -m pytest", "uv run -q python -m pytest")
        return PYTEST.sub(lambda found: f"{found.group(1)}-n {workers} --dist load ", command)

    return rewrite


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
        "Defender excludes the workspace, ~/.cargo, ~/.rustup, the temporary directory and the "
        "uv cache",
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
        "split-debuginfo unpacked (windows-msvc supports packed alone)",
        env={"CARGO_PROFILE_DEV_SPLIT_DEBUGINFO": "unpacked"},
    ),
    "rust-lld": Lever("link with the toolchain's own rust-lld (windows-msvc)", prepare=_rust_lld),
    "base-tree": Lever(
        "the base branch's ci.yml and justfile in place of the spike's, for the run",
        prepare=_base_tree,
        restore=_spike_tree,
    ),
    "temp-on-d": Lever("TEMP and TMP on the D: drive (windows-2025 has one)", prepare=_temp_on_d),
}
for _workers in (2, 3, 4):
    LEVERS[f"xdist-{_workers}"] = Lever(
        f"pytest-xdist at {_workers} workers, each worker under Python coverage",
        env={WORKER_COVERAGE: "1", STABLE_IDS: "1"},
        rewrite=_xdist(_workers),
    )
    LEVERS[f"xdist-{_workers}-nocov"] = Lever(
        f"pytest-xdist at {_workers} workers, no Python coverage",
        env={STABLE_IDS: "1"},
        rewrite=_xdist(_workers),
    )


@dataclass(frozen=True)
class DiskSpace:
    """Free space on the drive holding one directory."""

    path: str
    free_gb: float


def disk() -> tuple[DiskSpace, ...]:
    """Free space on the drives holding the temporary directory and the workspace."""
    return tuple(
        DiskSpace(str(path), round(shutil.disk_usage(path).free / 2**30, 2))
        for path in (Path(tempfile.gettempdir()), ROOT)
    )


def pytest_temporaries() -> set[Path]:
    """Every base temporary pytest has made under the temporary directory."""
    return {
        found
        for found in Path(tempfile.gettempdir()).glob("pytest-of-*/pytest-*")
        if found.is_dir() and not found.is_symlink()
    }


def clear_pytest_temporaries(before: set[Path]) -> float:
    """Remove the base temporaries pytest made since `before`, answering the GB freed.

    Called as one task ends, so what it removes is what appeared while that task
    ran: the task's own, and any an unrelated pytest on this host started in the
    meantime, which a CI cell running one task at a time does not have. One that
    already existed — the directory this run writes its report into among them,
    which exists before any task starts — is left.
    """
    temp = Path(tempfile.gettempdir())
    free = shutil.disk_usage(temp).free
    for made in pytest_temporaries() - before:
        shutil.rmtree(made, ignore_errors=True)
    return round((shutil.disk_usage(temp).free - free) / 2**30, 2)
