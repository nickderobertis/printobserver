"""This repository's two CI lines: how long a pull-request push's longest job ran, and how many jobs it ran.

The root `budgets.yaml` registers both, and `ci.yml`'s `budgets` job checks them
once every other job of the run has ended. Three verbs of `python -m
repo_checks`, the first two the budgets' own commands, which `onebudgetspec
check` runs:

* `ci_check_time_budget` reports `ci-check-time`: the longest job of the current
  run, from its start to its end, over every job but the `budgets` job measuring
  it. A job no runner picked up — skipped by its condition, or cancelled before
  it started — ran for no time and is not one. Queue time is GitHub's shared
  capacity rather than this repository's work, so it is reported in the detail,
  per job, and never charged; so is the span on the clock from the first start
  to the last end.
* `ci_job_count_budget` reports `ci-job-count`: every job instance that took a
  runner across every workflow run the push started — the runs of the current
  run's commit and event — each matrix cell one, the `budgets` job itself
  counted. The detail names each job, its runner label and its conclusion, and
  the count per runner family.
* `ci_budgets`, through `just ci-budgets`, is the `budgets` job's one step: the
  onebudgetspec check of both. Over either line it fails on a pull request. On
  any other event it reports both figures and passes, because each line is a
  line over a pull-request push.

Every figure is read off GitHub's REST API as GitHub documents it — the current
run, the runs of its commit and event, and each run's jobs — and the suites
replay documents recorded from real runs. A read this cannot complete refuses,
naming what is missing, rather than report a figure of zero.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Mapping, Sequence
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from typing import cast

from onebudgetspec_sdk import OnebudgetspecError, check, report

from repo_checks.model import Repo

#: The two budgets' ids in the root `budgets.yaml`.
CHECK_TIME = "ci-check-time"
JOB_COUNT = "ci-job-count"
BUDGETS = "budgets.yaml"
#: The job that measures the run, by the name it reports under: the one job
#: `ci-check-time` leaves out, because it is still running while it measures.
MEASURING_JOB = "budgets"
#: The event whose push the two lines hold.
PULL_REQUEST = "pull_request"

#: The forge's own API, the only address the job's token is sent to.
FORGE_API = "https://api.github.com"
#: Hosts the jobs may be read from over plain HTTP: a suite's stand-in on loopback.
LOOPBACK = frozenset({"127.0.0.1", "::1"})
#: How many entries one page of a list answers at most.
PAGE = 100
#: Seconds one request waits for an answer before the read is refused.
TIMEOUT = 30

#: The statuses of a job no runner has picked up yet, and every status the
#: jobs API answers.
UNSTARTED = frozenset({"queued", "waiting", "pending", "requested"})
STATUSES = UNSTARTED | {"in_progress", "completed"}

#: Each runner family, by the prefix of the hosted-runner labels it answers to.
FAMILIES = (("Linux", "ubuntu"), ("Windows", "windows"), ("macOS", "macos"))

#: The exit statuses of `ci_budgets`: within, over, and a refusal or a measurement error.
WITHIN, OVER, REFUSED = 0, 1, 2


class BudgetError(ValueError):
    """Why a figure cannot be read, in words naming what is missing."""


@dataclass(frozen=True, slots=True)
class Forge:
    """Where the reads go: the API, the repository, and the token sent with them."""

    address: str
    repository: str
    token: str | None

    def url(self, path: str, **query: str | int) -> str:
        """The address of one read of this repository's Actions API."""
        owner, _, name = self.repository.partition("/")
        quoted = f"{urllib.parse.quote(owner)}/{urllib.parse.quote(name)}"
        suffix = f"?{urllib.parse.urlencode(query)}" if query else ""
        return f"{self.address}/repos/{quoted}/actions/{path}{suffix}"


@dataclass(frozen=True, slots=True)
class Run:
    """One workflow run: its id, the attempt read, its commit and its event."""

    run_id: int
    attempt: int
    head_sha: str
    event: str
    workflow: str


@dataclass(frozen=True, slots=True)
class Job:
    """One job of a run as the jobs API recorded it, its times narrowed to instants."""

    name: str
    workflow: str
    status: str
    conclusion: str | None
    labels: tuple[str, ...]
    created_at: datetime
    started_at: datetime | None
    completed_at: datetime | None
    steps: tuple[StepTime, ...]

    @property
    def ran(self) -> bool:
        """Whether a runner picked it up."""
        return self.started_at is not None

    def seconds(self, now: datetime) -> float:
        """How long it has run: to its end, or to `now` while it runs."""
        if self.started_at is None:
            return 0.0
        return max(0.0, ((self.completed_at or now) - self.started_at).total_seconds())

    @property
    def queue_seconds(self) -> float | None:
        """How long it waited for a runner, or none where none picked it up."""
        if self.started_at is None:
            return None
        return max(0.0, (self.started_at - self.created_at).total_seconds())

    @property
    def family(self) -> str:
        """The runner family its labels name, or `other`."""
        for family, prefix in FAMILIES:
            if any(label.startswith(prefix) for label in self.labels):
                return family
        return "other"


