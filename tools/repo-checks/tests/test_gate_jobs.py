"""One platform's required-job span, read off jobs-API documents recorded from real runs.

Each document under `recorded/` is the forge's own answer to
`GET /repos/{owner}/{repo}/actions/runs/{run_id}/attempts/{attempt}/jobs` for the
run its name carries, saved as it came back. A refusal is driven by removing a
job from one of those documents, never by a document nobody recorded. What is
asserted is what `platform_span` reports from them, never a timing of the test's
own. The committed `ci.yml` is read for the other half: that the names its gate
and integration jobs report are the ones these formats read.
"""

from __future__ import annotations

import json
import re
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Literal

import pytest
from closures import closure
from repo_checks import gate_jobs
from repo_checks.checks_ci import status_contexts
from repo_checks.expect import contains, equal
from repo_checks.gate_jobs import (
    COMBINED_JOB,
    GATE_JOB,
    INTEGRATION_JOB,
    INTEGRATION_SHARD_JOB,
    SHARD_JOB,
    GateJob,
    GateJobsError,
    platform_gate,
    platform_span,
)
from repo_checks.model import Repo
from repo_checks.platforms import supported
from treecopy import Tree

#: The files the workflow half of this module reads; see `closures`.
CLOSURE = closure("ci")

RECORDED = Path(__file__).parent / "recorded"
#: A finished run of `main` at 0719ba3: every gate and integration job completed,
#: and `integration (macos-aarch64)` queued 1,369 s for a runner before it started.
COMPLETED = 38694510250
#: A run of `main` at e69dc35 read while it ran: every Linux and Windows job in
#: progress, and both macOS jobs still queued.
IN_FLIGHT = 38710213811
#: A finished run of this change's draft at fc06115, with `gate (windows-aarch64)`
#: split into two shards: each queued 2 s, and the aggregator queued 60 s for its
#: runner after the last shard completed.
SHARDED = 38887035697
#: A finished pull-request run at bb2fc63: both Windows gates in four shards, and
#: the combined `macos-aarch64 (gate, integration)` job running both macOS lanes
#: for 2,573 s, its two contexts carried from Linux after it.
COMBINED = 40235536077
#: A finished push to `main` at b75c483: the classifier owed no macOS lane, so the
#: combined job was skipped and both macOS contexts carried that omission.
OMITTED = 40239073093

CI = ".github/workflows/ci.yml"
#: The job keys whose cells report a gate or integration context, or the
#: combined job behind a platform's two.
KEYS = ("gate", "integration", "macos")


def recorded(run: int) -> list[dict[str, Any]]:
    """The `jobs` array of one recorded jobs-API document.

    `Any` because it is the forge's JSON as `json.loads` hands it back, which is
    exactly what `platform_span` is given in use and narrows itself.
    """
    document = json.loads((RECORDED / f"jobs-{run}-attempt-1.json").read_text(encoding="utf-8"))
    return document["jobs"]


def at(stamp: str) -> datetime:
    """An instant as the forge writes one."""
    return datetime.fromisoformat(stamp).replace(tzinfo=UTC)


def test_the_module_exposes_exactly_the_contract() -> None:
    """The names the budget consumer imports, and no others."""
    equal(
        sorted(gate_jobs.__all__),
        sorted(
            [
                "COMBINED_JOB",
                "GATE_JOB",
                "SHARD_JOB",
                "INTEGRATION_JOB",
                "INTEGRATION_SHARD_JOB",
                "GateJob",
                "GateSpan",
                "platform_span",
                "platform_gate",
                "GateJobsError",
            ]
        ),
    )
    equal(GATE_JOB.format(platform="p"), "gate (p)")
    equal(SHARD_JOB.format(platform="p", index=1, count=2), "gate-shard (p, 1/2)")
    equal(INTEGRATION_JOB.format(platform="p"), "integration (p)")
    equal(
        INTEGRATION_SHARD_JOB.format(platform="p", index=2, count=2), "integration-shard (p, 2/2)"
    )
    equal(COMBINED_JOB.format(platform="p"), "p (gate, integration)")


def test_an_unsharded_gate_spans_its_one_job_from_its_start_to_its_completion() -> None:
    """`gate (windows-x86_64)` of the finished run: two seconds queued, 2,764 s run."""
    span = platform_gate(recorded(COMPLETED), "windows-x86_64")

    equal(span.platform, "windows-x86_64")
    equal(span.kind, "gate")
    equal(span.started_at, at("2026-10-15T06:47:11"))
    equal(
        span.jobs,
        (
            GateJob(
                "gate (windows-x86_64)",
                at("2026-10-15T06:47:09"),
                at("2026-10-15T06:47:11"),
                at("2026-10-15T07:33:15"),
            ),
        ),
    )
    equal(span.jobs[0].queue_seconds, 2.0)
    equal(span.jobs[0].duration_seconds, 2764.0)
    equal(span.completed_seconds, 2764.0)
    equal(span.seconds_to(at("2026-10-15T07:17:11")), 1800.0)


