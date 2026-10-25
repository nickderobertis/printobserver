"""`python -m repo_checks probe-registries`, run as `just probe-registries` runs it.

Its journey against doubled registries is the end-to-end tier's, in
`tests/repo-e2e/tests/test_release_probe_journey.py`. What is here is what that
journey cannot reach: a declaration naming no probe, a probe answering no
release for every target, a probe answering something that is not a version,
and the Windows cells of the scheduled proof, which this suite runs on and that
journey does not.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path
from typing import NamedTuple

import pytest
from repo_checks.expect import contains, equal
from repo_checks.release_declaration import joined
from repo_checks.release_probes import COMMITTED_PROBE, STANDING_IN, semver
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


class Reconciled(NamedTuple):
    """What one run of the command left: its exit status, stdout and stderr."""

    code: int
    out: str
    err: str


def _reconcile(root: Path, **environment: str) -> Reconciled:
    """Run the command over the tree at `root`, with `environment` added to the caller's."""
    result = run(
        [sys.executable, "-m", "repo_checks", "probe-registries", "--root", str(root)],
        cwd=root,
        env={**os.environ, "PYTHONPATH": _tool_packages(), **environment},
        timeout=120,
    )
    return Reconciled(result.returncode, result.stdout or "", result.stderr or "")


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


@pytest.mark.skipif(sys.platform == "win32", reason="the stand-in probe is a bash script")
def test_a_probe_answering_no_release_for_every_target_passes(tmp_path: Path) -> None:
    """Exit 0 with nothing printed is no release, an answer a consumer acts on, so it passes."""
    committed = (ROOT / "release-targets.toml").read_text(encoding="utf-8")
    root = _tree(tmp_path, committed.replace(PROBE_LINE, 'probe = "probe.sh"\n'))
    stand_in = root / "probe.sh"
    stand_in.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
    stand_in.chmod(0o755)

    code, out, err = _reconcile(root)

    equal((code, err), (0, ""), describing="the reconciliation's exit and findings")
    equal(
        out.splitlines(),
        [f"{target.name} {target.id} no-release" for target in joined(root)],
        describing="what it says each registry serves",
    )


def test_a_declaration_declaring_no_target_fails_rather_than_passing_empty(
    tmp_path: Path,
) -> None:
    """Reconciling nothing would pass vacuously, so a declaration with no target fails."""
    root = _tree(tmp_path, f"schema_version = 3\n{PROBE_LINE}")

    code, out, err = _reconcile(root)

    equal((code, out), (1, ""), describing=f"the reconciliation's answer ({err!r})")
    contains(err, "declares no `[[target]]` table", describing="what it said")


@pytest.mark.skipif(sys.platform == "win32", reason="the stand-in probe is a bash script")
def test_a_probe_failing_without_a_word_is_reported_by_its_exit(tmp_path: Path) -> None:
    """A probe that says nothing on its way out is still named, with the status it left."""
    committed = (ROOT / "release-targets.toml").read_text(encoding="utf-8")
    root = _tree(tmp_path, committed.replace(PROBE_LINE, 'probe = "probe.sh"\n'))
    stand_in = root / "probe.sh"
    stand_in.write_text("#!/usr/bin/env bash\nexit 3\n", encoding="utf-8")
    stand_in.chmod(0o755)

    code, out, err = _reconcile(root)

    equal((code, out), (1, ""), describing=f"the reconciliation's answer ({err!r})")
    contains(
        err,
        "release:printobserver: the probe did not answer: exit 3 and nothing said",
        describing="what it said",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the stand-in probe is a bash script")
def test_a_proof_pointed_at_stand_in_registries_asks_the_real_ones_nothing(
    tmp_path: Path,
) -> None:
    """The gate's proofs over doubled registries never reach the live ones through the probe."""
    committed = (ROOT / "release-targets.toml").read_text(encoding="utf-8")
    root = _tree(tmp_path, committed.replace(PROBE_LINE, 'probe = "probe.sh"\n'))
    asked = root / "asked"
    stand_in = root / "probe.sh"
    stand_in.write_text(f"#!/usr/bin/env bash\ntouch '{asked}'\nexit 3\n", encoding="utf-8")
    stand_in.chmod(0o755)

    pointed = _reconcile(root, **{STANDING_IN: "http://127.0.0.1:9"})
    equal(tuple(pointed), (0, "", ""), describing="the reconciliation beside stand-ins")
    equal(asked.exists(), False, describing="whether the probe was run")

    code, _, err = _reconcile(root, **{STANDING_IN: ""})
    equal(code, 1, describing=f"the same reconciliation with no stand-in named ({err!r})")
    equal(asked.exists(), True, describing="whether the probe was run")


def test_the_stand_in_variable_is_the_one_the_registry_proof_reads() -> None:
    """The variable the reconciliation stands aside for is the proof's own, by its declaration."""
    registries = ROOT / "tools/release-artifacts/src/release_artifacts/registries.py"
    declared = re.findall(
        r'^PRINTOBSERVER_PROOF_REGISTRIES = "([^"]+)"$',
        registries.read_text(encoding="utf-8"),
        re.MULTILINE,
    )

    equal(declared, [STANDING_IN], describing=f"what {registries.name} declares")


@pytest.mark.skipif(sys.platform != "win32", reason="the omission is Windows' alone")
def test_a_windows_cell_says_it_asks_nothing_and_passes() -> None:
    """The scheduled proof's Windows cells cannot spawn the probe, and say so."""
    code, out, err = _reconcile(ROOT)

    equal((code, out), (0, ""), describing=f"the reconciliation's answer ({err!r})")
    contains(err, "not run on Windows", describing="what it said")


#: Answers on both sides of the grammar's edges, each held the same by bash and by Python.
VERSIONS = [
    "0.4.3",
    "10.20.30",
    "1.0.0-rc.1",
    "1.0.0-0.3.7",
    "1.0.0-x-y.7z.92",
    "1.0.0-rc.1+build.5",
    "1.0.0+20260101",
    "01.0.0",
    "1.0",
    "v1.0.0",
    "1.0.0-01",
    "1.0.0-",
    "1.0.0+",
    "1.0.0-rc..1",
    "1.0.0 ",
    "",
]


@pytest.mark.skipif(sys.platform == "win32", reason="the probe's grammar is bash's to evaluate")
@pytest.mark.parametrize("version", VERSIONS)
def test_the_reconciliation_reads_a_version_exactly_as_the_probe_does(version: str) -> None:
    """The grammar is the probe's own, so bash and this reconciliation agree on every answer."""
    assignments = "\n".join(
        line
        for line in COMMITTED_PROBE.read_text(encoding="utf-8").splitlines()
        if line.startswith("readonly SEMVER")
    )
    by_bash = run(
        ["bash", "-c", f'{assignments}\n[[ "$1" =~ $SEMVER ]]', "bash", version], timeout=30
    )

    equal(
        semver(COMMITTED_PROBE.read_text(encoding="utf-8")).fullmatch(version) is not None,
        by_bash.returncode == 0,
        describing=f"whether {version!r} is a version",
    )


def test_a_probe_that_states_no_grammar_is_refused_by_name() -> None:
    """A probe whose `SEMVER` moved leaves nothing to read an answer by, and says so."""
    with pytest.raises(ValueError, match="readonly SEMVER") as refused:
        semver("#!/usr/bin/env bash\necho 0.1.0\n")

    contains(str(refused.value), COMMITTED_PROBE.name, describing="the refusal")
