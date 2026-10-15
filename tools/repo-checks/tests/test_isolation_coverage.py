"""A suite run under coverage and xdist measures what its workers ran.

The suite's own process runs no test under xdist — its workers do — so what
`coverage run -m pytest -n auto` would measure is collection alone, unless each
worker measures itself. These run a real suite of two tests under coverage and
two workers, as an Nx `test` target runs one, with the hooks
`repo_checks.workers` gives every suite, and read back what was measured.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

from repo_checks.expect import equal, truth
from repo_checks.shell import run
from treecopy import REPO_ROOT

#: Code only a test reaches, so only a worker can have measured it.
MEASURED = """
def reached():
    return "reached"


def never():
    return "never"
"""

#: The hooks every xdist suite of this repository takes.
CONFTEST = """
from repo_checks import workers

pytest_configure_node = workers.pytest_configure_node
pytest_configure = workers.pytest_configure
pytest_unconfigure = workers.pytest_unconfigure
"""

TESTS = """
import measured


def test_one():
    if measured.reached() != "reached":
        raise AssertionError("reached")


def test_two():
    if measured.reached() != "reached":
        raise AssertionError("reached")
"""

CONFIGURATION = """
[tool.coverage.run]
branch = true
parallel = true
relative_files = true
source = ["."]
omit = ["tests/*"]
"""


def _suite(root: Path) -> Path:
    """A suite of two tests over one module, configured as this repository's are."""
    (root / "tests").mkdir(parents=True)
    (root / "pyproject.toml").write_text(CONFIGURATION, encoding="utf-8")
    (root / "measured.py").write_text(MEASURED, encoding="utf-8")
    (root / "tests" / "conftest.py").write_text(CONFTEST, encoding="utf-8")
    (root / "tests" / "test_measured.py").write_text(TESTS, encoding="utf-8")
    return root


def _python(root: Path, *arguments: str) -> None:
    """Run this interpreter in the suite's directory, able to import the suite and the hooks."""
    imports = os.pathsep.join([str(root), str(REPO_ROOT / "tools" / "repo-checks" / "src")])
    ran = run(
        [sys.executable, *arguments],
        cwd=root,
        env={**os.environ, "PYTHONPATH": imports},
        timeout=300,
    )
    equal(ran.returncode, 0, describing=f"{' '.join(arguments)}:\n{ran.stdout}{ran.stderr}")


def _data(root: Path) -> list[Path]:
    """Every coverage data file the run left."""
    return sorted(root.glob(".coverage*"))


def test_each_worker_measures_the_tests_it_ran(tmp_path: Path) -> None:
    """Combined, the run's data has the line only a test reaches, from a file of each worker's."""
    root = _suite(tmp_path / "suite")

    _python(
        root, "-m", "coverage", "run", "-m", "pytest", "tests", "-n", "2", "-p", "no:cacheprovider"
    )

    truth(len(_data(root)) == 3, describing=f"the suite's and two workers' data: {_data(root)}")
    _python(root, "-m", "coverage", "combine")
    _python(root, "-m", "coverage", "json", "-o", "measured.json")
    report = json.loads((root / "measured.json").read_text(encoding="utf-8"))
    lines = report["files"]["measured.py"]
    reached = MEASURED.splitlines().index('    return "reached"') + 1
    never = MEASURED.splitlines().index('    return "never"') + 1
    truth(reached in lines["executed_lines"], describing=f"line {reached} measured: {lines}")
    truth(never in lines["missing_lines"], describing=f"line {never} unmeasured: {lines}")


def test_a_suite_not_run_under_coverage_measures_nothing_in_its_workers(tmp_path: Path) -> None:
    """The workers measure themselves only when the suite's own process is measured."""
    root = _suite(tmp_path / "suite")

    _python(root, "-m", "pytest", "tests", "-n", "2", "-p", "no:cacheprovider")

    equal(_data(root), [], describing="the coverage data an unmeasured run left")
