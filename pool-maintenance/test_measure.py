"""Audit a real declared maintenance pass and refuse an empty workload."""

from pathlib import Path

import pytest
from measure import ROOT, age_created, audit, snapshot
from repo_checks import expect
from repo_checks.shell import run


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