@dataclass(frozen=True, slots=True)
class StepTime:
    """One recorded step of a job, and how long it ran."""

    name: str
    seconds: float | None


@dataclass(frozen=True, slots=True)
class JobTime:
    """One job of the measured run as the `ci-check-time` detail carries it."""

    name: str
    started_at: str | None
    completed_at: str | None
    seconds: float
    queue_seconds: float | None
    steps: tuple[StepTime, ...]


@dataclass(frozen=True, slots=True)
class CheckTime:
    """The `ci-check-time` figure, and the breakdown reported beside it."""

    run_id: int
    run_attempt: int
    event: str
    #: The longest job's own running time: the figure.
    seconds: float
    longest: str | None
    #: From the first job's start to the last job's end, queues after that
    #: start included: reported, not the figure.
    wall_seconds: float
    jobs: tuple[JobTime, ...]

    def detail(self) -> str:
        """The breakdown as the JSON document onebudgetspec carries as the result's detail."""
        return json.dumps(asdict(self), sort_keys=True)


@dataclass(frozen=True, slots=True)
class CountedJob:
    """One job instance that took a runner, as the `ci-job-count` detail names it."""

    workflow: str
    name: str
    runner: str
    conclusion: str | None


@dataclass(frozen=True, slots=True)
class JobCount:
    """The `ci-job-count` figure, and the breakdown reported beside it."""

    head_sha: str
    event: str
    runs: tuple[int, ...]
    count: int
    per_family: Mapping[str, int]
    jobs: tuple[CountedJob, ...]

    def detail(self) -> str:
        """The breakdown as the JSON document onebudgetspec carries as the result's detail."""
        return json.dumps(asdict(self), sort_keys=True)


def _required(environ: Mapping[str, str], name: str, what: str) -> str:
    """One variable a read cannot do without.

    Raises:
        BudgetError: Naming it, if it is unset or empty.
    """
    value = environ.get(name, "").strip()
    if not value:
        msg = f"{name} is not set, so there is no {what} to read"
        raise BudgetError(msg)
    return value


def api_address(environ: Mapping[str, str]) -> str:
    """The API's address: the forge's, or a stand-in on loopback.

    Raises:
        BudgetError: If `GITHUB_API_URL` names any other address.
    """
    address = environ.get("GITHUB_API_URL", "").strip().rstrip("/") or FORGE_API
    if address == FORGE_API:
        return address
    try:
        parsed = urllib.parse.urlsplit(address)
        host = parsed.hostname
    except ValueError:
        host, parsed = None, None
    if parsed is None or parsed.scheme != "http" or host not in LOOPBACK:
        msg = (
            f"GITHUB_API_URL={address!r} is neither {FORGE_API} nor a stand-in on loopback, "
            f"so nothing is read from it"
        )
        raise BudgetError(msg)
    return address


def forge(environ: Mapping[str, str]) -> Forge:
    """Where this run's reads go, off the variables the forge hands every step.

    The job's token goes to the forge's own API and nowhere else.

    Raises:
        BudgetError: If no repository is named, or it is not `owner/name`.
    """
    repository = _required(environ, "GITHUB_REPOSITORY", "repository")
    owner, _, name = repository.partition("/")
    if not owner or not name or "/" in name:
        msg = f"GITHUB_REPOSITORY={repository!r} is not `owner/name`"
        raise BudgetError(msg)
    address = api_address(environ)
    token = (environ.get("GITHUB_TOKEN", "").strip() or None) if address == FORGE_API else None
    return Forge(address, repository, token)