def test_an_integration_job_s_queue_before_it_starts_is_reported_and_not_spanned() -> None:
    """`integration (macos-aarch64)` waited 1,369 s for a runner; its span is the 2,138 s it ran."""
    span = platform_span(recorded(COMPLETED), "integration", "macos-aarch64")

    equal(span.kind, "integration")
    equal(span.started_at, at("2026-10-15T07:09:58"))
    equal([job.name for job in span.jobs], ["integration (macos-aarch64)"])
    equal(span.jobs[0].queue_seconds, 1369.0)
    equal(span.jobs[0].duration_seconds, 2138.0)
    equal(span.completed_seconds, 2138.0)


def test_a_job_still_running_has_a_span_so_far_and_no_completed_one() -> None:
    """Read in flight, a gate's span is measured to `now`, and its completion is absent."""
    jobs = recorded(IN_FLIGHT)
    gate = platform_gate(jobs, "windows-aarch64")
    integration = platform_span(jobs, "integration", "windows-x86_64")

    equal(gate.started_at, at("2026-10-15T09:04:14"))
    equal(gate.jobs[0].queue_seconds, 1.0)
    equal(gate.jobs[0].duration_seconds, None)
    equal(gate.completed_seconds, None)
    equal(gate.seconds_to(at("2026-10-15T09:34:14")), 1800.0)
    equal(integration.started_at, at("2026-10-15T09:04:48"))
    equal(integration.jobs[0].queue_seconds, 35.0)
    equal(integration.seconds_to(at("2026-10-15T09:05:48")), 60.0)


def test_platform_gate_is_the_gate_kind_of_platform_span() -> None:
    """The budget's call and the general one answer the same span."""
    jobs = recorded(COMPLETED)
    for platform in ("linux-x86_64", "macos-aarch64", "windows-aarch64"):
        equal(platform_gate(jobs, platform), platform_span(jobs, "gate", platform))


def test_a_job_the_forge_has_not_handed_a_runner_has_not_started() -> None:
    """A queued job's `started_at` is its creation echoed back, so no span is read off it."""
    jobs = recorded(IN_FLIGHT)
    queued = next(job for job in jobs if job["name"] == "gate (macos-aarch64)")
    equal(queued["started_at"], queued["created_at"], describing="the forge's placeholder")

    for kind in ("gate", "integration"):
        with pytest.raises(GateJobsError) as refused:
            platform_span(jobs, kind, "macos-aarch64")
        contains(str(refused.value), f"{kind} (macos-aarch64)")
        contains(str(refused.value), "has started")


def test_a_platform_whose_required_job_is_absent_is_refused_naming_it() -> None:
    """With `gate (linux-x86_64)` removed from the finished run, there is no verdict to span."""
    jobs = [job for job in recorded(COMPLETED) if job["name"] != "gate (linux-x86_64)"]

    with pytest.raises(GateJobsError) as refused:
        platform_gate(jobs, "linux-x86_64")
    contains(str(refused.value), "gate (linux-x86_64)")
    contains(str(refused.value), "no job of that name")

    with pytest.raises(GateJobsError) as missing:
        platform_span(recorded(COMPLETED), "integration", "freebsd-riscv64")
    contains(str(missing.value), "integration (freebsd-riscv64)")


def test_a_sharded_gate_spans_its_first_shard_start_to_its_aggregator_s_completion() -> None:
    """The critical path a pull request waits on, the queue between shards included."""
    span = platform_gate(recorded(SHARDED), "windows-aarch64")

    equal(span.started_at, at("2026-10-16T10:24:38"))
    equal(
        [job.name for job in span.jobs],
        [
            "gate (windows-aarch64)",
            "gate-shard (windows-aarch64, 1/2)",
            "gate-shard (windows-aarch64, 2/2)",
        ],
    )
    equal([job.queue_seconds for job in span.jobs], [60.0, 2.0, 2.0])
    equal([job.duration_seconds for job in span.jobs], [9.0, 2071.0, 2134.0])
    equal(span.completed_seconds, 2203.0)
    equal(span.seconds_to(at("2026-10-16T11:00:12")), 2134.0)
    equal(platform_gate(recorded(SHARDED), "windows-x86_64").jobs[0].name, "gate (windows-x86_64)")


