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
from typing import Any

import pytest
from closures import closure
from repo_checks import gate_jobs
from repo_checks.checks_ci import status_contexts
from repo_checks.expect import contains, equal
from repo_checks.gate_jobs import (
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

CI = ".github/workflows/ci.yml"
#: The job keys whose cells report a gate or integration context.
KEYS = ("gate", "integration")


def recorded(run: int) -> list[dict[str, Any]]:
    """The `jobs` array of one recorded jobs-API document."""
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
        _pattern(one) for one in (GATE_JOB, SHARD_JOB, INTEGRATION_JOB, INTEGRATION_SHARD_JOB)
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
    contains("\n".join(findings), "`gate (windows-aarch64)` is a required name ci.yml does not")
