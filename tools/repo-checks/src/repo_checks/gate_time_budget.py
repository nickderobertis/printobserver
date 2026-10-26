"""The `gate-time` budget: how long a pull request waits on one platform's required context.

Two verbs of `python -m repo_checks gate_time_budget`, one per side of `onebudgetspec`:

* `measure`, the default, is the budget's command,
  which `onebudgetspec check` runs. It reads the current run's jobs off GitHub's
  jobs API, names the context it runs in by the kind and platform its step hands
  it, reads that context's span with `gate_jobs.platform_span` — the one reading
  of which jobs are behind a context — and reports the figure and its breakdown
  through the onebudgetspec SDK. It refuses, naming what is missing, rather than
  report a figure it could not read: no run, no context, or jobs that no span
  can be read from.
* `check`, through `just gate-time`, is what the `gate-time` step of every required job runs: the
  onebudgetspec check of `gate-time`, which fails that required context over
  its threshold — except on the release pull request's full sweep, where the
  figure is reported and the step passes.

The span runs from the earliest real start of any job behind the context to the
start of the aggregating job's own `gate-time` step, read off that job's
recorded steps; while the forge has not recorded that step's start yet it runs
to the moment of measuring, which is that step's own. A run recorded before
the step existed is read to the aggregator's completion instead — the
retrospective span — so a finished run's figure is the one its context waited.
The queue before the first job started is reported, and is not part of the figure.

What the jobs API answers is read as GitHub documents it, and the suites replay
documents recorded from real runs. The producer is reconciled on every change
rather than on a schedule: each required job's own step reads the live API, and
an answer this cannot read fails that context naming what it could not read,
never a figure of zero.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Mapping
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from typing import Literal, cast

from onebudgetspec_sdk import OnebudgetspecError, check, report

from repo_checks import gate_tier
from repo_checks.gate_jobs import GateJobsError, GateSpan, platform_span
from repo_checks.model import Repo

#: The budget's id in the root `budgets.yaml`, and the name of the step that checks it.
BUDGET = "gate-time"
#: What that step runs, the last step of every job reporting a required context.
STEP_RUN = "just gate-time"
#: The budgets file registering it.
BUDGETS = "budgets.yaml"

#: What names the context a step measures: its kind, and the platform the job
#: already names for every recipe it runs.
KIND = "GATE_TIME_KIND"
PLATFORM = "PRINTOBSERVER_PLATFORM"
KINDS = ("gate", "integration")

#: The forge's own API, the only address the job's token is sent to.
FORGE_API = "https://api.github.com"
#: Hosts the jobs may be read from over plain HTTP: a suite's stand-in on loopback.
LOOPBACK = frozenset({"127.0.0.1", "::1"})
#: How many jobs one page of the jobs API answers at most.
PAGE = 100
#: Seconds one request waits for an answer before the read is refused.
TIMEOUT = 30

#: The exit statuses of `check`: within, over, and a refusal or a measurement error.
WITHIN, OVER, REFUSED = 0, 1, 2


class GateTimeError(ValueError):
    """Why no `gate-time` figure can be read, in words naming what is missing."""


@dataclass(frozen=True, slots=True)
class Context:
    """The required context a step measures: its kind and its platform."""

    kind: Literal["gate", "integration"]
    platform: str

    @property
    def name(self) -> str:
        """The context as a pull request reports it."""
        return f"{self.kind} ({self.platform})"


@dataclass(frozen=True, slots=True)
class RunJobs:
    """Every job of one attempt of the current run, as the jobs API answered them."""

    run_id: int
    attempt: int
    #: Each job as the forge's JSON, its `name` read as text; `platform_span`
    #: narrows every other field it reads.
    jobs: tuple[Mapping[str, object], ...]


@dataclass(frozen=True, slots=True)
class StepTime:
    """One recorded step of a job behind the context, and how long it ran."""

    name: str
    seconds: float | None


@dataclass(frozen=True, slots=True)
class JobTime:
    """One job behind the context: when it ran, how long it queued, and its steps."""

    name: str
    started_at: str | None
    completed_at: str | None
    queue_seconds: float | None
    steps: tuple[StepTime, ...]


@dataclass(frozen=True, slots=True)
class Measurement:
    """The figure for one context, and the breakdown the step reports beside it."""

    context: str
    kind: str
    platform: str
    run_id: int
    run_attempt: int
    seconds: float
    started_at: str | None
    measured_to: str | None
    #: `step` (the budget step's recorded start), `completion` (a finished
    #: run's aggregator) or `clock` (the moment of measuring).
    measured_to_source: str
    initial_queue_seconds: float
    jobs: tuple[JobTime, ...]

    def detail(self) -> str:
        """The breakdown as the JSON document onebudgetspec carries as the result's detail."""
        return json.dumps(asdict(self), sort_keys=True)