def test_a_sharded_gate_whose_aggregator_is_absent_is_refused_naming_it() -> None:
    """Without the job carrying the required name its shards have no verdict to span."""
    jobs = [job for job in recorded(SHARDED) if job["name"] != "gate (windows-aarch64)"]

    with pytest.raises(GateJobsError) as refused:
        platform_gate(jobs, "windows-aarch64")
    contains(str(refused.value), "gate (windows-aarch64)")
    contains(str(refused.value), "no job of that name")


def test_a_sharded_gate_missing_a_shard_is_refused_naming_it() -> None:
    """Shards 1 of 2 alone are not 1 to their count, so no span is read off them."""
    jobs = [job for job in recorded(SHARDED) if job["name"] != "gate-shard (windows-aarch64, 2/2)"]

    with pytest.raises(GateJobsError) as refused:
        platform_gate(jobs, "windows-aarch64")
    contains(str(refused.value), "gate (windows-aarch64)")
    contains(str(refused.value), "not 1 to their count")


@pytest.mark.parametrize(
    ("kind", "carrier_completed", "seconds"),
    [("gate", "22:11:21", 2581.0), ("integration", "22:11:22", 2582.0)],
)
def test_both_macos_contexts_span_the_combined_job_and_their_carrier(
    kind: Literal["gate", "integration"], carrier_completed: str, seconds: float
) -> None:
    """Each macOS context is waited on from the combined job's start to its own carrier's end."""
    span = platform_span(recorded(COMBINED), kind, "macos-aarch64")

    equal(span.started_at, at("2026-10-25T21:28:20"))
    equal(
        [job.name for job in span.jobs],
        [f"{kind} (macos-aarch64)", "macos-aarch64 (gate, integration)"],
    )
    equal(span.jobs[1].queue_seconds, 3.0)
    equal(span.jobs[1].duration_seconds, 2573.0)
    equal(span.jobs[0].completed_at, at(f"2026-10-25T{carrier_completed}"))
    equal(span.completed_seconds, seconds)


def test_a_skipped_combined_job_leaves_the_carrier_its_own_span() -> None:
    """A run owing no macOS lane skipped the combined job, which never started."""
    span = platform_gate(recorded(OMITTED), "macos-aarch64")

    equal([job.started_at for job in span.jobs], [at("2026-10-25T22:12:51"), None])
    equal(span.started_at, at("2026-10-25T22:12:51"))
    equal(span.completed_seconds, 8.0)


def test_the_combined_job_reported_twice_is_refused_naming_it() -> None:
    """Two combined jobs are two answers to which ran the platform's lanes."""
    jobs = recorded(COMBINED)
    combined = next(job for job in jobs if job["name"] == "macos-aarch64 (gate, integration)")

    with pytest.raises(GateJobsError) as refused:
        platform_span([*jobs, combined], "integration", "macos-aarch64")
    contains(
        str(refused.value),
        "integration (macos-aarch64): macos-aarch64 (gate, integration) is reported twice",
    )


def test_another_platform_s_combined_job_is_not_behind_this_one() -> None:
    """Linux's gate is its one job, whatever macOS combined."""
    span = platform_gate(recorded(COMBINED), "linux-x86_64")

    equal([job.name for job in span.jobs], ["gate (linux-x86_64)"])


def _with(run: int, name: str, **fields: object) -> list[dict[str, Any]]:
    """One recorded document's jobs, with job `name`'s `fields` replaced.

    As `Any`-valued as the document `recorded` reads, for the reason it gives.
    """
    return [{**job, **fields} if job["name"] == name else job for job in recorded(run)]


@pytest.mark.parametrize(
    "stamp", ["2026-10-16T10:24:38", "2026-10-16", "yesterday", 1792154016, ["2026-10-16"]]
)
def test_a_timestamp_that_is_no_instant_is_refused_naming_the_job(stamp: object) -> None:
    """No zone, no time, no date, or no string: none of them names one moment."""
    jobs = _with(SHARDED, "gate-shard (windows-aarch64, 1/2)", started_at=stamp)

    with pytest.raises(GateJobsError) as refused:
        platform_gate(jobs, "windows-aarch64")
    contains(str(refused.value), "gate (windows-aarch64)")
    contains(str(refused.value), "'gate-shard (windows-aarch64, 1/2)' has started_at=")
    contains(str(refused.value), "not an instant")


