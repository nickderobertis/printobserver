"""Audit a real declared maintenance pass and refuse an empty workload."""

import json
import os
import shutil
import sys
import time
from pathlib import Path

import pytest
from measure import ROOT, age_created, audit, snapshot
from repo_checks import expect
from repo_checks.shell import run

# A workspace whose package `x` enables only one feature of `shared`, so a
# per-package build (the gate's) and a workspace build (the measurement's)
# compile `shared` under two metadata hashes, as tokio or serde are in the real
# tree.
FIXTURE = {
    "Cargo.toml": '[workspace]\nmembers = ["shared", "x", "y"]\nresolver = "2"\n',
    "shared/Cargo.toml": (
        '[package]\nname = "shared"\nversion = "0.1.0"\nedition = "2021"\n\n'
        "[features]\na = []\nb = []\n"
    ),
    "shared/src/lib.rs": (
        '#[cfg(feature = "a")]\npub fn a() -> u8 {\n    1\n}\n\n'
        '#[cfg(feature = "b")]\npub fn b() -> u8 {\n    2\n}\n'
    ),
    "x/Cargo.toml": (
        '[package]\nname = "x"\nversion = "0.1.0"\nedition = "2021"\n\n'
        '[dependencies]\nshared = { path = "../shared", features = ["a"] }\n'
    ),
    "x/src/lib.rs": (
        "pub fn x() -> u8 {\n    shared::a()\n}\n\n"
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn x() {\n"
        "        assert_eq!(super::x(), 1);\n    }\n}\n"
    ),
    "y/Cargo.toml": (
        '[package]\nname = "y"\nversion = "0.1.0"\nedition = "2021"\n\n'
        '[dependencies]\nshared = { path = "../shared", features = ["b"] }\n'
    ),
    "y/src/lib.rs": "pub fn y() -> u8 {\n    shared::b()\n}\n",
}
# Variables that would point the fixture's builds or profiles somewhere else.
FOREIGN = ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR", "LLVM_PROFILE_FILE", "ONEBUDGETSPEC_")


def build(target: Path, metadata_hash: str) -> Path:
    """Write Cargo-shaped output with an explicit metadata hash."""
    fingerprint = target / "debug" / ".fingerprint" / f"example-{metadata_hash}"
    fingerprint.mkdir(parents=True)
    (fingerprint / "lib-example").write_text(metadata_hash)
    deps = target / "debug" / "deps"
    deps.mkdir(exist_ok=True)
    (deps / f"libexample-{metadata_hash}.rlib").write_text(metadata_hash)
    return fingerprint


def maintain(worktree: Path) -> None:
    """Exercise the real recipe with the installed released pruner."""
    expect.passing(
        run(
            [
                "just",
                "--justfile",
                str(ROOT / "justfile"),
                "--working-directory",
                str(worktree),
                "pool-maintenance",
            ],
            cwd=worktree,
        )
    )


def test_empty_workload_refused_then_alternative_removed(tmp_path: Path) -> None:
    """A no-op is no measurement; a superseded build gives an audited pass."""
    target = tmp_path / "target"
    baseline = snapshot(target)
    build(target, "1111111111111111")
    alternative = age_created(target, baseline, snapshot(target))
    unchanged = snapshot(target)
    cutoff = time.time_ns() - 2 * 24 * 60 * 60 * 1_000_000_000
    expect.equal(all(entry.mtime_ns < cutoff for entry in unchanged.values()), True)
    maintain(tmp_path)
    with pytest.raises(ValueError, match="removed no alternative"):
        audit(unchanged, snapshot(target), alternative, set())

    build(target, "2222222222222222")
    before = snapshot(target)
    ordinary = before.keys() - alternative
    maintain(tmp_path)
    after = snapshot(target)
    removed = audit(before, after, alternative, ordinary)
    expect.equal(removed, alternative)
    expect.equal(ordinary.intersection(after), ordinary)


def test_audit_refuses_unrelated_removal(tmp_path: Path) -> None:
    """Fail closed if a pass loses ordinary output, even alongside valid removals."""
    target = tmp_path / "target"
    fingerprint = build(target, "1111111111111111")
    before = snapshot(target)
    (fingerprint / "lib-example").unlink()
    with pytest.raises(ValueError, match="outside the alternative"):
        audit(before, snapshot(target), set(), set(before))


def test_aging_does_not_follow_symlinks(tmp_path: Path) -> None:
    """Alternative-build symlinks never age their unrelated destinations."""
    target = tmp_path / "target"
    fingerprint = build(target, "1111111111111111")
    outside = tmp_path / "outside"
    outside.write_text("unrelated")
    try:
        (fingerprint / "linked").symlink_to(outside)
    except OSError:
        pytest.skip("this host does not permit creating symlinks")
    before = outside.stat().st_mtime_ns
    age_created(target, {}, snapshot(target))
    expect.equal(outside.stat().st_mtime_ns, before)


def fixture(root: Path, keep_within: str) -> dict[str, str]:
    """Copy the real measurement and recipe beside a workspace the gate has built.

    Returns:
        The environment to run the measurement under.
    """
    for name, text in FIXTURE.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text)
    (root / "pool-maintenance").mkdir()
    for name in ("measure.sh", "measure.py"):
        shutil.copy2(ROOT / "pool-maintenance" / name, root / "pool-maintenance" / name)
    recipe = run(["just", "--show", "pool-maintenance"], cwd=ROOT, check=True).stdout
    declared = "--keep-within 2d"
    expect.contains(recipe, declared, describing="the declared pool-maintenance recipe")
    tools = (ROOT / "tools" / "repo-checks" / "src").as_posix()
    (root / "justfile").write_text(
        f"export PYTHONPATH := '{tools}'\n\n" + recipe.replace(declared, keep_within)
    )
    environment = {
        name: value for name, value in os.environ.items() if not name.startswith(FOREIGN)
    }
    expect.passing(run(["cargo", "generate-lockfile", "--offline"], cwd=root, env=environment))
    # The gate's coverage output of one package, restored by the cache with
    # mtimes older than the retention window, as on the release check.
    expect.passing(
        run(
            ["cargo", "llvm-cov", "nextest", "-p", "x", "--locked", "--no-report"],
            cwd=root,
            env=environment,
        ),
        describing="the gate's per-package coverage build",
    )
    target = root / "target"
    age_created(target, {}, snapshot(target))
    return {**environment, "ONEBUDGETSPEC_RESULT": str(root / "result.json")}


