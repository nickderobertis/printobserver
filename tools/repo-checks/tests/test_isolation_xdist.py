"""Run serially and under xdist, `test_verified_download` collects one set of tests.

Its parameters once carried what each process drew afresh, so two xdist workers
collected two different sets and refused to run the session at all. This runs
it as the suite's Nx target runs it — pytest under xdist, in a process of its
own — and the way a serial run does, and compares the tests each reached.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from repo_checks.expect import equal, truth
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: The module whose parameters were drawn afresh in each process.
MODULE = "tools/repo-checks/tests/test_verified_download.py"

#: A test pytest reports it ran, with the worker that ran it.
RAN = re.compile(r"^\[gw\d+\] \[\s*\d+%\] (?:PASSED|SKIPPED|XFAIL) (.+?)\s*$", re.MULTILINE)


def _pytest(*arguments: str, basetemp: Path) -> str:
    """Run pytest over the module from the repository root, as Nx does, and answer its output."""
    ran = run(
        [
            sys.executable,
            "-m",
            "pytest",
            MODULE,
            "-p",
            "no:cacheprovider",
            "--basetemp",
            str(basetemp),
            *arguments,
        ],
        cwd=REPO_ROOT,
        timeout=600,
    )
    equal(ran.returncode, 0, describing=f"pytest {' '.join(arguments)}:\n{ran.stdout}{ran.stderr}")
    return ran.stdout


def test_xdist_workers_collect_and_run_exactly_what_a_serial_run_collects(tmp_path: Path) -> None:
    """Three workers each collect the serial run's tests, and between them run every one."""
    serial = _pytest("--collect-only", basetemp=tmp_path / "serial")
    collected = sorted(line for line in serial.splitlines() if "::" in line)
    parallel = _pytest("-n", "3", "-vv", basetemp=tmp_path / "parallel")
    ran = sorted(RAN.findall(parallel))

    truth(len(collected) > 1, describing=f"the {len(collected)} tests the serial run collected")
    equal(ran, collected, describing="the tests three xdist workers ran")