def _page(url: str, token: str | None) -> Mapping[str, object]:
    """One answer of the API, as the forge answered it.

    Raises:
        BudgetError: If it does not answer, answers an error, or answers a body
            that is not a JSON object.
    """
    opener = urllib.request.build_opener()
    opener.addheaders = [
        ("Accept", "application/vnd.github+json"),
        ("User-Agent", "printobserver-ci-budgets"),
    ]
    if token is not None:
        opener.addheaders.append(("Authorization", f"Bearer {token}"))
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        with opener.open(url, timeout=TIMEOUT) as answer:
            body = answer.read()
    except urllib.error.HTTPError as error:
        msg = f"{url} answered {error.code} {error.reason}, so it could not be read"
        raise BudgetError(msg) from error
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        msg = f"{url} could not be reached ({error}), so it could not be read"
        raise BudgetError(msg) from error
    try:
        document = json.loads(body)
    except ValueError as error:
        msg = f"{url} answered a body that is not JSON, so it could not be read"
        raise BudgetError(msg) from error
    if not isinstance(document, dict):
        msg = f"{url} answered {type(document).__name__}, not a JSON object"
        raise BudgetError(msg)
    return cast("Mapping[str, object]", document)


def _listed(found: Forge, path: str, field: str, **query: str | int) -> list[Mapping[str, object]]:
    """Every entry of one paged list, `field` of each page, all pages read.

    Raises:
        BudgetError: If a page cannot be read, its count is not a count, its
            list is not a list of objects, or the pages answer fewer entries
            than the count says.
    """
    entries: list[Mapping[str, object]] = []
    page = 1
    while True:
        url = found.url(path, **query, per_page=PAGE, page=page)
        document = _page(url, found.token)
        total, listed = document.get("total_count"), document.get(field)
        if isinstance(total, bool) or not isinstance(total, int) or total < 0:
            msg = f"{url} answered `total_count` {total!r}, not a count"
            raise BudgetError(msg)
        if not isinstance(listed, list) or not all(
            isinstance(entry, dict) for entry in cast("list[object]", listed)
        ):
            msg = f"{url} answered no `{field}` list of objects"
            raise BudgetError(msg)
        entries.extend(cast("list[Mapping[str, object]]", listed))
        if len(entries) >= total or not listed:
            break
        page += 1
    if len(entries) != total:
        msg = f"{found.url(path, **query)} answered {len(entries)} of the {total} it counts"
        raise BudgetError(msg)
    return entries


def _number(environ: Mapping[str, str], name: str, default: str | None = None) -> int:
    """A variable naming a run or an attempt, as the number it must be.

    Raises:
        BudgetError: If it is unset with no default, or is not a whole number.
    """
    value = environ.get(name, "").strip() or default
    if value is None:
        msg = f"{name} is not set, so there is no run to read"
        raise BudgetError(msg)
    if not value.isdigit():
        msg = f"{name}={value!r} is not a number, so it names no run to read"
        raise BudgetError(msg)
    return int(value)


def _text(entry: Mapping[str, object], field: str, where: str) -> str:
    """One field the forge answers as text.

    Raises:
        BudgetError: If it is absent or not text.
    """
    value = entry.get(field)
    if not isinstance(value, str) or not value:
        msg = f"{where} answered `{field}` {value!r}, not text"
        raise BudgetError(msg)
    return value


def current_run(found: Forge, environ: Mapping[str, str]) -> Run:
    """The run `GITHUB_RUN_ID` names, at the attempt `GITHUB_RUN_ATTEMPT` names.

    Raises:
        BudgetError: If either is not a number, or the run cannot be read.
    """
    run_id = _number(environ, "GITHUB_RUN_ID")
    attempt = _number(environ, "GITHUB_RUN_ATTEMPT", "1")
    url = found.url(f"runs/{run_id}")
    answered = _page(url, found.token)
    return Run(
        run_id,
        attempt,
        _text(answered, "head_sha", url),
        _text(answered, "event", url),
        _text(answered, "name", url),
    )


def _instant(value: object, where: str, field: str) -> datetime | None:
    """A recorded timestamp, or none where the forge recorded none.

    Raises:
        BudgetError: If it is present and is not an ISO 8601 instant carrying
            its offset from UTC.
    """
    if value is None:
        return None
    if isinstance(value, str):
        try:
            instant = datetime.fromisoformat(value)
        except ValueError:
            pass
        else:
            if instant.tzinfo is not None and "T" in value.upper():
                return instant
    msg = f"{where} has {field}={value!r}, not an instant"
    raise BudgetError(msg)


def _steps(entry: Mapping[str, object], where: str) -> tuple[StepTime, ...]:
    """A job's recorded steps and how long each ran, none where it recorded none.

    Raises:
        BudgetError: If a step is not an object whose name is text.
    """
    steps = entry.get("steps")
    if steps is None:
        return ()
    if not isinstance(steps, list):
        msg = f"{where} answered `steps` {steps!r}, not a list"
        raise BudgetError(msg)
    read: list[StepTime] = []
    for step in cast("list[object]", steps):
        if not isinstance(step, dict) or not isinstance(step.get("name"), str):
            msg = f"{where} answered a step with no name"
            raise BudgetError(msg)
        recorded = cast("Mapping[str, object]", step)
        began = _instant(recorded.get("started_at"), where, "a step's started_at")
        ended = _instant(recorded.get("completed_at"), where, "a step's completed_at")
        seconds = (ended - began).total_seconds() if began and ended else None
        read.append(StepTime(cast("str", recorded["name"]), seconds))
    return tuple(read)


