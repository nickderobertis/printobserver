"""The `ci-check-time` and `ci-job-count` budgets, measured from recorded runs over real HTTP.

Each test serves runs recorded from the forge — the jobs-API document of a real
pull-request run under `recorded/`, or one derived from it — from a stand-in
Actions API on loopback, and drives the real `repo_checks` budget commands
against it: in this process for each measurement, and through the real
`onebudgetspec check` of the committed `budgets.yaml` for the verdict, which runs
those commands the way the `budgets` job's step does. A refusal is driven by
removing or corrupting a field of a recorded document. What is asserted is the
reported result, never a timing of this suite's own.
"""

from __future__ import annotations

import copy
import json
import os
import secrets
import tomllib
from collections.abc import Callable, Iterator
from contextlib import ExitStack
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pytest
from onebudgetspec_sdk import list_budgets
from repo_checks import ci_budgets
from repo_checks.ci_budgets import (
    CHECK_TIME,
    JOB_COUNT,
    OVER,
    REFUSED,
    WITHIN,
    Attempt,
    CommitSha,
    RunId,
    check_budgets,
    measure_check_time,
    measure_job_count,
)
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo, toolchain_tools
from standin_jobs_api import Forge, Run, serving, slowed
from treecopy import REPO_ROOT

RECORDED = Path(__file__).parent / "recorded"
REPOSITORY = "nickderobertis/printobserver"
#: A finished whole-graph pull-request run at bb2fc63, 24 jobs, every one of
#: which took a runner: its longest, the combined macOS job, ran 2,573 s.
RECORDED_RUN = 40235536077
HEAD_SHA = "bb2fc636106ef454d12b9a0901d271015e9d7dc9"
LONGEST = "macos-aarch64 (gate, integration)"
LONGEST_SECONDS = 2573.0
#: A second run the same push started, in another workflow.
SIBLING_RUN = 40235536099
#: What `just` exports to every recipe, so a budget's command finds its package.
PACKAGES = os.pathsep.join(
    str(REPO_ROOT / package)
    for package in (
        "tools/repo-checks/src",
        "tools/contract-codegen/src",
        "tools/release-artifacts/src",
    )
)


@dataclass(frozen=True)
class Registered:
    """What the committed `budgets.yaml` registers for one budget, as onebudgetspec lists it."""

    command: list[str]
    unit: str
    direction: str
    threshold: float


def registered(budget: str) -> Registered:
    """One budget as the committed `budgets.yaml` registers it."""
    (found,) = (
        one for one in list_budgets(paths=[REPO_ROOT / "budgets.yaml"]).budgets if one.id == budget
    )
    return Registered(list(found.command), str(found.unit), str(found.direction), found.threshold)


#: The lines the committed `budgets.yaml` holds, read from it alone.
CHECK_TIME_LINE = int(registered(CHECK_TIME).threshold)
JOB_COUNT_LINE = int(registered(JOB_COUNT).threshold)


def document(run: int) -> dict[str, Any]:
    """One recorded jobs-API document, as the forge answered it.

    `Any`-valued because it is the forge's JSON as `json.loads` hands it back.
    """
    return json.loads((RECORDED / f"jobs-{run}-attempt-1.json").read_text(encoding="utf-8"))


def job(recorded: dict[str, Any], name: str) -> dict[str, Any]:
    """The job of `recorded` called `name`, to derive a document from."""
    return next(entry for entry in recorded["jobs"] if entry["name"] == name)


def skipped(entry: dict[str, Any]) -> None:
    """`entry` as the forge records a job its condition skipped: completed with no runner."""
    entry.update(status="completed", conclusion="skipped", runner_name=None, runner_id=None)
    entry["started_at"] = entry["completed_at"] = entry["created_at"]
    entry["steps"] = []


def measuring(recorded: dict[str, Any]) -> dict[str, Any]:
    """`recorded` with the `budgets` job running, started at the last job's end, as it measures."""
    derived = copy.deepcopy(recorded)
    last = max(entry["completed_at"] for entry in derived["jobs"])
    started = datetime.strptime(last, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=UTC)
    stamp = started.strftime("%Y-%m-%dT%H:%M:%SZ")
    derived["jobs"].append(
        {
            "name": "budgets",
            "status": "in_progress",
            "conclusion": None,
            "created_at": stamp,
            "started_at": stamp,
            "completed_at": None,
            "labels": ["ubuntu-24.04"],
            "runner_name": "GitHub Actions 1",
            "steps": [{"name": "Set up job", "status": "completed", "started_at": stamp}],
        }
    )
    derived["total_count"] = len(derived["jobs"])
    return derived


