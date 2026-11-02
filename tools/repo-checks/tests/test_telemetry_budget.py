"""The shared telemetry reader, driven the way a budget's check drives it.

Each case writes the telemetry file a recording test writes, registers a budget
over it in a budgets file of its own, and runs the real `onebudgetspec check`
of that file — which runs `python -m repo_checks telemetry_budget` as the
budget's command, exactly as a project's `budgets` target does. A refusal is
driven by writing the file wrong, or not at all. What is asserted is the
verdict, the figure and the detail the check carries.
"""

from __future__ import annotations

import json
import os
from collections.abc import Iterator
from pathlib import Path

import pytest
from onebudgetspec_sdk import CheckResult, check, list_budgets
from repo_checks import telemetry_budget
from repo_checks.expect import contains, equal, truth
from treecopy import REPO_ROOT

#: What `just` exports to every recipe, so the budget's command finds its package.
PACKAGES = os.pathsep.join(
    str(REPO_ROOT / package)
    for package in (
        "tools/repo-checks/src",
        "tools/contract-codegen/src",
        "tools/release-artifacts/src",
    )
)

#: The budget each case registers.
BUDGET = "a-recorded-figure"


@pytest.fixture
def project(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Iterator[Path]:
    """A project root holding a budgets file over `telemetry/figures.json`, at 23 seconds."""
    monkeypatch.delenv("ONEBUDGETSPEC_RESULT", raising=False)
    monkeypatch.setenv("PYTHONPATH", PACKAGES)
    (tmp_path / "telemetry").mkdir()
    (tmp_path / "budgets.yaml").write_text(
        "schema_version: 1\n"
        "budgets:\n"
        f"  - id: {BUDGET}\n"
        "    description: A figure a test recorded.\n"
        "    measure: reported\n"
        "    command: [uv, run, --project, "
        f"{json.dumps(str(REPO_ROOT))}, python, -m, repo_checks, telemetry_budget, "
        "telemetry/figures.json, seconds]\n"
        "    unit: seconds\n"
        "    direction: max\n"
        "    threshold: 23\n"
        "    timeout_seconds: 120\n",
        encoding="utf-8",
    )
    yield tmp_path


def recorded(project: Path, document: object) -> None:
    """Write the telemetry file as a recording test writes it."""
    (project / "telemetry" / "figures.json").write_text(json.dumps(document), encoding="utf-8")


def checked(project: Path) -> CheckResult:
    """The real check's one result for the budget."""
    (result,) = check(paths=["budgets.yaml"], ids=[BUDGET], cwd=project).results
    return result


def test_a_recorded_figure_within_its_threshold_is_within_with_its_detail(project: Path) -> None:
    """21.57 s against 23 s is within, and the recorded detail is the result's detail."""
    recorded(
        project,
        {"figures": {"seconds": 21.57, "other": 1}, "detail": {"attempts_s": [4.9, 4.9]}},
    )

    result = checked(project)

    equal((result.verdict, result.actual, result.threshold), ("within", 21.57, 23.0))
    equal(json.loads(result.detail or "{}"), {"attempts_s": [4.9, 4.9]})


def test_a_recorded_figure_over_its_threshold_is_over(project: Path) -> None:
    """The reader reports the figure and never judges it: 41.57 s is the check's to call over."""
    recorded(project, {"figures": {"seconds": 41.57}, "detail": {}})

    equal(checked(project).verdict, "over")


@pytest.mark.parametrize(
    ("document", "said"),
    [
        (None, "is absent"),
        ("not an object", "is not a JSON object"),
        ({"detail": {}}, "carries no `figures` object"),
        ({"figures": {"other": 1.0}}, "records no figure `seconds`; it records other"),
        ({"figures": {"seconds": True}}, "which is not a finite number"),
        ({"figures": {"seconds": "21"}}, "which is not a finite number"),
        ({"figures": {"seconds": 1.0}, "detail": [1]}, "a `detail` that is not a JSON object"),
    ],
)
def test_a_file_or_figure_that_cannot_be_read_is_an_error_naming_why(
    project: Path, document: object, said: str, capsys: pytest.CaptureFixture[str]
) -> None:
    """Nothing is reported for it, and the check answers the budget as an error, not a figure."""
    if document is not None:
        recorded(project, document)

    result = checked(project)
    equal(result.verdict, "error")
    truth(result.actual is None, describing="no figure for an unreadable file")

    path = project / "telemetry" / "figures.json"
    equal(telemetry_budget.measure(str(path), "seconds"), telemetry_budget.REFUSED)
    contains(capsys.readouterr().err, said)


@pytest.mark.parametrize("figure", ["NaN", "Infinity", "-Infinity"])
def test_a_figure_that_is_not_finite_is_an_error(
    project: Path, figure: str, capsys: pytest.CaptureFixture[str]
) -> None:
    """JSON as Python writes it can carry NaN and the infinities; none is a figure."""
    path = project / "telemetry" / "figures.json"
    path.write_text(f'{{"figures": {{"seconds": {figure}}}, "detail": {{}}}}', encoding="utf-8")

    result = checked(project)
    equal(result.verdict, "error")
    truth(result.actual is None, describing="no figure for a value that is not finite")

    equal(telemetry_budget.measure(str(path), "seconds"), telemetry_budget.REFUSED)
    contains(capsys.readouterr().err, "which is not a finite number")


def test_a_file_that_is_not_json_is_refused_naming_it(
    project: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Text that is not JSON is refused as such."""
    path = project / "telemetry" / "figures.json"
    path.write_text("{ not json", encoding="utf-8")

    equal(telemetry_budget.measure(str(path), "seconds"), telemetry_budget.REFUSED)
    contains(capsys.readouterr().err, "is not JSON")


def test_a_command_naming_no_file_or_figure_is_refused(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Both are the budget's to name; neither is guessed."""
    equal(telemetry_budget.measure(None, "seconds"), telemetry_budget.REFUSED)
    equal(telemetry_budget.measure("telemetry/figures.json", None), telemetry_budget.REFUSED)
    contains(capsys.readouterr().err, "give the telemetry file and the figure")


def test_the_check_delay_budget_is_registered_as_it_is_stated() -> None:
    """Seconds, at most 23, no label, read by this reader from the journey's telemetry.

    The command changes to the repository root first, because onebudgetspec runs
    a budget's command in its budgets file's directory, and both the telemetry
    path and the recipes' search path for this package are the root's.
    """
    root = REPO_ROOT / "crates/printobserver-server/tests/check_delay_budget"
    (registered,) = list_budgets(paths=[root / "budgets.yaml"]).budgets

    equal(registered.id, "scheduled-check-worst-delay-seconds")
    equal((registered.unit, registered.direction, registered.threshold), ("seconds", "max", 23.0))
    equal(registered.labels, [])
    equal(registered.measure, "reported")
    equal(
        registered.command,
        [
            "uv",
            "run",
            "--directory",
            "../../../..",
            "python",
            "-m",
            "repo_checks",
            "telemetry_budget",
            "crates/printobserver-server/tests/check_delay_budget/telemetry/scheduled-check-delay.json",
            "seconds",
        ],
    )
    equal((root / "../../../..").resolve(), REPO_ROOT.resolve())
