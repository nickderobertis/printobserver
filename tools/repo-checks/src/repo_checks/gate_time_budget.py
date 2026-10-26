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
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Mapping, Sequence
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


def context(environ: Mapping[str, str]) -> tuple[Literal["gate", "integration"], str]:
    """The kind and platform of the required context this step measures.

    Raises:
        GateTimeError: If either is unset, or the kind is neither of the two.
    """
    kind = _required(environ, KIND, "required context")
    if kind not in KINDS:
        msg = f"{KIND}={kind!r} names no required context: it is one of {', '.join(KINDS)}"
        raise GateTimeError(msg)
    platform = _required(environ, PLATFORM, "required context")
    return kind, platform


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


def run_jobs(environ: Mapping[str, str]) -> tuple[str, str, list[Mapping[str, object]]]:
    """The run id, its attempt, and every job of that attempt, read off the jobs API.

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
    token = environ.get("GITHUB_TOKEN", "").strip() or None
    base = (
        f"{address}/repos/{urllib.parse.quote(owner)}/{urllib.parse.quote(name)}"
        f"/actions/runs/{run_id}/attempts/{attempt}/jobs"
    )
    jobs: list[Mapping[str, object]] = []
    page = 1
    while True:
        url = f"{base}?per_page={PAGE}&page={page}"
        document = _page(url, token if address == FORGE_API else None)
        total, listed = document.get("total_count"), document.get("jobs")
        if not isinstance(total, int) or not isinstance(listed, list):
            msg = f"{url} answered no `total_count` and `jobs`, so the jobs could not be read"
            raise GateTimeError(msg)
        entries = cast("list[object]", listed)
        jobs.extend(cast("Mapping[str, object]", job) for job in entries if isinstance(job, dict))
        if len(jobs) >= total or not entries:
            break
        page += 1
    if len(jobs) < total:
        msg = f"{base} answered {len(jobs)} of the {total} jobs it counts"
        raise GateTimeError(msg)
    return run_id, attempt, jobs


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
    """A job's recorded steps, in the order it ran them."""
    steps = entry.get("steps")
    if not isinstance(steps, list):
        return []
    return [
        cast("Mapping[str, object]", step)
        for step in cast("list[object]", steps)
        if isinstance(step, dict)
    ]


def budget_step_start(entry: Mapping[str, object]) -> datetime | None:
    """When the aggregating job's own `gate-time` step started, where the forge has recorded it.

    A step the forge skipped — its job failed before reaching it — never started,
    whatever instant the forge records beside it.
    """
    for step in reversed(_steps(entry)):
        if (
            step.get("name") == BUDGET
            and step.get("status") in {"in_progress", "completed"}
            and step.get("conclusion") != "skipped"
        ):
            return _instant(step.get("started_at"))
    return None


def measured(
    jobs: Sequence[Mapping[str, object]],
    kind: Literal["gate", "integration"],
    platform: str,
    now: datetime,
) -> tuple[float, dict[str, object]]:
    """The span of the context, in seconds, and the breakdown of it.

    Raises:
        GateTimeError: If `platform_span` cannot read the context's jobs.
    """
    try:
        span: GateSpan = platform_span(jobs, kind, platform)
    except GateJobsError as error:
        msg = f"{error}, so no gate-time figure is read rather than one of zero"
        raise GateTimeError(msg) from error
    entries = {entry.get("name"): entry for entry in jobs}
    aggregator = entries[span.jobs[0].name]
    step_start = budget_step_start(aggregator)
    completed = span.jobs[0].completed_at
    if step_start is not None:
        end, source = step_start, "step"
    elif completed is not None:
        end, source = completed, "completion"
    else:
        end, source = now, "clock"
    first_created = min(job.created_at for job in span.jobs)
    breakdown_jobs: list[dict[str, object]] = []
    for job in span.jobs:
        steps = []
        for step in _steps(entries[job.name]):
            began, ended = _instant(step.get("started_at")), _instant(step.get("completed_at"))
            seconds = (ended - began).total_seconds() if began and ended else None
            steps.append({"name": step.get("name"), "seconds": seconds})
        breakdown_jobs.append(
            {
                "name": job.name,
                "started_at": _stamp(job.started_at),
                "completed_at": _stamp(job.completed_at),
                "queue_seconds": job.queue_seconds,
                "steps": steps,
            }
        )
    seconds = span.seconds_to(end)
    detail: dict[str, object] = {
        "context": f"{kind} ({platform})",
        "kind": kind,
        "platform": platform,
        "seconds": seconds,
        "started_at": _stamp(span.started_at),
        "measured_to": _stamp(end),
        "measured_to_source": source,
        "initial_queue_seconds": max(0.0, (span.started_at - first_created).total_seconds()),
        "jobs": breakdown_jobs,
    }
    return seconds, detail


def measure(environ: Mapping[str, str] | None = None, now: datetime | None = None) -> int:
    """The budget's command: report the figure and its breakdown, or refuse naming why."""
    environment = os.environ if environ is None else environ
    try:
        kind, platform = context(environment)
        run_id, attempt, jobs = run_jobs(environment)
        seconds, detail = measured(jobs, kind, platform, now or datetime.now(UTC))
    except GateTimeError as error:
        print(f"gate-time: refused: {error}", file=sys.stderr)
        return REFUSED
    detail = {"run_id": int(run_id), "run_attempt": int(attempt), **detail}
    print(
        f"gate-time: {detail['context']} in run {run_id} attempt {attempt}: {seconds:.0f} s "
        f"from {detail['started_at']} to {detail['measured_to']}",
        file=sys.stderr,
    )
    report(seconds, json.dumps(detail, sort_keys=True))
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
    if result.detail:
        print(json.dumps(json.loads(result.detail), indent=2, sort_keys=True))
    if result.verdict == "error" or result.actual is None:
        print(f"gate-time: error: {result.error}", file=sys.stderr)
        return REFUSED
    figure = f"{result.actual:.0f} s against {result.threshold:.0f} s"
    if result.verdict == "within":
        print(f"gate-time: within: {figure}")
        return WITHIN
    if sweep:
        print(f"gate-time: over, reported and not enforced on the release sweep: {figure}")
        return WITHIN
    print(f"gate-time: over: {figure}", file=sys.stderr)
    return OVER
