"""How long a pull request waits on one platform's required job, read off the forge.

Every platform reports two required contexts, `gate (<platform>)` and
`integration (<platform>)`. Either may be one job, or — where its work does not
fit one runner in the time a platform is held to — shard jobs and an aggregating
job that keeps the required name and `needs` every shard. Whichever it is, what a
pull request waits on is the span from the first of those jobs to start to the
aggregator's end, and this module is the one reading of that span: the four name
formats are declared here and nowhere else, `ci.yml`'s job names are held to them
by a test, and whatever measures or budgets the wait imports `platform_span`
rather than restating it.

The input is the `jobs` array GitHub's
`GET /repos/{owner}/{repo}/actions/runs/{run_id}/attempts/{attempt}/jobs` answers.
A job the forge has not handed a runner yet is answered there with `started_at`
equal to its `created_at`; that is no start, so a job still `queued` (or
`waiting`, `pending`, `requested`), or one that completed with no runner — skipped,
or cancelled before it started — is read as never having started. A span begins
at the earliest real start, so the queue before a platform's first job starts is
not part of it and is reported per job as `queue_seconds`, while a shard that
queued after another had started is inside it.
"""

from __future__ import annotations

import re
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import datetime
from typing import Literal

__all__ = [
    "GATE_JOB",
    "INTEGRATION_JOB",
    "INTEGRATION_SHARD_JOB",
    "SHARD_JOB",
    "GateJob",
    "GateJobsError",
    "GateSpan",
    "platform_gate",
    "platform_span",
]

#: The required gate context of one platform.
GATE_JOB = "gate ({platform})"
#: One shard of a sharded gate, `index` from 1 to `count`.
SHARD_JOB = "gate-shard ({platform}, {index}/{count})"
#: The required integration context of one platform.
INTEGRATION_JOB = "integration ({platform})"
#: One shard of a sharded integration job, `index` from 1 to `count`.
INTEGRATION_SHARD_JOB = "integration-shard ({platform}, {index}/{count})"

#: Each kind's required name and shard name.
_FORMATS: dict[str, tuple[str, str]] = {
    "gate": (GATE_JOB, SHARD_JOB),
    "integration": (INTEGRATION_JOB, INTEGRATION_SHARD_JOB),
}

#: The statuses of a job no runner has picked up yet.
_UNSTARTED = frozenset({"queued", "waiting", "pending", "requested"})


class GateJobsError(ValueError):
    """A platform's jobs of one kind that no span can be read from."""


@dataclass(frozen=True, slots=True)
class GateJob:
    """One job of a platform's required context: the aggregator or one shard."""

    name: str
    created_at: datetime
    started_at: datetime | None
    completed_at: datetime | None

    @property
    def queue_seconds(self) -> float | None:
        """How long it waited for a runner, or none before one picked it up."""
        if self.started_at is None:
            return None
        return (self.started_at - self.created_at).total_seconds()

    @property
    def duration_seconds(self) -> float | None:
        """How long it ran, or none before it both started and completed."""
        if self.started_at is None or self.completed_at is None:
            return None
        return (self.completed_at - self.started_at).total_seconds()


@dataclass(frozen=True, slots=True)
class GateSpan:
    """The critical path one platform's required context of one kind is waited on for."""

    platform: str
    kind: str
    #: The earliest recorded start among the platform's jobs of this kind.
    started_at: datetime
    #: The aggregator — the job carrying the required name — and then its shards.
    jobs: tuple[GateJob, ...]

    def seconds_to(self, now: datetime) -> float:
        """The span so far: from its first start to `now`, as a job still inside it reads it."""
        return (now - self.started_at).total_seconds()

    @property
    def completed_seconds(self) -> float | None:
        """The whole span, to the aggregator's completion, or none before it completes."""
        completed = self.jobs[0].completed_at
        if completed is None:
            return None
        return (completed - self.started_at).total_seconds()


