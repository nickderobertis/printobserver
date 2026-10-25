"""`python -m repo_checks probe-registries`, run as `just probe-registries` runs it.

Its journey against doubled registries is the end-to-end tier's, in
`tests/repo-e2e/tests/test_release_probe_journey.py`. What is here is what that
journey cannot reach: a declaration naming no probe, a probe answering
something that is not a version, and the Windows cells of the scheduled proof,
which this suite runs on and that journey does not.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest
from repo_checks.expect import contains, equal
from repo_checks.release_declaration import joined
from repo_checks.shell import run

#: This repository's root, which every case reads its committed files from.
ROOT = Path(__file__).resolve().parents[3]

#: The line naming the probe in the committed declaration.
PROBE_LINE = 'probe = "tools/repo-checks/src/repo_checks/release-probe.sh"\n'


def _tool_packages() -> str:
    """The justfile's `TOOL_PACKAGES`, as the search path `just` runs this command under."""
    for line in (ROOT / "justfile").read_text(encoding="utf-8").splitlines():
        if line.startswith("TOOL_PACKAGES :="):
            listed = line.partition(":=")[2].strip().strip('"').split(":")
            return os.pathsep.join(str(ROOT / entry) for entry in listed)
    message = "the justfile lists no TOOL_PACKAGES, and this command's packages live on it"
    raise AssertionError(message)


def _reconcile(root: Path) -> tuple[int, str, str]:
    """Run the command over the tree at `root`: its exit status, stdout and stderr."""
    result = run(
        [sys.executable, "-m", "repo_checks", "probe-registries", "--root", str(root)],
        cwd=root,
        env={**os.environ, "PYTHONPATH": _tool_packages()},
        timeout=120,
    )
    return result.returncode, result.stdout or "", result.stderr or ""


def _tree(tmp_path: Path, declaration: str) -> Path:
    """A tree carrying `declaration` and the committed `repo-policy.toml`."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "release-targets.toml").write_text(declaration, encoding="utf-8")
    policy = (ROOT / "repo-policy.toml").read_text(encoding="utf-8")
    (root / "repo-policy.toml").write_text(policy, encoding="utf-8")
    return root


def test_a_declaration_naming_no_probe_is_refused(tmp_path: Path) -> None:
    """With no probe there is nothing to reconcile, which is a failure rather than a pass."""
    committed = (ROOT / "release-targets.toml").read_text(encoding="utf-8")
    contains(committed, PROBE_LINE, describing="the committed declaration")
    root = _tree(tmp_path, committed.replace(PROBE_LINE, ""))

    code, out, err = _reconcile(root)

    equal((code, out), (1, ""), describing=f"the reconciliation's answer ({err!r})")
    contains(err, "names no `probe`", describing="what it said")


@pytest.mark.skipif(sys.platform == "win32", reason="the stand-in probe is a bash script")
@pytest.mark.parametrize(
    "printed", ["not-a-version", "0.4.3\\n0.4.4", "0.4.3 trailing"], ids=["text", "two", "extra"]
)
def test_a_probe_answering_something_other_than_one_version_fails(
    tmp_path: Path, printed: str
) -> None:
    """Exit 0 with output is read as released only when the output is one version."""
    committed = (ROOT / "release-targets.toml").read_text(encoding="utf-8")
    root = _tree(tmp_path, committed.replace(PROBE_LINE, 'probe = "probe.sh"\n'))
    stand_in = root / "probe.sh"
    stand_in.write_text(f"#!/usr/bin/env bash\nprintf '{printed}\\n'\n", encoding="utf-8")
    stand_in.chmod(0o755)

    code, out, err = _reconcile(root)

    equal((code, out), (1, ""), describing=f"the reconciliation's answer ({err!r})")
    for target in joined(root):
        contains(err, f"{target.id}: the probe answered", describing="what it said")
    contains(err, "not one line holding one semantic version", describing="what it said")


@pytest.mark.skipif(sys.platform != "win32", reason="the omission is Windows' alone")
def test_a_windows_cell_says_it_asks_nothing_and_passes() -> None:
    """The scheduled proof's Windows cells cannot spawn the probe, and say so."""
    code, out, err = _reconcile(ROOT)

    equal((code, out), (0, ""), describing=f"the reconciliation's answer ({err!r})")
    contains(err, "not run on Windows", describing="what it said")
