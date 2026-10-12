"""Fixtures that run the spike's harness the way a cell of its workflow does.

Every test drives `measure.py` as a program over real tasks: tasks of a project
this fixture declares for the length of one test, so a task that fails and a
task whose pytest spawns, sleeps and fails are real ones rather than stand-ins.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any, cast

import pytest
from repo_checks.expect import truth
from repo_checks.shell import run

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
PYTHONPATH = os.pathsep.join(
    [str(HERE), str(ROOT / "tools/repo-checks/src"), os.environ.get("PYTHONPATH", "")]
)

#: A suite whose three tests spend their time on the three things the plugin
#: attributes: a child process, a sleep, and a failure.
CASES = """
import subprocess
import time

import pytest


def test_spawns_git():
    subprocess.run(["git", "--version"], check=True, capture_output=True)


def test_sleeps():
    time.sleep(0.05)


@pytest.mark.parametrize("value", [time.time_ns(), time.time_ns() + 1])
def test_a_parameter_rendered_differently_by_each_process(value):
    pass


def test_fails():
    raise AssertionError("this case fails on purpose")
"""


@dataclass(frozen=True)
class Measured:
    """One run of the harness: its exit, what it printed, and the report it wrote."""

    code: int
    out: str
    err: str
    report: dict[str, object] | None


def measure(*arguments: str, out: Path) -> Measured:
    """Run `measure.py` as the `spike-measure` recipe does, into `out`."""
    ran = run(
        [sys.executable, str(HERE / "measure.py"), "--out", str(out), *arguments],
        cwd=ROOT,
        env={**os.environ, "PYTHONPATH": PYTHONPATH},
    )
    written = out / "measure.json"
    report = json.loads(written.read_text(encoding="utf-8")) if written.is_file() else None
    return Measured(ran.returncode, ran.stdout, ran.stderr, report)


@pytest.fixture
def fixture_project() -> Iterator[str]:
    """A project declared in the tree for one test, named by what this returns."""
    name = f"spike-fixture-{os.getpid()}"
    root = ROOT / "tools" / name
    (root / "cases").mkdir(parents=True)
    (root / "cases" / "test_cases.py").write_text(CASES, encoding="utf-8")
    manifest = {
        "name": name,
        "root": f"tools/{name}",
        "targets": {
            "fails": {"command": "exit 3"},
            "suite": {"command": f"uv run -q pytest tools/{name}/cases -p no:cacheprovider"},
        },
    }
    (root / "project.json").write_text(json.dumps(manifest), encoding="utf-8")
    try:
        yield name
    finally:
        shutil.rmtree(root)


@pytest.fixture
def tasks_of() -> Callable[[Measured], list[dict[str, object]]]:
    """The task results a report carries."""

    def read(measured: Measured) -> list[dict[str, object]]:
        report = measured.report or {}
        tasks = report.get("tasks")
        return tasks if isinstance(tasks, list) else []

    return read


def mapping(value: object, *, describing: str) -> dict[str, Any]:
    """`value` as the JSON object a report holds there, refusing anything else."""
    truth(isinstance(value, dict), describing=f"{describing} as an object, not {value!r}")
    return cast("dict[str, Any]", value)