def _pattern(template: str) -> re.Pattern[str]:
    """A name format as a pattern capturing its fields."""
    escaped = re.escape(template)
    for field, capture in (
        ("platform", r"(?P<platform>[^,()]+)"),
        ("index", r"(?P<index>[1-9][0-9]*)"),
        ("count", r"(?P<count>[1-9][0-9]*)"),
    ):
        escaped = escaped.replace(re.escape("{" + field + "}"), capture)
    return re.compile(f"^{escaped}$")


def _instant(job: Mapping[str, object], field: str, kind: str, platform: str) -> datetime | None:
    """A job's timestamp, or none where the forge recorded none.

    Raises:
        GateJobsError: If it is present and is not an ISO 8601 instant carrying
            its offset from UTC: a date, or a time of day with no zone, names no
            one moment to subtract another from.
    """
    value = job.get(field)
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
    msg = f"{kind} ({platform}): job {job.get('name')!r} has {field}={value!r}, not an instant"
    raise GateJobsError(msg)


def _job(job: Mapping[str, object], kind: str, platform: str) -> GateJob:
    """One jobs-API entry read as a `GateJob`.

    Raises:
        GateJobsError: If it records no creation, or a timestamp that is not an instant.
    """
    name = job.get("name")
    created = _instant(job, "created_at", kind, platform)
    if not isinstance(name, str) or created is None:
        msg = f"{kind} ({platform}): job {name!r} records no name or no created_at"
        raise GateJobsError(msg)
    started = _instant(job, "started_at", kind, platform)
    if job.get("status") in _UNSTARTED or (
        job.get("status") == "completed" and not job.get("runner_name")
    ):
        started = None
    return GateJob(name, created, started, _instant(job, "completed_at", kind, platform))


def platform_span(
    jobs: Sequence[Mapping[str, object]],
    kind: Literal["gate", "integration"],
    platform: str,
) -> GateSpan:
    """The span of `platform`'s required job of `kind`, and of every shard of it.

    Raises:
        GateJobsError: Naming the kind and platform, if the job carrying the
            required name is absent or reported twice, none of its jobs has
            started, or its shards' indices are not 1 to their count under one
            count.
    """
    required_name, shard_name = _FORMATS[kind]
    required = required_name.format(platform=platform)
    shard_pattern = _pattern(shard_name)
    aggregator: GateJob | None = None
    shards: dict[int, GateJob] = {}
    counts: set[int] = set()
    for entry in jobs:
        name = entry.get("name")
        if name == required:
            if aggregator is not None:
                msg = f"{required}: reported twice, so which is its verdict is not known"
                raise GateJobsError(msg)
            aggregator = _job(entry, kind, platform)
            continue
        matched = shard_pattern.match(name) if isinstance(name, str) else None
        if matched is None or matched["platform"] != platform:
            continue
        index = int(matched["index"])
        if index in shards:
            msg = f"{required}: shard {index} is reported twice"
            raise GateJobsError(msg)
        shards[index] = _job(entry, kind, platform)
        counts.add(int(matched["count"]))
    if aggregator is None:
        msg = f"{required}: no job of that name, so the {kind} of {platform} has no verdict"
        raise GateJobsError(msg)
    if shards and (len(counts) != 1 or sorted(shards) != list(range(1, max(counts) + 1))):
        msg = (
            f"{required}: shard indices {sorted(shards)} under counts {sorted(counts)} are not "
            f"1 to their count under one count"
        )
        raise GateJobsError(msg)
    span_jobs = (aggregator, *(shards[index] for index in sorted(shards)))
    starts = [job.started_at for job in span_jobs if job.started_at is not None]
    if not starts:
        msg = f"{required}: no job of the {kind} of {platform} has started"
        raise GateJobsError(msg)
    return GateSpan(platform, kind, min(starts), span_jobs)


def platform_gate(jobs: Sequence[Mapping[str, object]], platform: str) -> GateSpan:
    """The span of `platform`'s gate: `platform_span(jobs, "gate", platform)`.

    Raises:
        GateJobsError: As `platform_span` does.
    """
    return platform_span(jobs, "gate", platform)
