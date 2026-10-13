"""Drive the hosted-runner temp recipe and read what real Python children use."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import pytest
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.shell import run

ROOT = Path(__file__).resolve().parents[3]
PROBE = """
import json, os, tempfile
with tempfile.NamedTemporaryFile() as opened:
    print(json.dumps({"temp": tempfile.gettempdir(), "file": opened.name,
                      "TEMP": os.environ["TEMP"], "TMP": os.environ["TMP"]}))
"""


def _setup(tmp_path: Path, *, drive_present: bool) -> dict[str, str]:
    """Run the recipe before importing tempfile in a fresh child."""
    drive = tmp_path / "scratch drive"
    fallback = tmp_path / "existing temp"
    fallback.mkdir()
    (fallback / "unrelated").write_text("keep", encoding="utf-8")
    if drive_present:
        drive.mkdir()
        (drive / "unrelated").write_text("keep", encoding="utf-8")
    environment_file = tmp_path / "job-env"
    environment = dict(os.environ, PRINTOBSERVER_PLATFORM="windows-x86_64")
    environment["GITHUB_ENV"] = str(environment_file)
    result = run(
        ["just", "runner-temp", "setup", str(drive), str(fallback)],
        cwd=ROOT,
        env=environment,
        timeout=30,
    )
    passing(result)
    for line in environment_file.read_text(encoding="utf-8-sig").splitlines():
        name, value = line.split("=", 1)
        environment[name] = value
    return environment


@pytest.mark.parametrize("drive_present", [True, False])
def test_children_use_scratch_or_usable_fallback_and_cleanup_keeps_unrelated_files(
    tmp_path: Path, drive_present: bool
) -> None:
    """The real recipe exports the same temp to children and Python's file creation."""
    environment = _setup(tmp_path, drive_present=drive_present)
    chosen = Path(environment["TEMP"])
    expected_parent = tmp_path / ("scratch drive" if drive_present else "existing temp")
    equal(chosen.parent if drive_present else chosen, expected_parent)
    result = run(["just", "--command", sys.executable, "-c", PROBE], cwd=ROOT, env=environment)
    passing(result)
    answer = json.loads(result.stdout)
    equal(Path(answer["temp"]), chosen)
    equal(Path(answer["file"]).parent, chosen)
    equal(answer["TEMP"], answer["TMP"])
    passing(run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30))
    truth((expected_parent / "unrelated").is_file(), describing="unrelated files to survive")
    truth(
        (tmp_path / "existing temp" / "unrelated").is_file(),
        describing="files in the existing temp directory to survive",
    )
    if drive_present:
        truth(not chosen.exists(), describing="only this job's scratch to be removed")
    else:
        truth(chosen.exists(), describing="the shared fallback to remain")


@pytest.mark.parametrize("has_another_jobs_marker", [False, True])
def test_cleanup_refuses_a_directory_without_its_ownership_marker(
    tmp_path: Path, has_another_jobs_marker: bool
) -> None:
    """A foreign path in the job environment is refused without removing it."""
    environment = _setup(tmp_path, drive_present=False)
    foreign = tmp_path / ("printobserver-" + "a" * 32)
    foreign.mkdir()
    if has_another_jobs_marker:
        (foreign / ".printobserver-owned").write_text(foreign.name, encoding="utf-8")
    retained = foreign / "unrelated"
    retained.write_text("keep", encoding="utf-8")
    environment["PRINTOBSERVER_JOB_SCRATCH"] = str(foreign)
    result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30)
    failing(result, naming="Refusing cleanup of unowned scratch")
    contains(result.stderr, "Refusing cleanup of unowned scratch")
    truth(retained.is_file(), describing="the foreign directory contents to survive")


def test_windows_arm_keeps_its_existing_temp(tmp_path: Path) -> None:
    """Only x86 opts in, even if another host has a writable scratch drive."""
    environment_file = tmp_path / "job-env"
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM="windows-aarch64",
        GITHUB_ENV=str(environment_file),
    )
    passing(run(["just", "runner-temp", "setup", str(tmp_path)], cwd=ROOT, env=environment))
    truth(not environment_file.exists(), describing="ARM's environment to remain usable unchanged")
    equal(list(tmp_path.iterdir()), [])