@pytest.mark.parametrize(
    "fields",
    [{"status": "teleported"}, {"status": ["completed"]}, {"status": None}, {"runner_name": 7}],
)
def test_a_status_or_runner_that_says_nothing_of_a_start_is_refused(
    fields: dict[str, object],
) -> None:
    """A status the jobs API never answers, or a runner that is no name, is not read as a start."""
    jobs = _with(SHARDED, "gate-shard (windows-aarch64, 2/2)", **fields)

    with pytest.raises(GateJobsError) as refused:
        platform_gate(jobs, "windows-aarch64")
    contains(str(refused.value), "'gate-shard (windows-aarch64, 2/2)' has status=")
    contains(str(refused.value), "which say nothing of whether it started")


def test_a_job_recording_no_creation_is_refused() -> None:
    """The aggregator with no `created_at` is no job a queue or a span is read from."""
    jobs = _with(SHARDED, "gate (windows-aarch64)", created_at=None)

    with pytest.raises(GateJobsError) as refused:
        platform_gate(jobs, "windows-aarch64")
    contains(str(refused.value), "records no name or no created_at")


def test_a_job_completed_with_no_runner_never_started() -> None:
    """A cancelled or skipped aggregator ran nowhere: its shards' start begins the span."""
    jobs = _with(
        SHARDED,
        "gate (windows-aarch64)",
        conclusion="skipped",
        runner_name=None,
        started_at="2026-10-16T11:00:12Z",
        completed_at="2026-10-16T11:00:12Z",
    )

    span = platform_gate(jobs, "windows-aarch64")

    equal(span.jobs[0].started_at, None)
    equal(span.jobs[0].queue_seconds, None)
    equal(span.jobs[0].duration_seconds, None)
    equal(span.started_at, at("2026-10-16T10:24:38"))
    equal(span.completed_seconds, 2134.0)


def test_a_required_job_reported_twice_is_refused_naming_it() -> None:
    """Two jobs under the required name are two verdicts, and which spans is not known."""
    jobs = recorded(SHARDED)
    aggregator = next(job for job in jobs if job["name"] == "gate (windows-aarch64)")

    with pytest.raises(GateJobsError) as refused:
        platform_gate([*jobs, aggregator], "windows-aarch64")
    contains(str(refused.value), "gate (windows-aarch64): reported twice")


def test_a_shard_reported_twice_is_refused_naming_it() -> None:
    """Two jobs under one shard's name are two answers to which ran it."""
    jobs = recorded(SHARDED)
    shard = next(job for job in jobs if job["name"] == "gate-shard (windows-aarch64, 1/2)")

    with pytest.raises(GateJobsError) as refused:
        platform_gate([*jobs, shard], "windows-aarch64")
    contains(str(refused.value), "gate (windows-aarch64): shard 1 is reported twice")


def _pattern(template: str) -> re.Pattern[str]:
    """One of the module's name formats as a pattern over a rendered name."""
    escaped = re.escape(template)
    for field in ("platform", "index", "count"):
        escaped = escaped.replace(re.escape("{" + field + "}"), r"[^,()]+")
    return re.compile(f"^{escaped}$")


def drifted(repo: Repo) -> list[str]:
    """Every gate or integration context `ci.yml` reports that the formats do not read.

    And every supported platform whose required gate or integration name it
    does not report at all, since a name the formats cannot find is the same
    drift seen from the other side.
    """
    formats = [
        _pattern(one)
        for one in (GATE_JOB, SHARD_JOB, INTEGRATION_JOB, INTEGRATION_SHARD_JOB, COMBINED_JOB)
    ]
    reported = [
        context.name
        for context in status_contexts(repo)
        if context.file == "ci.yml" and context.job.startswith(KEYS)
    ]
    findings = [
        f"`{name}` matches none of the gate_jobs formats"
        for name in reported
        if not any(one.match(name) for one in formats)
    ]
    for platform in supported(repo):
        for required in (GATE_JOB, INTEGRATION_JOB):
            name = required.format(platform=platform.id)
            if name not in reported:
                findings.append(f"`{name}` is a required name ci.yml does not report")
    return findings


def test_ci_reports_its_gate_and_integration_jobs_under_the_formats(committed: Repo) -> None:
    """Every gate and integration context the committed workflow reports, read by the formats."""
    equal(drifted(committed), [], describing="ci.yml's gate and integration job names")


def test_a_job_name_that_parts_ways_with_the_formats_is_drift(tree: Callable[[], Tree]) -> None:
    """Renaming the gate's cells is found from both sides: unread, and the required name gone."""
    copy = tree()
    copy.edit(
        CI, "name: gate (${{ matrix.platform.id }})", "name: gate [${{ matrix.platform.id }}]"
    )

    findings = drifted(copy.repo)

    contains("\n".join(findings), "`gate [linux-x86_64]` matches none of the gate_jobs formats")
    contains("\n".join(findings), "`gate (linux-aarch64)` is a required name ci.yml does not")