def job_of(entry: Mapping[str, object], workflow: str) -> Job:
    """One jobs-API entry, narrowed.

    A job the forge has not handed a runner yet is answered with `started_at`
    equal to its `created_at`; that is no start, so a job still queued, or one
    that completed with no runner — skipped, or cancelled before it started —
    is read as never having started.

    Raises:
        BudgetError: If it records no name, no creation, a status that says
            nothing of whether it started, or a time that is not an instant.
    """
    name = entry.get("name")
    where = f"job {name!r}"
    created = _instant(entry.get("created_at"), where, "created_at")
    if not isinstance(name, str) or created is None:
        msg = f"{where} records no name or no created_at"
        raise BudgetError(msg)
    status, runner = entry.get("status"), entry.get("runner_name")
    conclusion, labels = entry.get("conclusion"), entry.get("labels", [])
    if (
        not isinstance(status, str)
        or status not in STATUSES
        or not (runner is None or isinstance(runner, str))
        or not (conclusion is None or isinstance(conclusion, str))
        or not isinstance(labels, list)
        or not all(isinstance(label, str) for label in cast("list[object]", labels))
    ):
        msg = (
            f"{where} has status={status!r}, runner_name={runner!r}, conclusion="
            f"{conclusion!r} and labels={labels!r}, which say nothing of whether it ran"
        )
        raise BudgetError(msg)
    started = _instant(entry.get("started_at"), where, "started_at")
    if status in UNSTARTED or (status == "completed" and not runner):
        started = None
    return Job(
        name=name,
        workflow=workflow,
        status=status,
        conclusion=conclusion,
        labels=tuple(cast("list[str]", labels)),
        created_at=created,
        started_at=started,
        completed_at=_instant(entry.get("completed_at"), where, "completed_at"),
        steps=_steps(entry, where),
    )


def run_jobs(found: Forge, run: Run) -> tuple[Job, ...]:
    """Every job of one run's attempt, narrowed.

    Raises:
        BudgetError: If the jobs cannot all be read, or one cannot be narrowed.
    """
    entries = _listed(found, f"runs/{run.run_id}/attempts/{run.attempt}/jobs", "jobs")
    return tuple(job_of(entry, run.workflow) for entry in entries)


def _stamp(instant: datetime | None) -> str | None:
    """An instant as the forge writes one, or none."""
    return None if instant is None else instant.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def check_time(run: Run, jobs: Sequence[Job], now: datetime) -> CheckTime:
    """The longest job of `run` but the one measuring it, and the breakdown of the run."""
    measured = [job for job in jobs if job.ran and job.name != MEASURING_JOB]
    longest = max(measured, key=lambda job: job.seconds(now), default=None)
    starts = [job.started_at for job in measured if job.started_at is not None]
    ends = [job.completed_at or now for job in measured]
    wall = (max(ends) - min(starts)).total_seconds() if starts else 0.0
    return CheckTime(
        run_id=run.run_id,
        run_attempt=run.attempt,
        event=run.event,
        seconds=longest.seconds(now) if longest else 0.0,
        longest=longest.name if longest else None,
        wall_seconds=wall,
        jobs=tuple(
            JobTime(
                job.name,
                _stamp(job.started_at),
                _stamp(job.completed_at),
                job.seconds(now),
                job.queue_seconds,
                job.steps,
            )
            for job in jobs
            if job.name != MEASURING_JOB
        ),
    )


