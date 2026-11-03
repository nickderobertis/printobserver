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
from repo_checks.shell import run
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


@pytest.mark.parametrize(
    ("text", "said"),
    [
        (b'{"figures": {"seconds": NaN}}', "which is not a finite number"),
        (b'{"figures": {"seconds": Infinity}}', "which is not a finite number"),
        (b'{"figures": {"seconds": -Infinity}}', "which is not a finite number"),
        (b'{"figures": {"seconds": 1e400}}', "which is not a finite number"),
        (b'{"figures": {"seconds": 1' + b"0" * 400 + b"}}", "which is not a finite number"),
        (b'{"figures": {"seconds": ' + b"9" * 5000 + b"}}", "is not JSON this can read"),
        (b"{ not json", "is not JSON this can read"),
        (b'{"figures": {"seconds": 1.0}, "detail": "\xff"}', "is not UTF-8 text"),
    ],
)
def test_text_that_holds_no_finite_figure_is_an_error_through_the_command(
    project: Path, text: bytes, said: str
) -> None:
    """Text that holds no finite figure is an error of the real command, naming why.

    What JSON as Python reads it can hold beyond a figure — NaN, the infinities, an
    integer past a float's range, an integer too long to read — and text that is not
    JSON or not UTF-8 are each refused, and the command's stderr says which.
    """
    (project / "telemetry" / "figures.json").write_bytes(text)

    result = checked(project)

    equal(result.verdict, "error")
    truth(result.actual is None, describing="no figure for a file holding none")
    contains(result.error or "", said)


def test_a_file_that_cannot_be_read_is_an_error_through_the_command(project: Path) -> None:
    """A path that names something no file can be read from is refused, naming it."""
    (project / "telemetry" / "figures.json").mkdir()

    result = checked(project)

    equal(result.verdict, "error")
    contains(result.error or "", "could not be read")


@pytest.mark.parametrize(
    "given",
    [[], ["telemetry/figures.json"]],
    ids=["no file and no figure", "a file and no figure"],
)
def test_the_command_naming_no_file_or_figure_exits_refused_saying_so(given: list[str]) -> None:
    """Both are the budget's to name: the command refuses, nonzero, naming what is missing."""
    ran = run(
        ["uv", "run", "-q", "python", "-m", "repo_checks", "telemetry_budget", *given],
        cwd=REPO_ROOT,
        env={**os.environ, "PYTHONPATH": PACKAGES},
    )

    equal(ran.returncode, telemetry_budget.REFUSED)
    contains(ran.stderr, "give the telemetry file and the figure")


def test_a_figure_given_to_another_command_is_refused() -> None:
    """Only this command reads a figure, so no other command quietly ignores one."""
    ran = run(
        ["uv", "run", "-q", "python", "-m", "repo_checks", "gate_time_budget", "measure", "x"],
        cwd=REPO_ROOT,
        env={**os.environ, "PYTHONPATH": PACKAGES},
    )

    truth(ran.returncode != 0, describing="a stray figure refused")
    contains(ran.stderr, "a figure can only be given to telemetry_budget")


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