def _required(environ: Mapping[str, str], name: str, what: str) -> str:
    """One variable the measurement cannot do without.

    Raises:
        GateTimeError: Naming it, if it is unset or empty.
    """
    value = environ.get(name, "").strip()
    if not value:
        msg = f"{name} is not set, so there is no {what} to read the jobs of"
        raise GateTimeError(msg)
    return value


def context(environ: Mapping[str, str]) -> Context:
    """The required context this step measures.

    Raises:
        GateTimeError: If its kind or platform is unset, or the kind is neither of the two.
    """
    kind = _required(environ, KIND, "required context")
    if kind not in KINDS:
        msg = f"{KIND}={kind!r} names no required context: it is one of {', '.join(KINDS)}"
        raise GateTimeError(msg)
    return Context(kind, _required(environ, PLATFORM, "required context"))


def api_address(environ: Mapping[str, str]) -> str:
    """The jobs API's address: the forge's, or a stand-in on loopback.

    Raises:
        GateTimeError: If `GITHUB_API_URL` names any other address.
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
            f"so the jobs are not read from it"
        )
        raise GateTimeError(msg)
    return address


def _page(url: str, token: str | None) -> Mapping[str, object]:
    """One page of the jobs API, as the forge answered it.

    Raises:
        GateTimeError: If it does not answer, answers an error, or answers a
            body that is not a JSON object.
    """
    opener = urllib.request.build_opener()
    opener.addheaders = [
        ("Accept", "application/vnd.github+json"),
        ("User-Agent", "printobserver-gate-time"),
    ]
    if token is not None:
        opener.addheaders.append(("Authorization", f"Bearer {token}"))
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        with opener.open(url, timeout=TIMEOUT) as answer:
            body = answer.read()
    except urllib.error.HTTPError as error:
        msg = f"{url} answered {error.code} {error.reason}, so the jobs could not be read"
        raise GateTimeError(msg) from error
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        msg = f"{url} could not be reached ({error}), so the jobs could not be read"
        raise GateTimeError(msg) from error
    try:
        document = json.loads(body)
    except ValueError as error:
        msg = f"{url} answered a body that is not JSON, so the jobs could not be read"
        raise GateTimeError(msg) from error
    if not isinstance(document, dict):
        msg = f"{url} answered {type(document).__name__}, not a jobs document"
        raise GateTimeError(msg)
    return cast("Mapping[str, object]", document)


def run_jobs(environ: Mapping[str, str]) -> RunJobs:
    """Every job of the current run's attempt, read off the jobs API.

    The job's token goes to the forge's own API and nowhere else.

    Raises:
        GateTimeError: Naming what is missing, if no repository or run is named,
            the attempt is no number, or the jobs cannot all be read.
    """
    repository = _required(environ, "GITHUB_REPOSITORY", "repository")
    run_id = _required(environ, "GITHUB_RUN_ID", "run")
    attempt = environ.get("GITHUB_RUN_ATTEMPT", "").strip() or "1"
    for name, value in (("GITHUB_RUN_ID", run_id), ("GITHUB_RUN_ATTEMPT", attempt)):
        if not value.isdigit():
            msg = f"{name}={value!r} is not a number, so it names no run to read the jobs of"
            raise GateTimeError(msg)
    owner, _, name = repository.partition("/")
    if not owner or not name or "/" in name:
        msg = f"GITHUB_REPOSITORY={repository!r} is not `owner/name`"
        raise GateTimeError(msg)
    address = api_address(environ)
    token = (environ.get("GITHUB_TOKEN", "").strip() or None) if address == FORGE_API else None
    base = (
        f"{address}/repos/{urllib.parse.quote(owner)}/{urllib.parse.quote(name)}"
        f"/actions/runs/{run_id}/attempts/{attempt}/jobs"
    )
    jobs: list[Mapping[str, object]] = []
    page = 1
    while True:
        url = f"{base}?per_page={PAGE}&page={page}"
        listed, total = _jobs_of(url, _page(url, token))
        jobs.extend(listed)
        if len(jobs) >= total or not listed:
            break
        page += 1
    if len(jobs) != total:
        msg = f"{base} answered {len(jobs)} of the {total} jobs it counts"
        raise GateTimeError(msg)
    return RunJobs(int(run_id), int(attempt), tuple(jobs))


def _jobs_of(url: str, document: Mapping[str, object]) -> tuple[list[Mapping[str, object]], int]:
    """One page's jobs and the count the API answers beside them, each validated.

    Raises:
        GateTimeError: If the count is not a non-negative whole number, the jobs
            are not a list, or a job is not an object with a name.
    """
    total, listed = document.get("total_count"), document.get("jobs")
    if isinstance(total, bool) or not isinstance(total, int) or total < 0:
        msg = f"{url} answered `total_count` {total!r}, not a count of jobs"
        raise GateTimeError(msg)
    if not isinstance(listed, list):
        msg = f"{url} answered no `jobs` list, so the jobs could not be read"
        raise GateTimeError(msg)
    jobs: list[Mapping[str, object]] = []
    for position, entry in enumerate(cast("list[object]", listed)):
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            msg = f"{url} answered job {position} with no name, so the jobs could not be read"
            raise GateTimeError(msg)
        jobs.append(cast("Mapping[str, object]", entry))
    return jobs, total


def _instant(value: object) -> datetime | None:
    """A recorded timestamp, or none where the forge recorded none or no instant."""
    if not isinstance(value, str):
        return None
    try:
        instant = datetime.fromisoformat(value)
    except ValueError:
        return None
    return instant if instant.tzinfo is not None else None


def _stamp(instant: datetime | None) -> str | None:
    """An instant as the forge writes one, or none."""
    return None if instant is None else instant.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def _steps(entry: Mapping[str, object]) -> list[Mapping[str, object]]:
    """A job's recorded steps, in the order it ran them, none where it recorded none.

    Raises:
        GateTimeError: If a step is not an object whose name and status are text.
    """
    steps = entry.get("steps")
    if steps is None:
        return []
    if not isinstance(steps, list):
        msg = f"job {entry.get('name')!r} answered `steps` {steps!r}, not a list"
        raise GateTimeError(msg)
    read: list[Mapping[str, object]] = []
    for step in cast("list[object]", steps):
        if (
            not isinstance(step, dict)
            or not isinstance(step.get("name"), str)
            or not isinstance(step.get("status"), str)
        ):
            msg = f"job {entry.get('name')!r} answered a step with no name or status"
            raise GateTimeError(msg)
        read.append(cast("Mapping[str, object]", step))
    return read


def budget_step_start(entry: Mapping[str, object]) -> datetime | None:
    """When the aggregating job's own `gate-time` step started, where the forge has recorded it.

    A step the forge skipped — its job failed before reaching it — never started,
    whatever instant the forge records beside it.

    Raises:
        GateTimeError: If its steps cannot be read.
    """
    for step in reversed(_steps(entry)):
        if (
            step["name"] == BUDGET
            and step["status"] in {"in_progress", "completed"}
            and step.get("conclusion") != "skipped"
        ):
            return _instant(step.get("started_at"))
    return None


def measured(run: RunJobs, context: Context, now: datetime) -> Measurement:
    """The span of `context` in `run`, and the breakdown of it.

    Raises:
        GateTimeError: If `platform_span` cannot read the context's jobs, or a
            job's steps cannot be read.
    """
    try:
        span: GateSpan = platform_span(run.jobs, context.kind, context.platform)
    except GateJobsError as error:
        msg = f"{error}, so no gate-time figure is read rather than one of zero"
        raise GateTimeError(msg) from error
    entries = {cast("str", entry["name"]): entry for entry in run.jobs}
    step_start = budget_step_start(entries[span.jobs[0].name])
    completed = span.jobs[0].completed_at
    if step_start is not None:
        end, source = step_start, "step"
    elif completed is not None:
        end, source = completed, "completion"
    else:
        end, source = now, "clock"
    breakdown: list[JobTime] = []
    for job in span.jobs:
        steps: list[StepTime] = []
        for step in _steps(entries[job.name]):
            began, ended = _instant(step.get("started_at")), _instant(step.get("completed_at"))
            seconds = (ended - began).total_seconds() if began and ended else None
            steps.append(StepTime(cast("str", step["name"]), seconds))
        breakdown.append(
            JobTime(
                job.name,
                _stamp(job.started_at),
                _stamp(job.completed_at),
                job.queue_seconds,
                tuple(steps),
            )
        )
    first_created = min(job.created_at for job in span.jobs)
    return Measurement(
        context=context.name,
        kind=context.kind,
        platform=context.platform,
        run_id=run.run_id,
        run_attempt=run.attempt,
        seconds=span.seconds_to(end),
        started_at=_stamp(span.started_at),
        measured_to=_stamp(end),
        measured_to_source=source,
        initial_queue_seconds=max(0.0, (span.started_at - first_created).total_seconds()),
        jobs=tuple(breakdown),
    )


def measure(environ: Mapping[str, str] | None = None, now: datetime | None = None) -> int:
    """The budget's command: report the figure and its breakdown, or refuse naming why."""
    environment = os.environ if environ is None else environ
    try:
        named = context(environment)
        found = measured(run_jobs(environment), named, now or datetime.now(UTC))
    except GateTimeError as error:
        print(f"gate-time: refused: {error}", file=sys.stderr)
        return REFUSED
    print(
        f"gate-time: {found.context} in run {found.run_id} attempt {found.run_attempt}: "
        f"{found.seconds:.0f} s from {found.started_at} to {found.measured_to}",
        file=sys.stderr,
    )
    report(found.seconds, found.detail())
    return 0


