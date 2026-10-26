"""The `gate-time` budget, measured from recorded jobs-API documents over real HTTP.

Each run here serves one document under `recorded/` — the forge's own answer
for a real run — from a stand-in jobs API on loopback, and drives the real
`repo_checks gate_time_budget` against it: in this process for the measurement,
and through the real `onebudgetspec check` of the committed `budgets.yaml` for
the verdict, which runs that command the way the `gate-time` step does. A
refusal is driven by removing or corrupting a field of a recorded document.
What is asserted is the reported result, never a timing of this suite's own.
"""

from __future__ import annotations

import copy
import json
import os
import secrets
import tomllib
from collections.abc import Callable, Iterator
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pytest
from onebudgetspec_sdk import check, list_budgets
from repo_checks import gate_time_budget
from repo_checks.expect import contains, equal, truth
from repo_checks.gate_time_budget import OVER, REFUSED, WITHIN, check_budget, measure
from repo_checks.model import Repo, toolchain_tools
from standin_jobs_api import Jobs, serving
from treecopy import REPO_ROOT

RECORDED = Path(__file__).parent / "recorded"
REPOSITORY = "nickderobertis/printobserver"
#: A finished pull-request run at bb2fc63, every required context completed:
#: `gate (linux-x86_64)` one job of 1,559 s, `gate (windows-aarch64)` four
#: shards and an aggregator, and both macOS contexts carried after the combined
#: `macos-aarch64 (gate, integration)` job's 2,573 s.
COMBINED = 40235536077
#: What `just` exports to every recipe, so the budget's command finds its package.
PACKAGES = os.pathsep.join(
    str(REPO_ROOT / package)
    for package in (
        "tools/repo-checks/src",
        "tools/contract-codegen/src",
        "tools/release-artifacts/src",
    )
)


def document(run: int) -> dict[str, Any]:
    """One recorded jobs-API document, as the forge answered it.

    `Any`-valued because it is the forge's JSON as `json.loads` hands it back.
    """
    return json.loads((RECORDED / f"jobs-{run}-attempt-1.json").read_text(encoding="utf-8"))


def job(recorded: dict[str, Any], name: str) -> dict[str, Any]:
    """The job of `recorded` called `name`, to derive a document from."""
    return next(entry for entry in recorded["jobs"] if entry["name"] == name)


