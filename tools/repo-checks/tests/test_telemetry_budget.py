"""`python -m repo_checks telemetry_budget <file> <figure>`: a recorded figure, reported.

Driven the way `onebudgetspec check` drives a budget's command: as a
subprocess, with `ONEBUDGETSPEC_RESULT` naming the file the result is written
to, and through a real `onebudgetspec check` of a budgets file registering it.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal
from repo_checks.shell import run

RECORDED = {
    "figures": {"served_bytes": 0, "fixture_bytes": 6144},
    "detail": {"objects": 3, "paths": ["a.jpg"]},
}


def _recorded(tmp_path: Path, document: object) -> Path:
    path = tmp_path / "telemetry.json"
    path.write_text(json.dumps(document), encoding="utf-8")
    return path


def _command(path: Path, figure: str, result: Path) -> tuple[int, str, str]:
    """Run the verb as a budget's command is run, answering its status, output and result."""
    environment = {**os.environ, "ONEBUDGETSPEC_RESULT": str(result)}
    environment["PYTHONPATH"] = os.pathsep.join(sys.path)
    answer = run(
        [sys.executable, "-m", "repo_checks", "telemetry_budget", str(path), figure],
        env=environment,
    )
    written = result.read_text(encoding="utf-8") if result.exists() else ""
    return answer.returncode, answer.stderr, written


def test_a_recorded_figure_is_reported_with_its_detail(tmp_path: Path) -> None:
    """The figure is the budget's value, and the detail rides beside it."""
    path = _recorded(tmp_path, RECORDED)
    code, _, written = _command(path, "fixture_bytes", tmp_path / "result.json")
    equal(code, 0)
    result = json.loads(written)
    equal(result["value"], 6144.0)
    equal(json.loads(result["detail"]), RECORDED["detail"])


def test_a_zero_is_reported_as_a_zero(tmp_path: Path) -> None:
    """A figure of nothing served is a measured zero rather than a missing one."""
    path = _recorded(tmp_path, RECORDED)
    code, _, written = _command(path, "served_bytes", tmp_path / "result.json")
    equal(code, 0)
    equal(json.loads(written)["value"], 0.0)


@pytest.mark.parametrize(
    ("document", "figure", "said"),
    [
        (None, "served_bytes", "is not there: run the test that records it first"),
        ("not json {", "served_bytes", "could not be read as JSON"),
        ({"detail": {}}, "served_bytes", "records no `figures` object"),
        (RECORDED, "missing", "records no finite number under `missing`"),
        ({"figures": {"x": "7"}}, "x", "records no finite number under `x`"),
        ({"figures": {"x": True}}, "x", "records no finite number under `x`"),
        ('{"figures": {"x": NaN}}', "x", "records no finite number under `x`"),
        ('{"figures": {"x": Infinity}}', "x", "records no finite number under `x`"),
        ('{"figures": {"x": -Infinity}}', "x", "records no finite number under `x`"),
        ('{"figures": {"x": 1' + "0" * 400 + "}}", "x", "records no finite number under `x`"),
    ],
)
def test_an_unreadable_figure_is_an_error_and_never_a_zero(
    tmp_path: Path, document: object, figure: str, said: str
) -> None:
    """Missing or malformed telemetry fails the measurement, naming what is missing."""
    path = tmp_path / "telemetry.json"
    match document:
        case None:
            pass
        case str(text):
            path.write_text(text, encoding="utf-8")
        case recorded:
            path.write_text(json.dumps(recorded), encoding="utf-8")
    code, err, written = _command(path, figure, tmp_path / "result.json")
    equal(code, 1)
    contains(err, said)
    equal(written, "", describing="no result written for a figure that could not be read")


def test_a_check_of_a_registered_budget_reads_its_verdict_off_the_figure(tmp_path: Path) -> None:
    """`onebudgetspec check` runs the command, and rules on the figure it reported."""
    path = _recorded(tmp_path, RECORDED)
    budgets = tmp_path / "budgets.yaml"
    command = [sys.executable, "-m", "repo_checks", "telemetry_budget", str(path)]
    budgets.write_text(
        "schema_version: 1\nbudgets:\n"
        + "".join(
            f"  - id: {name}\n    description: d\n    measure: reported\n"
            f"    command: {json.dumps([*command, figure])}\n"
            f"    unit: bytes\n    direction: max\n    threshold: {threshold}\n"
            for name, figure, threshold in (
                ("zero", "served_bytes", 0),
                ("over", "fixture_bytes", 100),
            )
        ),
        encoding="utf-8",
    )
    environment = {**os.environ, "PYTHONPATH": os.pathsep.join(sys.path)}
    answer = run(["onebudgetspec", "check", "--json", str(budgets)], env=environment)
    verdicts = {result["id"]: result["verdict"] for result in json.loads(answer.stdout)["results"]}
    equal(verdicts, {"zero": "within", "over": "over"})


def test_the_verb_needs_both_its_arguments(capsys: pytest.CaptureFixture[str]) -> None:
    """Without the figure there is nothing to report, and the verb says what it needs."""
    with pytest.raises(SystemExit):
        main(["telemetry_budget", "telemetry.json"])
    contains(capsys.readouterr().err, "needs the telemetry file and the figure")
    with pytest.raises(SystemExit):
        main(["coverage", "a", "b"])
    contains(capsys.readouterr().err, "coverage takes no second argument")