def sibling_runs(found: Forge, run: Run) -> list[Run]:
    """Every workflow run the push that started `run` started: the runs of its commit and event.

    Each is read at its latest attempt, and `run` itself at the attempt measuring it.

    Raises:
        BudgetError: If the runs cannot all be read, or `run` is not among them.
    """
    entries = _listed(found, "runs", "workflow_runs", head_sha=run.head_sha, event=run.event)
    runs: list[Run] = []
    for entry in entries:
        identifier, attempt = entry.get("id"), entry.get("run_attempt")
        if (
            isinstance(identifier, bool)
            or not isinstance(identifier, int)
            or isinstance(attempt, bool)
            or not isinstance(attempt, int)
        ):
            msg = f"a run of {run.head_sha[:12]} answered id={identifier!r}, run_attempt={attempt!r}"
            raise BudgetError(msg)
        if identifier == run.run_id:
            runs.append(run)
            continue
        where = f"run {identifier}"
        runs.append(
            Run(
                identifier,
                attempt,
                run.head_sha,
                _text(entry, "event", where),
                _text(entry, "name", where),
            )
        )
    if not any(one.run_id == run.run_id for one in runs):
        msg = f"the runs of {run.head_sha[:12]} on {run.event} do not include run {run.run_id}"
        raise BudgetError(msg)
    return sorted(runs, key=lambda one: one.run_id)


def job_count(run: Run, runs: Sequence[Run], jobs: Sequence[Job]) -> JobCount:
    """How many of `jobs`, across `runs`, took a runner, and which they are."""
    counted = [job for job in jobs if job.ran]
    families = {family: 0 for family, _ in FAMILIES}
    for job in counted:
        families[job.family] = families.get(job.family, 0) + 1
    return JobCount(
        head_sha=run.head_sha,
        event=run.event,
        runs=tuple(one.run_id for one in runs),
        count=len(counted),
        per_family=families,
        jobs=tuple(
            CountedJob(job.workflow, job.name, "+".join(job.labels), job.conclusion)
            for job in counted
        ),
    )


def measure_check_time(environ: Mapping[str, str] | None = None, now: datetime | None = None) -> int:
    """`ci-check-time`'s command: report the figure and its breakdown, or refuse naming why."""
    environment = os.environ if environ is None else environ
    try:
        found = forge(environment)
        run = current_run(found, environment)
        figure = check_time(run, run_jobs(found, run), now or datetime.now(UTC))
    except BudgetError as error:
        print(f"{CHECK_TIME}: refused: {error}", file=sys.stderr)
        return REFUSED
    print(
        f"{CHECK_TIME}: run {figure.run_id} attempt {figure.run_attempt} ({figure.event}): "
        f"{figure.seconds:.0f} s, its longest job {figure.longest or 'none'}",
        file=sys.stderr,
    )
    report(figure.seconds, figure.detail())
    return 0


def measure_job_count(environ: Mapping[str, str] | None = None) -> int:
    """`ci-job-count`'s command: report the figure and its breakdown, or refuse naming why."""
    environment = os.environ if environ is None else environ
    try:
        found = forge(environment)
        run = current_run(found, environment)
        runs = sibling_runs(found, run)
        jobs = [job for one in runs for job in run_jobs(found, one)]
        figure = job_count(run, runs, jobs)
    except BudgetError as error:
        print(f"{JOB_COUNT}: refused: {error}", file=sys.stderr)
        return REFUSED
    print(
        f"{JOB_COUNT}: {figure.count} jobs took a runner across runs "
        f"{', '.join(str(one) for one in figure.runs)} of {figure.head_sha[:12]} ({figure.event})",
        file=sys.stderr,
    )
    report(float(figure.count), figure.detail())
    return 0


def check_budgets(repo: Repo, environ: Mapping[str, str] | None = None) -> int:
    """The `budgets` job's step: both lines checked, enforced on a pull request alone."""
    environment = os.environ if environ is None else environ
    enforced = environment.get("GITHUB_EVENT_NAME", "") == PULL_REQUEST
    try:
        results = check(paths=[BUDGETS], ids=[CHECK_TIME, JOB_COUNT], cwd=repo.root).results
    except OnebudgetspecError as error:
        print(f"ci-budgets: refused: {error}", file=sys.stderr)
        return REFUSED
    exit_status = WITHIN
    for result in results:
        if result.verdict == "error" or result.actual is None:
            print(f"{result.id}: error: {result.error}", file=sys.stderr)
            exit_status = REFUSED
            continue
        figure = f"{result.actual:.0f} {result.unit} against {result.threshold:.0f}"
        if result.verdict == "within":
            print(f"{result.id}: within: {figure}")
            continue
        # Over the line, the breakdown is what says where the time or the jobs went.
        if result.detail:
            print(json.dumps(json.loads(result.detail), indent=2, sort_keys=True))
        if not enforced:
            print(f"{result.id}: over, reported and not enforced off a pull request: {figure}")
            continue
        print(
            f"{result.id}: over: {figure}. Read the breakdown above for the job that grew, "
            f"and bring it under by making that work faster or cheaper; no job is sharded "
            f"and the line is not loosened.",
            file=sys.stderr,
        )
        exit_status = max(exit_status, OVER)
    return exit_status
