"""Prepare and audit one real run of superseded Cargo test output for the budget."""

from __future__ import annotations

import json
import os
import stat
import sys
import time
from pathlib import Path
from typing import NamedTuple

from repo_checks.shell import run

ROOT = Path(__file__).resolve().parents[1]
CONTAINERS = frozenset({"deps", ".fingerprint", "build", "incremental"})


class Entry(NamedTuple):
    """Filesystem identity relevant to detecting one build's output changes."""

    mtime_ns: int
    size: int
    mode: int


type Snapshot = dict[str, Entry]


def snapshot(target: Path) -> Snapshot:
    """Record prunable entries without descending through symlinks."""
    result: Snapshot = {}
    for directory, dirs, files in target.walk(follow_symlinks=False):
        dirs[:] = [name for name in dirs if not (directory / name).is_symlink()]
        relative = directory.relative_to(target)
        # Container roots are shared across configurations; only their children
        # belong to builds. Include symlink nodes, but never follow or age them.
        if not CONTAINERS.intersection(relative.parts):
            continue
        for name in [*dirs, *files]:
            path = directory / name
            metadata = path.lstat()
            result[path.relative_to(target).as_posix()] = Entry(
                metadata.st_mtime_ns,
                metadata.st_size,
                metadata.st_mode,
            )
    return result


def age_created(target: Path, before: Snapshot, after: Snapshot) -> set[str]:
    """Backdate only entries the alternative build created or modified."""
    created = {name for name, value in after.items() if before.get(name) != value}
    # Older than both the retention window and every pre-existing build, so
    # an already-cached ordinary build remains newer than the alternative.
    oldest = min([time.time_ns(), *(value.mtime_ns for value in before.values())])
    aged = oldest - 3 * 24 * 60 * 60 * 1_000_000_000
    for name in sorted(created, reverse=True):
        if not stat.S_ISLNK(after[name].mode):
            os.utime(target / name, ns=(aged, aged), follow_symlinks=False)
    return created


def audit(before: Snapshot, after: Snapshot, alternative: set[str], ordinary: set[str]) -> set[str]:
    """Refuse a meaningless pass or any removal outside the alternative build.

    Raises:
        ValueError: If the workload removed nothing or lost unrelated output.
    """
    removed = before.keys() - after.keys()
    if not removed:
        msg = "timed pass removed no alternative build entries"
        raise ValueError(msg)
    if unexpected := removed - alternative:
        msg = f"timed pass removed entries outside the alternative build: {sorted(unexpected)}"
        raise ValueError(msg)
    if missing := ordinary - after.keys():
        msg = f"timed pass removed ordinary build entries: {sorted(missing)}"
        raise ValueError(msg)
    return removed


def command(argv: list[str], environment: dict[str, str], evidence: Path, name: str) -> None:
    """Run a real command, retaining its full output and exposing failures.

    Raises:
        RuntimeError: If the command fails.
    """
    print(f"pool-maintenance: {name}", file=sys.stderr, flush=True)
    answer = run(argv, cwd=ROOT, env=environment)
    (evidence / f"{name}.log").write_text(answer.stdout + answer.stderr)
    if answer.returncode:
        msg = f"{name}: exit {answer.returncode}\n{answer.stdout}{answer.stderr}"
        raise RuntimeError(msg)


def main() -> None:
    """Clear backlog, build and age an alternative, build normally, then measure."""
    evidence = ROOT / "dist" / "pool-maintenance"
    evidence.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ)
    result = Path(environment.pop("ONEBUDGETSPEC_RESULT", str(evidence / "timed.json")))
    # A target-dir override would measure a different workload than the recipe.
    if any(key in environment for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR")):
        sys.exit("pool-maintenance: unset Cargo target-dir overrides before measuring")
    backlog = evidence / "backlog.json"
    command(
        ["just", "pool-maintenance"],
        {**environment, "ONEBUDGETSPEC_RESULT": str(backlog)},
        evidence,
        "backlog",
    )
    # The recipe owns target selection; Cargo metadata independently verifies it.
    metadata = run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, check=True
    )
    target = Path(json.loads(metadata.stdout)["target_directory"])
    if target != ROOT / "target":
        sys.exit(f"pool-maintenance: Cargo target {target} differs from the declared recipe")
    before = snapshot(target)
    # Same instrumented nextest test-target compilation as the gate, without
    # executing tests. Disabling debug assertions changes metadata in this same
    # profile, allowing the ordinary configuration to supersede these units.
    build = [
        "cargo",
        "llvm-cov",
        "nextest",
        "--workspace",
        "--locked",
        "--no-report",
        "--no-tests=pass",
        "-E",
        "none()",
    ]
    command(
        build,
        {**environment, "CARGO_PROFILE_DEV_DEBUG_ASSERTIONS": "false"},
        evidence,
        "alternative",
    )
    alternative = age_created(target, before, snapshot(target))
    aged = snapshot(target)
    command(build, environment, evidence, "ordinary")
    ready = snapshot(target)
    ordinary = {
        name for name, value in ready.items() if name not in alternative or aged.get(name) != value
    }
    # These manifests list artifact paths including their Cargo metadata hashes.
    for name, entries in (("alternative", alternative), ("ordinary-before", ordinary)):
        (evidence / f"{name}.json").write_text(json.dumps(sorted(entries), indent=2) + "\n")
    command(
        ["just", "pool-maintenance"],
        {**environment, "ONEBUDGETSPEC_RESULT": str(result)},
        evidence,
        "timed",
    )
    after = snapshot(target)
    removed = audit(ready, after, alternative, ordinary)
    (evidence / "removed.json").write_text(json.dumps(sorted(removed), indent=2) + "\n")
    (evidence / "ordinary-after.json").write_text(
        json.dumps(sorted(ordinary.intersection(after)), indent=2) + "\n"
    )
    report = json.loads(result.read_text())
    backlog_report = json.loads(backlog.read_text())
    report["detail"] += (
        f"; untimed backlog seconds (excluded): {backlog_report['value']}"
        f"; audited alternative entries removed: {len(removed)}"
        f"; ordinary entries retained: {len(ordinary)}"
    )
    result.write_text(json.dumps(report) + "\n")
    (evidence / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