def environment(address: str, kind: str, platform: str, **extra: str) -> dict[str, str]:
    """What a `gate-time` step's command sees: the run, the context, and the API."""
    return {
        "GITHUB_API_URL": address,
        "GITHUB_REPOSITORY": REPOSITORY,
        "GITHUB_RUN_ID": str(COMBINED),
        "GITHUB_RUN_ATTEMPT": "1",
        "GATE_TIME_KIND": kind,
        "PRINTOBSERVER_PLATFORM": platform,
        "GITHUB_EVENT_NAME": "pull_request",
        "GITHUB_HEAD_REF": "nick/255/printobserver-release-declaration",
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
def api() -> Iterator[tuple[Jobs, str]]:
    """The stand-in jobs API serving the recorded run."""
    jobs = Jobs(REPOSITORY, COMBINED, 1, document(COMBINED))
    with serving(jobs) as address:
        yield jobs, address


def test_an_unsharded_gate_is_measured_with_its_breakdown(
    api: tuple[Jobs, str], result: Path
) -> None:
    """`gate (linux-x86_64)` ran 1,559 s as one job, its three seconds of queue reported apart."""
    jobs, address = api

    equal(measure(environment(address, "gate", "linux-x86_64")), 0)

    value, detail = reported(result)
    equal(value, 1559.0)
    equal(detail["context"], "gate (linux-x86_64)")
    equal((detail["kind"], detail["platform"]), ("gate", "linux-x86_64"))
    equal((detail["run_id"], detail["run_attempt"]), (COMBINED, 1))
    equal(detail["initial_queue_seconds"], 3.0)
    equal(detail["measured_to_source"], "completion")
    equal([entry["name"] for entry in detail["jobs"]], ["gate (linux-x86_64)"])
    only = detail["jobs"][0]
    equal(
        (only["started_at"], only["completed_at"], only["queue_seconds"]),
        ("2026-10-25T21:28:10Z", "2026-10-25T21:54:09Z", 3.0),
    )
    steps = {step["name"]: step["seconds"] for step in only["steps"]}
    equal(steps['Run just check "$(just gate-tier)"'], 1494.0)
    equal(len(only["steps"]), 17)
    equal(jobs.authorized, [None], describing="the token is sent to the forge's API alone")


def test_a_sharded_gate_spans_its_shards_to_its_aggregator(
    api: tuple[Jobs, str], result: Path
) -> None:
    """`gate (windows-aarch64)`: its longest shard's 1,155 s, then its aggregator's 9 s."""
    _, address = api

    equal(measure(environment(address, "gate", "windows-aarch64")), 0)

    value, detail = reported(result)
    equal(value, 1164.0)
    equal(
        [entry["name"] for entry in detail["jobs"]],
        ["gate (windows-aarch64)", *(f"gate-shard (windows-aarch64, {k}/4)" for k in range(1, 5))],
    )
    equal([entry["queue_seconds"] for entry in detail["jobs"]], [1.0, 2.0, 1.0, 1.0, 2.0])
    equal(detail["initial_queue_seconds"], 1.0)


#: A pull-request run at 65bf8ff while the account's runners were saturated:
#: `gate (windows-aarch64)`'s five shards started over fourteen minutes, and its
#: aggregator queued 717 s for a runner after the last of them ended.
SATURATED = 40310931115


def test_a_runner_queue_is_reported_and_never_part_of_the_figure(result: Path) -> None:
    """The longest shard's 1,584 s and the aggregator's own 9 s, not the 2,633 s waited."""
    with serving(Jobs(REPOSITORY, SATURATED, 1, document(SATURATED))) as address:
        settings = {
            **environment(address, "gate", "windows-aarch64"),
            "GITHUB_RUN_ID": str(SATURATED),
        }
        equal(measure(settings), 0)

    value, detail = reported(result)
    equal(value, 1593.0)
    equal(detail["wall_seconds"], 2633.0)
    queues = {job["name"]: job["queue_seconds"] for job in detail["jobs"]}
    equal(queues["gate (windows-aarch64)"], 717.0)
    equal(queues["gate-shard (windows-aarch64, 2/5)"], 561.0)


def test_the_combined_macos_job_is_behind_its_integration_context(
    api: tuple[Jobs, str], result: Path
) -> None:
    """`integration (macos-aarch64)`: the combined job's 2,573 s, then its carrier's 7 s."""
    _, address = api

    equal(measure(environment(address, "integration", "macos-aarch64")), 0)

    value, detail = reported(result)
    equal(value, 2580.0)
    equal(
        [entry["name"] for entry in detail["jobs"]],
        ["integration (macos-aarch64)", "macos-aarch64 (gate, integration)"],
    )
    combined = {step["name"]: step["seconds"] for step in detail["jobs"][1]["steps"]}
    equal(combined["Run just test-integration"], 681.0)


def _in_flight(recorded: dict[str, Any], name: str, step_started: str | None) -> dict[str, Any]:
    """`recorded` with job `name` still running, inside a `gate-time` step where one is given."""
    derived = copy.deepcopy(recorded)
    running = job(derived, name)
    running.update(status="in_progress", conclusion=None, completed_at=None)
    if step_started is not None:
        running["steps"].append(
            {
                "name": "gate-time",
                "status": "in_progress",
                "conclusion": None,
                "number": len(running["steps"]) + 1,
                "started_at": step_started,
                "completed_at": None,
            }
        )
    return derived


def test_a_running_aggregator_is_measured_to_its_gate_time_step(result: Path) -> None:
    """Read from inside the step, the span ends where the forge recorded that step's start."""
    derived = _in_flight(document(COMBINED), "gate (linux-x86_64)", "2026-10-25T21:58:10Z")
    with serving(Jobs(REPOSITORY, COMBINED, 1, derived)) as address:
        now = datetime(2026, 10, 25, 23, 0, tzinfo=UTC)
        equal(measure(environment(address, "gate", "linux-x86_64"), now=now), 0)

    value, detail = reported(result)
    equal(value, 1800.0)
    equal((detail["measured_to"], detail["measured_to_source"]), ("2026-10-25T21:58:10Z", "step"))


def test_a_skipped_gate_time_step_is_read_as_never_started(result: Path) -> None:
    """A job that failed before its budget step ran is measured to its completion instead."""
    derived = document(COMBINED)
    job(derived, "gate (linux-x86_64)")["steps"].append(
        {
            "name": "gate-time",
            "status": "completed",
            "conclusion": "skipped",
            "number": 18,
            "started_at": "2026-10-25T21:54:09Z",
            "completed_at": "2026-10-25T21:54:09Z",
        }
    )
    with serving(Jobs(REPOSITORY, COMBINED, 1, derived)) as address:
        equal(measure(environment(address, "gate", "linux-x86_64")), 0)

    value, detail = reported(result)
    equal(value, 1559.0)
    equal(detail["measured_to_source"], "completion")


def test_a_step_the_forge_has_not_recorded_yet_is_measured_to_the_clock(result: Path) -> None:
    """Before the forge records the step's start, the moment of measuring is that start."""
    derived = _in_flight(document(COMBINED), "gate (linux-x86_64)", None)
    with serving(Jobs(REPOSITORY, COMBINED, 1, derived)) as address:
        now = datetime(2026, 10, 25, 21, 58, 11, tzinfo=UTC)
        equal(measure(environment(address, "gate", "linux-x86_64"), now=now), 0)

    value, detail = reported(result)
    equal(value, 1801.0)
    equal(detail["measured_to_source"], "clock")


def test_every_page_of_jobs_is_read(
    api: tuple[Jobs, str], result: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Twenty-four jobs at ten a page are three pages, and a context on the last is found."""
    jobs, address = api
    monkeypatch.setattr(gate_time_budget, "PAGE", 10)

    equal(measure(environment(address, "integration", "macos-aarch64")), 0)

    equal([path.rsplit("page=", 1)[-1] for path in jobs.asked], ["1", "2", "3"])
    truth(result.is_file(), describing="a result read off three pages")


@pytest.mark.parametrize(
    ("unset", "naming"),
    [
        ("GITHUB_RUN_ID", "GITHUB_RUN_ID is not set, so there is no run"),
        ("GITHUB_REPOSITORY", "GITHUB_REPOSITORY is not set"),
        ("GATE_TIME_KIND", "GATE_TIME_KIND is not set"),
        ("PRINTOBSERVER_PLATFORM", "PRINTOBSERVER_PLATFORM is not set"),
    ],
)
def test_a_measurement_with_nothing_to_name_is_refused_by_name(
    api: tuple[Jobs, str],
    result: Path,
    capsys: pytest.CaptureFixture[str],
    unset: str,
    naming: str,
) -> None:
    """No run, repository or context: nothing is read and nothing is reported."""
    jobs, address = api
    environ = {k: v for k, v in environment(address, "gate", "linux-x86_64").items() if k != unset}

    equal(measure(environ), REFUSED)

    contains(capsys.readouterr().err, naming)
    truth(not result.exists(), describing="no figure reported")
    if unset in {"GITHUB_RUN_ID", "GITHUB_REPOSITORY"}:
        equal(jobs.asked, [], describing="what was asked of the API")


@pytest.mark.parametrize(
    ("fields", "naming"),
    [
        ({"GATE_TIME_KIND": "release"}, "GATE_TIME_KIND='release' names no required context"),
        ({"GITHUB_RUN_ID": "latest"}, "GITHUB_RUN_ID='latest' is not a number"),
        ({"GITHUB_RUN_ATTEMPT": "1; x"}, "GITHUB_RUN_ATTEMPT='1; x' is not a number"),
        ({"GITHUB_REPOSITORY": "printobserver"}, "'printobserver' is not `owner/name`"),
        ({"GITHUB_API_URL": "http://example.com"}, "is neither https://api.github.com nor"),
        ({"GITHUB_API_URL": "https://[::1"}, "is neither https://api.github.com nor"),
        ({"GITHUB_RUN_ID": "1"}, "answered 404"),
    ],
)
def test_a_measurement_that_names_no_readable_run_is_refused(
    api: tuple[Jobs, str],
    result: Path,
    capsys: pytest.CaptureFixture[str],
    fields: dict[str, str],
    naming: str,
) -> None:
    """A context, run or address this cannot read from is refused naming it."""
    _, address = api

    equal(measure({**environment(address, "gate", "linux-x86_64"), **fields}), REFUSED)

    contains(capsys.readouterr().err, naming)
    truth(not result.exists(), describing="no figure reported")


def _without_aggregator(recorded: dict[str, Any]) -> None:
    recorded["jobs"] = [e for e in recorded["jobs"] if e["name"] != "gate (windows-aarch64)"]
    recorded["total_count"] = len(recorded["jobs"])


def _corrupt_start(recorded: dict[str, Any]) -> None:
    job(recorded, "gate-shard (windows-aarch64, 2/4)")["started_at"] = "yesterday"


def _undercounted(recorded: dict[str, Any]) -> None:
    recorded["total_count"] = len(recorded["jobs"]) + 5


def _no_count(recorded: dict[str, Any]) -> None:
    del recorded["total_count"]


def _boolean_count(recorded: dict[str, Any]) -> None:
    recorded["total_count"] = True


def _nameless_job(recorded: dict[str, Any]) -> None:
    recorded["jobs"][3]["name"] = ["gate (windows-aarch64)"]


def _nameless_step(recorded: dict[str, Any]) -> None:
    job(recorded, "gate (windows-aarch64)")["steps"][0]["status"] = {"completed": True}


@pytest.mark.parametrize(
    ("derive", "naming"),
    [
        (_without_aggregator, "gate (windows-aarch64): no job of that name"),
        (_corrupt_start, "has started_at='yesterday', not an instant"),
        (_undercounted, "answered 24 of the 29 jobs it counts"),
        (_no_count, "answered `total_count` None, not a count of jobs"),
        (_boolean_count, "answered `total_count` True, not a count of jobs"),
        (_nameless_job, "answered job 3 with no name"),
        (_nameless_step, "answered a step with no name or status"),
    ],
)
def test_a_context_whose_jobs_cannot_be_read_is_refused_rather_than_zero(
    result: Path,
    capsys: pytest.CaptureFixture[str],
    derive: Callable[[dict[str, Any]], None],
    naming: str,
) -> None:
    """A document no span can be read from reports no figure at all."""
    derived = document(COMBINED)
    derive(derived)
    with serving(Jobs(REPOSITORY, COMBINED, 1, derived)) as address:
        equal(measure(environment(address, "gate", "windows-aarch64")), REFUSED)

    contains(capsys.readouterr().err, naming)
    truth(not result.exists(), describing="no figure reported")


@pytest.mark.parametrize(
    ("raw", "status", "naming"),
    [
        (b"<html>busy</html>", 200, "answered a body that is not JSON"),
        (b"[]", 200, "answered list, not a jobs document"),
        (None, 502, "answered 502"),
    ],
)
def test_a_forge_answering_no_jobs_document_is_refused(
    result: Path,
    capsys: pytest.CaptureFixture[str],
    raw: bytes | None,
    status: int,
    naming: str,
) -> None:
    """Garbage or an error from the API is a refusal naming the address, never zero."""
    jobs = Jobs(REPOSITORY, COMBINED, 1, document(COMBINED), raw=raw, status=status)
    with serving(jobs) as address:
        equal(measure(environment(address, "gate", "linux-x86_64")), REFUSED)

    contains(capsys.readouterr().err, naming)
    truth(not result.exists(), describing="no figure reported")


def test_an_api_nothing_listens_on_is_refused(
    result: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """An unreachable API is said to be unreachable."""
    with serving(Jobs(REPOSITORY, COMBINED, 1, document(COMBINED))) as address:
        pass

    equal(measure(environment(address, "gate", "linux-x86_64")), REFUSED)

    contains(capsys.readouterr().err, "could not be reached")
    truth(not result.exists(), describing="no figure reported")


@pytest.fixture
def checking(monkeypatch: pytest.MonkeyPatch) -> Iterator[str]:
    """The stand-in API, with the process environment a `gate-time` step's check runs under."""
    monkeypatch.delenv("ONEBUDGETSPEC_RESULT", raising=False)
    monkeypatch.setenv("PYTHONPATH", PACKAGES)
    with serving(Jobs(REPOSITORY, COMBINED, 1, document(COMBINED))) as address:
        yield address


def _step_environment(monkeypatch: pytest.MonkeyPatch, settings: dict[str, str]) -> dict[str, str]:
    for name, value in settings.items():
        monkeypatch.setenv(name, value)
    return settings


def test_the_check_passes_a_context_within_budget_its_breakdown_in_the_result(
    checking: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The real onebudgetspec check of `gate-time`: 1,559 s is within 1,800 s.

    The step says so in one line, and the per-job breakdown is the check
    result's own detail, where onebudgetspec carries it.
    """
    environ = _step_environment(monkeypatch, environment(checking, "gate", "linux-x86_64"))

    equal(check_budget(Repo(REPO_ROOT), environ), WITHIN)

    equal(capsys.readouterr().out, "gate-time: within: 1559 s against 1800 s\n")
    (checked,) = check(paths=["budgets.yaml"], ids=["gate-time"], cwd=REPO_ROOT).results
    equal((checked.verdict, checked.actual, checked.threshold), ("within", 1559.0, 1800.0))
    detail = json.loads(checked.detail or "{}")
    equal([job["name"] for job in detail["jobs"]], ["gate (linux-x86_64)"])
    equal(detail["initial_queue_seconds"], 3.0)
    truth(bool(detail["jobs"][0]["steps"]), describing="each step's recorded duration")


def test_the_check_fails_a_context_over_budget_on_a_pull_request(
    checking: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """2,579 s of `gate (macos-aarch64)` is over, and the required context fails."""
    environ = _step_environment(monkeypatch, environment(checking, "gate", "macos-aarch64"))

    equal(check_budget(Repo(REPO_ROOT), environ), OVER)

    said = capsys.readouterr()
    contains(said.err, "gate-time: over: 2579 s against 1800 s")
    contains(said.err, "sharding it behind its own required name (`just gate-shards`)")
    contains(said.out, '"name": "macos-aarch64 (gate, integration)"')


def test_the_release_sweep_reports_an_over_budget_figure_and_passes(
    checking: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """On the release pull request's full sweep the figure is reported and not enforced."""
    environ = _step_environment(
        monkeypatch,
        environment(
            checking, "gate", "macos-aarch64", GITHUB_HEAD_REF="release-plz-2026-10-25T21-00-00Z"
        ),
    )

    equal(check_budget(Repo(REPO_ROOT), environ), WITHIN)

    contains(
        capsys.readouterr().out,
        "gate-time: over, reported and not enforced on the release sweep: 2579 s against 1800 s",
    )


@pytest.mark.parametrize("event", ["push", "workflow_dispatch"])
def test_only_the_release_pull_request_is_spared(
    checking: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str], event: str
) -> None:
    """A push, and a dispatch by hand — which takes the `all` tier too — are enforced.

    The forge hands neither a head branch, so neither is the release pull request.
    """
    environ = _step_environment(
        monkeypatch,
        environment(checking, "gate", "macos-aarch64", GITHUB_EVENT_NAME=event, GITHUB_HEAD_REF=""),
    )

    equal(check_budget(Repo(REPO_ROOT), environ), OVER)

    contains(capsys.readouterr().err, "gate-time: over: 2579 s")


def test_the_check_refuses_by_name_with_no_run_id(
    checking: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """With no run named, the check errors naming the variable rather than passing on zero."""
    environ = _step_environment(monkeypatch, environment(checking, "gate", "linux-x86_64"))
    monkeypatch.delenv("GITHUB_RUN_ID")

    equal(check_budget(Repo(REPO_ROOT), environ), REFUSED)

    contains(capsys.readouterr().err, "GITHUB_RUN_ID is not set")


def test_the_check_refuses_a_context_whose_jobs_cannot_be_read(
    checking: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A context the run has no job for is an error of the check, on the release sweep too."""
    environ = _step_environment(
        monkeypatch,
        environment(checking, "gate", "freebsd-riscv64", GITHUB_HEAD_REF="release-plz-x"),
    )

    equal(check_budget(Repo(REPO_ROOT), environ), REFUSED)

    contains(capsys.readouterr().err, "gate (freebsd-riscv64): no job of that name")


def test_the_held_cli_is_the_one_the_sdk_brings() -> None:
    """`repo-policy.toml`'s onebudgetspec, the dev group's SDK pin and the lock name one release."""
    held = next(t for t in toolchain_tools(Repo(REPO_ROOT)) if t.command == "onebudgetspec")
    lock = tomllib.loads((REPO_ROOT / "uv.lock").read_text(encoding="utf-8"))
    locked = {p["name"]: p["version"] for p in lock["package"]}
    project = tomllib.loads((REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8"))

    equal(locked["onebudgetspec-cli"], held.version)
    equal(locked["onebudgetspec-sdk"], held.version)
    contains("\n".join(project["dependency-groups"]["dev"]), f"onebudgetspec-sdk=={held.version}")


def test_the_budget_is_registered_as_the_task_states() -> None:
    """Seconds, at most 1,800, no label, and its command the `gate_time_budget` subcommand."""
    (registered,) = list_budgets(paths=[REPO_ROOT / "budgets.yaml"]).budgets

    equal(registered.id, "gate-time")
    equal((registered.unit, registered.direction, registered.threshold), ("seconds", "max", 1800.0))
    equal(registered.labels, [])
    equal(registered.measure, "reported")
    equal(registered.command, ["uv", "run", "python", "-m", "repo_checks", "gate_time_budget"])


def test_the_token_goes_to_the_forge_s_api_and_never_to_another_address(
    result: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """With `GITHUB_TOKEN` set, a stand-in is read without it, and the forge's API with it.

    The forge's own address is the stand-in's for the second read, so the same
    real HTTP request the step makes shows what it carries.
    """
    credential = f"job-{secrets.token_hex(8)}"
    jobs = Jobs(REPOSITORY, COMBINED, 1, document(COMBINED))
    with serving(jobs) as address:
        settings = {**environment(address, "gate", "linux-x86_64"), "GITHUB_TOKEN": credential}
        equal(measure(settings), 0)
        monkeypatch.setattr(gate_time_budget, "FORGE_API", address)
        equal(measure(settings), 0)

    equal(jobs.authorized, [None, f"Bearer {credential}"])
    truth(result.is_file(), describing="a figure reported")


def test_the_forge_s_own_api_is_the_default_address() -> None:
    """Unset or blank, the address is the forge's; its trailing slash is not part of it."""
    equal(gate_time_budget.api_address({}), "https://api.github.com")
    equal(gate_time_budget.api_address({"GITHUB_API_URL": " "}), "https://api.github.com")
    equal(
        gate_time_budget.api_address({"GITHUB_API_URL": "https://api.github.com/"}),
        "https://api.github.com",
    )


def test_a_job_whose_steps_are_unrecorded_or_unreadable_still_has_its_span(result: Path) -> None:
    """Steps the forge left out, or recorded with no instant, break down to nothing."""
    derived = document(COMBINED)
    del job(derived, "gate (windows-aarch64)")["steps"]
    job(derived, "gate-shard (windows-aarch64, 1/4)")["steps"][0]["started_at"] = "soon"
    with serving(Jobs(REPOSITORY, COMBINED, 1, derived)) as address:
        equal(measure(environment(address, "gate", "windows-aarch64")), 0)

    value, detail = reported(result)
    equal(value, 1164.0)
    equal(detail["jobs"][0]["steps"], [])
    equal(detail["jobs"][1]["steps"][0]["seconds"], None)


def test_the_check_refuses_a_tree_registering_no_budget(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """With no `budgets.yaml` to check, the step errors with onebudgetspec's own words."""
    equal(check_budget(Repo(tmp_path), {}), REFUSED)

    contains(capsys.readouterr().err, "gate-time: refused:")