def release_sweep(repo: Repo, environ: Mapping[str, str]) -> bool:
    """Whether this run is the release pull request's full sweep, where the figure is not enforced.

    Raises:
        gate_tier.TierError: If `release-plz.toml` names no release branch prefix.
    """
    head = environ.get("GITHUB_HEAD_REF", "")
    return gate_tier.select(repo, environ) == gate_tier.ALL and head.startswith(
        gate_tier.release_branch_prefix(repo)
    )


def check_budget(repo: Repo, environ: Mapping[str, str] | None = None) -> int:
    """The `gate-time` step: the onebudgetspec check of it, enforced but on the release sweep."""
    environment = os.environ if environ is None else environ
    try:
        result = check(paths=[BUDGETS], ids=[BUDGET], cwd=repo.root).results[0]
        sweep = release_sweep(repo, environment)
    except (OnebudgetspecError, gate_tier.TierError) as error:
        print(f"gate-time: refused: {error}", file=sys.stderr)
        return REFUSED
    if result.verdict == "error" or result.actual is None:
        print(f"gate-time: error: {result.error}", file=sys.stderr)
        return REFUSED
    figure = f"{result.actual:.0f} s against {result.threshold:.0f} s"
    if result.verdict == "within":
        print(f"gate-time: within: {figure}")
        return WITHIN
    # Over budget, the breakdown is what says where the time went.
    if result.detail:
        print(json.dumps(json.loads(result.detail), indent=2, sort_keys=True))
    if sweep:
        print(f"gate-time: over, reported and not enforced on the release sweep: {figure}")
        return WITHIN
    print(
        f"gate-time: over: {figure}. Read the breakdown above for the job and step that "
        f"grew, and bring the context under by making that work faster or by sharding it "
        f"behind its own required name (`just gate-shards`); the budget is not loosened.",
        file=sys.stderr,
    )
    return OVER