def forge_of(*documents: tuple[int, str, dict[str, Any]]) -> Forge:
    """A stand-in serving each `(run, workflow, document)` as one pull-request run of `HEAD_SHA`."""
    return Forge(
        REPOSITORY,
        [
            Run(RunId(run), Attempt(1), CommitSha(HEAD_SHA), "pull_request", workflow, doc)
            for run, workflow, doc in documents
        ],
    )


def environment(address: str, **extra: str) -> dict[str, str]:
    """What the `budgets` job's step hands a budget's command: the run, and the API."""
    return {
        "GITHUB_API_URL": address,
        "GITHUB_REPOSITORY": REPOSITORY,
        "GITHUB_RUN_ID": str(RECORDED_RUN),
        "GITHUB_RUN_ATTEMPT": "1",
        "GITHUB_EVENT_NAME": "pull_request",
        **extra,
    }


@pytest.fixture
def result(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """The file a `reported` budget's command writes its result to, under a check."""
    written = tmp_path / "result.json"
    monkeypatch.setenv("ONEBUDGETSPEC_RESULT", str(written))
    return written


def reported(written: Path) -> tuple[float, dict[str, Any]]:
    """The value and the decoded detail a measurement reported."""
    answer = json.loads(written.read_text(encoding="utf-8"))
    return answer["value"], json.loads(answer["detail"])


@pytest.fixture
def served() -> Iterator[Callable[[Forge], str]]:
    """Serve stand-in forges for the test, each until it ends, yielding a way to start one."""
    with ExitStack() as stack:
        yield lambda forge: stack.enter_context(serving(forge))


def test_the_longest_job_is_the_figure_and_every_job_is_in_the_breakdown(
    served: Callable[[Forge], str], result: Path
) -> None:
    """The recorded run's longest job, 2,573 s, with each job's start, end, queue and steps."""
    forge = forge_of((RECORDED_RUN, "ci", measuring(document(RECORDED_RUN))))
    address = served(forge)

    equal(measure_check_time(environment(address)), 0)

    value, detail = reported(result)
    equal(value, LONGEST_SECONDS)
    equal(detail["longest"], LONGEST)
    equal(
        (detail["run_id"], detail["run_attempt"], detail["event"]),
        (RECORDED_RUN, 1, "pull_request"),
    )
    names = [entry["name"] for entry in detail["jobs"]]
    equal(len(names), 24, describing="every job of the run but the one measuring it")
    truth("budgets" not in names, describing="the measuring job left out of the breakdown")
    longest = next(entry for entry in detail["jobs"] if entry["name"] == LONGEST)
    equal(longest["seconds"], LONGEST_SECONDS)
    truth(longest["queue_seconds"] is not None, describing="its queue reported")
    truth(longest["steps"], describing="its steps reported")
    truth(detail["wall_seconds"] >= LONGEST_SECONDS, describing="the clock span beside the figure")
    equal(forge.authorized, [None, None], describing="no token sent to a stand-in")


def test_a_job_its_condition_skipped_ran_for_no_time(
    served: Callable[[Forge], str], result: Path
) -> None:
    """With the longest job skipped, the next longest, 1,559 s, is the figure."""
    recorded = measuring(document(RECORDED_RUN))
    skipped(job(recorded, LONGEST))
    address = served(forge_of((RECORDED_RUN, "ci", recorded)))

    equal(measure_check_time(environment(address)), 0)

    value, detail = reported(result)
    equal((value, detail["longest"]), (1559.0, "gate (linux-x86_64)"))
    gone = next(entry for entry in detail["jobs"] if entry["name"] == LONGEST)
    equal((gone["seconds"], gone["queue_seconds"]), (0.0, None))


def test_the_measuring_job_is_never_the_figure(
    served: Callable[[Forge], str], result: Path
) -> None:
    """A `budgets` job recorded as having run longer than every other job is still left out."""
    recorded = measuring(document(RECORDED_RUN))
    mine = job(recorded, "budgets")
    mine.update(status="completed", conclusion="success", completed_at="2099-01-01T00:00:00Z")
    address = served(forge_of((RECORDED_RUN, "ci", recorded)))

    equal(measure_check_time(environment(address)), 0)

    equal(reported(result)[0], LONGEST_SECONDS)


def test_every_job_that_took_a_runner_across_the_pushs_runs_is_counted(
    served: Callable[[Forge], str], result: Path
) -> None:
    """24 jobs in one run, the measuring job, and one of a second run; a skipped job is not one."""
    recorded = measuring(document(RECORDED_RUN))
    skipped(job(recorded, "classify"))
    sibling = {"total_count": 1, "jobs": [copy.deepcopy(job(document(RECORDED_RUN), "pr-title"))]}
    forge = forge_of((RECORDED_RUN, "ci", recorded), (SIBLING_RUN, "notignored", sibling))
    address = served(forge)

    equal(measure_job_count(environment(address)), 0)

    value, detail = reported(result)
    equal(value, 25.0, describing="24 - 1 skipped + budgets + 1 of the sibling run")
    equal(detail["runs"], [RECORDED_RUN, SIBLING_RUN])
    equal((detail["head_sha"], detail["event"]), (HEAD_SHA, "pull_request"))
    equal(detail["per_family"], {"Linux": 14, "Windows": 10, "macOS": 1})
    counted = {(entry["workflow"], entry["name"]) for entry in detail["jobs"]}
    truth(("ci", "budgets") in counted, describing="the measuring job counted")
    truth(("notignored", "pr-title") in counted, describing="the sibling run's job counted")
    truth(("ci", "classify") not in counted, describing="a skipped job not counted")
    runner = next(entry for entry in detail["jobs"] if entry["name"] == LONGEST)["runner"]
    equal(runner, "macos-15")


def test_a_run_of_another_commit_or_event_is_not_this_pushs(
    served: Callable[[Forge], str], result: Path
) -> None:
    """Only the runs of the current run's own commit and event are read."""
    other = Run(
        RunId(SIBLING_RUN),
        Attempt(1),
        CommitSha("0" * 40),
        "pull_request",
        "ci",
        document(RECORDED_RUN),
    )
    pushed = Run(
        RunId(SIBLING_RUN + 1),
        Attempt(1),
        CommitSha(HEAD_SHA),
        "push",
        "warm",
        document(RECORDED_RUN),
    )
    forge = forge_of((RECORDED_RUN, "ci", measuring(document(RECORDED_RUN))))
    forge.runs = [*forge.runs, other, pushed]
    address = served(forge)

    equal(measure_job_count(environment(address)), 0)

    equal(reported(result)[1]["runs"], [RECORDED_RUN])


def test_jobs_are_read_page_by_page(
    served: Callable[[Forge], str], result: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """With ten jobs a page, the 25 recorded are read over three pages and all counted."""
    monkeypatch.setattr(ci_budgets, "PAGE", 10)
    forge = forge_of((RECORDED_RUN, "ci", measuring(document(RECORDED_RUN))))
    address = served(forge)

    equal(measure_job_count(environment(address)), 0)

    equal(reported(result)[0], 25.0)
    pages = [path for path in forge.asked if "/jobs?" in path]
    equal(len(pages), 3, describing="the pages of jobs asked")


@pytest.mark.parametrize(
    ("unset", "naming"),
    [
        ("GITHUB_REPOSITORY", "GITHUB_REPOSITORY is not set"),
        ("GITHUB_RUN_ID", "GITHUB_RUN_ID is not set"),
    ],
)
def test_a_measurement_with_no_run_named_refuses(
    served: Callable[[Forge], str],
    capsys: pytest.CaptureFixture[str],
    unset: str,
    naming: str,
) -> None:
    """Nothing named to read is a refusal naming what is missing, never a figure of zero."""
    address = served(forge_of((RECORDED_RUN, "ci", document(RECORDED_RUN))))
    named = environment(address)
    del named[unset]

    equal(measure_check_time(named), REFUSED)
    equal(measure_job_count(named), REFUSED)

    said = capsys.readouterr().err
    contains(said, f"{CHECK_TIME}: refused: {naming}")
    contains(said, f"{JOB_COUNT}: refused: {naming}")


@pytest.mark.parametrize(
    ("given", "naming"),
    [
        ({"GITHUB_RUN_ID": "latest"}, "GITHUB_RUN_ID='latest' is not a number"),
        ({"GITHUB_RUN_ATTEMPT": "first"}, "GITHUB_RUN_ATTEMPT='first' is not a number"),
        ({"GITHUB_REPOSITORY": "printobserver"}, "is not `owner/name`"),
        ({"GITHUB_API_URL": "http://example.com"}, "neither https://api.github.com nor"),
        ({"GITHUB_RUN_ID": "1"}, "answered 404"),
    ],
)
def test_a_run_named_wrongly_refuses(
    served: Callable[[Forge], str],
    capsys: pytest.CaptureFixture[str],
    given: dict[str, str],
    naming: str,
) -> None:
    """A run, attempt, repository or address that names nothing readable is refused."""
    address = served(forge_of((RECORDED_RUN, "ci", document(RECORDED_RUN))))

    equal(measure_check_time({**environment(address), **given}), REFUSED)

    contains(capsys.readouterr().err, naming)


def test_a_forge_refusing_the_read_refuses(
    served: Callable[[Forge], str], capsys: pytest.CaptureFixture[str]
) -> None:
    """An error answer is a refusal naming it."""
    forge = forge_of((RECORDED_RUN, "ci", document(RECORDED_RUN)))
    forge.status = 502
    address = served(forge)

    equal(measure_job_count(environment(address)), REFUSED)

    contains(capsys.readouterr().err, "answered 502")


@pytest.mark.parametrize(
    ("corrupt", "naming"),
    [
        (lambda doc: doc.update(total_count=99), "answered 24 of the 99 it counts"),
        (lambda doc: doc.update(total_count="24"), "not a count"),
        (lambda doc: doc["jobs"][0].update(started_at="soon"), "started_at='soon', not an instant"),
        (lambda doc: doc["jobs"][0].update(status="paused"), "say nothing of whether it ran"),
        (lambda doc: doc["jobs"][0].pop("created_at"), "no created_at"),
        (lambda doc: doc["jobs"][0].update(steps=[{"status": "x"}]), "a step with no name"),
        (
            lambda doc: doc["jobs"][0].update(status="in_progress", started_at=None),
            "is in_progress with started_at=None",
        ),
        (
            lambda doc: doc["jobs"][0].update(completed_at=None),
            "is completed with started_at=",
        ),
        (
            lambda doc: doc["jobs"][0].update(runner_name=None, conclusion="success"),
            "completed 'success' with no runner",
        ),
    ],
)
def test_jobs_this_cannot_read_refuse(
    served: Callable[[Forge], str],
    capsys: pytest.CaptureFixture[str],
    corrupt: Callable[[dict[str, Any]], object],
    naming: str,
) -> None:
    """A jobs document of another shape is refused naming the field, never read as zero."""
    recorded = document(RECORDED_RUN)
    corrupt(recorded)
    address = served(forge_of((RECORDED_RUN, "ci", recorded)))

    equal(measure_check_time(environment(address)), REFUSED)

    contains(capsys.readouterr().err, naming)


def test_a_body_that_is_not_json_refuses(
    served: Callable[[Forge], str], capsys: pytest.CaptureFixture[str]
) -> None:
    """Garbage in place of the jobs is refused."""
    forge = forge_of((RECORDED_RUN, "ci", document(RECORDED_RUN)))
    forge.raw = b"<html>rate limited</html>"
    address = served(forge)

    equal(measure_check_time(environment(address)), REFUSED)

    contains(capsys.readouterr().err, "a body that is not JSON")


def test_the_token_goes_to_the_forge_alone(monkeypatch: pytest.MonkeyPatch) -> None:
    """`GITHUB_TOKEN` is sent to the forge's own API and to no stand-in."""
    secret = secrets.token_hex(8)
    found = ci_budgets.forge({"GITHUB_REPOSITORY": REPOSITORY, "GITHUB_TOKEN": secret})
    equal((found.address, found.token), ("https://api.github.com", secret))
    local = ci_budgets.forge(
        {
            "GITHUB_REPOSITORY": REPOSITORY,
            "GITHUB_TOKEN": secret,
            "GITHUB_API_URL": "http://127.0.0.1:9",
        }
    )
    equal(local.token, None)
    monkeypatch.delenv("GITHUB_API_URL", raising=False)
    equal(
        ci_budgets.api_address({"GITHUB_API_URL": "https://api.github.com/"}), ci_budgets.FORGE_API
    )


@pytest.fixture
def check_run(
    served: Callable[[Forge], str], monkeypatch: pytest.MonkeyPatch
) -> Callable[[dict[str, Any], str], int]:
    """Run the `budgets` job's step over one served run, under one event, as the job runs it.

    The real `onebudgetspec check` of the committed `budgets.yaml` runs each
    budget's registered command in a subprocess, which reads the stand-in.
    """

    def run(recorded: dict[str, Any], event: str) -> int:
        address = served(forge_of((RECORDED_RUN, "ci", recorded)))
        for name, value in environment(address, GITHUB_EVENT_NAME=event).items():
            monkeypatch.setenv(name, value)
        monkeypatch.setenv("PYTHONPATH", PACKAGES)
        monkeypatch.delenv("ONEBUDGETSPEC_RESULT", raising=False)
        return check_budgets(Repo(REPO_ROOT))

    return run


def test_the_step_passes_a_run_within_both_lines(
    check_run: Callable[[dict[str, Any], str], int], capsys: pytest.CaptureFixture[str]
) -> None:
    """With every job inside the line and the jobs under the count, both are within."""
    recorded = measuring(document(RECORDED_RUN))
    recorded["jobs"] = [
        entry for entry in recorded["jobs"] if entry["name"] in {"llmlint", "pr-title", "budgets"}
    ]
    recorded["total_count"] = len(recorded["jobs"])

    equal(check_run(recorded, "pull_request"), WITHIN)

    said = capsys.readouterr().out
    contains(said, f"{CHECK_TIME}: within: 261 seconds against {CHECK_TIME_LINE}")
    contains(said, f"{JOB_COUNT}: within: 3 jobs against {JOB_COUNT_LINE}")


def test_the_step_fails_a_pull_request_over_either_line(
    check_run: Callable[[dict[str, Any], str], int], capsys: pytest.CaptureFixture[str]
) -> None:
    """The recorded 25-job run, its longest job stretched past the line, is over both."""
    recorded = slowed(measuring(document(RECORDED_RUN)), LONGEST, CHECK_TIME_LINE)

    equal(check_run(recorded, "pull_request"), OVER)

    said = capsys.readouterr()
    over = int(LONGEST_SECONDS) + CHECK_TIME_LINE
    contains(said.err, f"{CHECK_TIME}: over: {over} seconds against {CHECK_TIME_LINE}")
    contains(said.err, f"{JOB_COUNT}: over: 25 jobs against {JOB_COUNT_LINE}")
    contains(said.err, "no job is sharded and the line is not loosened")
    contains(said.out, f'"longest": "{LONGEST}"')


def test_the_step_reports_and_passes_off_a_pull_request(
    check_run: Callable[[dict[str, Any], str], int], capsys: pytest.CaptureFixture[str]
) -> None:
    """On a push, over either line is reported and not enforced: each line holds a pull request."""
    recorded = slowed(measuring(document(RECORDED_RUN)), LONGEST, CHECK_TIME_LINE)

    equal(check_run(recorded, "push"), WITHIN)

    said = capsys.readouterr().out
    contains(said, f"{CHECK_TIME}: over, reported and not enforced off a pull request")
    contains(said, f"{JOB_COUNT}: over, reported and not enforced off a pull request")


def test_the_step_errs_where_a_figure_cannot_be_read(
    check_run: Callable[[dict[str, Any], str], int], capsys: pytest.CaptureFixture[str]
) -> None:
    """A run the commands refuse to read is an error of the step, not a pass."""
    recorded = document(RECORDED_RUN)
    recorded["total_count"] = 99

    equal(check_run(recorded, "pull_request"), REFUSED)

    contains(capsys.readouterr().err, f"{CHECK_TIME}: error:")


def test_both_lines_are_registered_with_their_commands() -> None:
    """The committed `budgets.yaml` registers both, with the commands `repo_checks` answers."""
    check_time, job_count = registered(CHECK_TIME), registered(JOB_COUNT)
    equal(
        check_time.command,
        ["uv", "run", "python", "-m", "repo_checks", "ci_check_time_budget"],
    )
    equal(job_count.command, ["uv", "run", "python", "-m", "repo_checks", "ci_job_count_budget"])
    equal((check_time.unit, check_time.direction), ("seconds", "max"))
    equal((job_count.unit, job_count.direction, JOB_COUNT_LINE), ("jobs", "max", 8))
    truth(0 < CHECK_TIME_LINE <= 3600, describing="the check-time line within the one-hour cap")
    truth(CHECK_TIME_LINE % 60 == 0, describing="the check-time line in whole minutes")
    ids = [one.id for one in list_budgets(paths=[REPO_ROOT / "budgets.yaml"]).budgets]
    truth("gate-time" not in ids, describing="the per-context line it replaces gone")


def test_the_held_cli_is_the_one_the_sdk_brings() -> None:
    """`repo-policy.toml`'s onebudgetspec, the dev group's SDK pin and the lock name one release."""
    held = next(t for t in toolchain_tools(Repo(REPO_ROOT)) if t.command == "onebudgetspec")
    lock = tomllib.loads((REPO_ROOT / "uv.lock").read_text(encoding="utf-8"))
    # A virtual workspace member, the calibration tool, is locked with no version.
    locked = {p["name"]: p["version"] for p in lock["package"] if "version" in p}
    project = tomllib.loads((REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8"))

    equal(locked["onebudgetspec-cli"], held.version)
    equal(locked["onebudgetspec-sdk"], held.version)
    contains("\n".join(project["dependency-groups"]["dev"]), f"onebudgetspec-sdk=={held.version}")