skip_windows = pytest.mark.skipif(
    sys.platform == "win32",
    reason="the measurement is a bash script `just budgets-release-pr` runs on Linux alone",
)


@skip_windows
def test_measures_beside_the_gates_kept_coverage_output(tmp_path: Path) -> None:
    """Coverage output the gate keeps, superseded by the measurement's build, is backlog."""
    root = tmp_path.resolve()
    environment = fixture(root, "--keep-within 2d")
    measured = run(["bash", "pool-maintenance/measure.sh"], cwd=root, env=environment)
    expect.passing(measured, describing="the measurement")
    report = json.loads((root / "result.json").read_text())
    expect.truth(
        isinstance(report["value"], float) and report["value"] >= 0,
        describing=f"a figure in seconds, not {report['value']!r}",
    )
    expect.contains(report["detail"], "audited alternative entries removed")


@skip_windows
def test_maintenance_leaving_the_superseded_build_is_refused(tmp_path: Path) -> None:
    """A pass that leaves the superseded alternative build behind produces no figure."""
    root = tmp_path.resolve()
    environment = fixture(root, "--keep-within 30d")
    expect.failing(
        run(["bash", "pool-maintenance/measure.sh"], cwd=root, env=environment),
        naming="timed pass removed no alternative build entries",
    )
