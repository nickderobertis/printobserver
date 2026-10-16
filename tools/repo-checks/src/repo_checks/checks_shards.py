"""A sharded required job is its shards and one aggregating job keeping the required name.

A platform's `gate (<platform>)` may be split into shard jobs and an
aggregating job, as `repo_checks.gate_shards` describes. The required context
is then reported by the aggregator alone, so whether it is a verdict over the
whole of that platform's gate turns entirely on the workflow's shape, and this
is the check holding the workflow to it:

* a shard job's cells are one platform's, under `gate_jobs.SHARD_JOB`, with
  `shard` running 1 to one `shards` count a plan exists for, each on the runner
  the supported-platform list declares and each running `just check-shard`;
* exactly one aggregating job reports that platform's `gate_jobs.GATE_JOB`: it
  `needs` the shard job, runs `if: always()` so a failed or cancelled shard
  still reaches it, and runs nothing but `just shard-verdict` over the
  workflow's own `needs`, handed over as `${{ toJSON(needs) }}`;
* and the unsharded gate job's matrix leaves the platform out, so no second job
  reports the same context.

The integration tier has no selector splitting it, so an integration shard job
is refused rather than read.
"""

from __future__ import annotations

from typing import Any

from repo_checks.gate_jobs import GATE_JOB, INTEGRATION_SHARD_JOB, SHARD_JOB
from repo_checks.gate_shards import (
    NEEDS_VARIABLE,
    SHARD_RECIPE,
    VERDICT_RECIPE,
    ShardError,
    plan,
)
from repo_checks.model import Repo
from repo_checks.parsing import (
    MarkerBlockMissingError,
    jobs_of,
    load_workflow,
    run_commands,
    steps_of,
)
from repo_checks.platforms import supported

#: What a shard job's declared name starts with, for each kind.
GATE_SHARD_PREFIX = SHARD_JOB.split("{", 1)[0]
INTEGRATION_SHARD_PREFIX = INTEGRATION_SHARD_JOB.split("{", 1)[0]
#: A shard job's declared name, as the workflow writes it for its cells to render.
SHARD_NAME = SHARD_JOB.format(
    platform="${{ matrix.platform.id }}",
    index="${{ matrix.platform.shard }}",
    count="${{ matrix.platform.shards }}",
)
#: The value the aggregating job hands its verdict the `needs` context in.
NEEDS_VALUE = "${{ toJSON(needs) }}"
#: The condition that runs the aggregator whatever its shards concluded.
ALWAYS = "always()"


def is_shard_job(job: dict[str, Any]) -> bool:
    """Whether a workflow job is one shard job of a sharded required context."""
    name = job.get("name")
    return isinstance(name, str) and name.startswith((GATE_SHARD_PREFIX, INTEGRATION_SHARD_PREFIX))


def _cells(job: dict[str, Any]) -> list[dict[str, Any]]:
    """The `platform` matrix cells a job declares."""
    matrix = (job.get("strategy") or {}).get("matrix")
    entries = matrix.get("platform") if isinstance(matrix, dict) else None
    if not isinstance(entries, list):
        return []
    return [entry for entry in entries if isinstance(entry, dict)]


def sharded_platforms(repo: Repo) -> dict[str, dict[str, str]]:
    """Each workflow's sharded gate platforms, mapped to the shard job carrying them."""
    sharded: dict[str, dict[str, str]] = {}
    for path in repo.workflow_paths:
        for key, job in jobs_of(load_workflow(path)).items():
            if not is_shard_job(job):
                continue
            for cell in _cells(job):
                if isinstance(cell.get("id"), str):
                    sharded.setdefault(path.name, {})[cell["id"]] = key
    return sharded


def _needs(job: dict[str, Any]) -> list[str]:
    """The job keys a job `needs`, in either of the shapes a workflow writes them."""
    declared = job.get("needs")
    if isinstance(declared, str):
        return [declared]
    if isinstance(declared, list):
        return [str(one) for one in declared]
    return []


def _shard_findings(
    repo: Repo, where: str, job: dict[str, Any], runners: dict[str, str]
) -> tuple[str | None, list[str]]:
    """The one platform a shard job's cells carry, and what is wrong with them."""
    if job.get("name") != SHARD_NAME:
        return None, [f"{where} is a shard job not named `{SHARD_NAME}`"]
    cells = _cells(job)
    if not cells:
        return None, [f"{where} is a shard job with no `platform` matrix cells"]
    findings: list[str] = []
    platforms = {str(cell.get("id")) for cell in cells}
    if len(platforms) != 1:
        findings.append(
            f"{where}'s cells carry {', '.join(sorted(platforms))}: a shard job is one "
            f"platform's, so the result its aggregator reads is that platform's alone"
        )
    counts = {cell.get("shards") for cell in cells}
    indices = sorted(index for cell in cells if isinstance(index := cell.get("shard"), int))
    count = next(iter(counts)) if len(counts) == 1 else None
    if not isinstance(count, int) or indices != list(range(1, count + 1)):
        findings.append(
            f"{where}'s cells run shards {indices} of {sorted(map(str, counts))}: a shard job "
            f"runs every shard 1 to its count under one count"
        )
    else:
        try:
            plan(repo, count)
        except ShardError as error:
            findings.append(f"{where}: {error}")
    findings.extend(
        f"{where} runs `{cell.get('id')}` on `{cell.get('runner')}`, but AGENTS.md declares "
        f"`{runners[cell['id']]}`"
        for cell in cells
        if cell.get("id") in runners and cell.get("runner") != runners[cell["id"]]
    )
    if not any(command.startswith(f"just {SHARD_RECIPE} ") for command in run_commands(job)):
        findings.append(f"{where} runs no `just {SHARD_RECIPE}`, so its cells run no shard")
    platform = next(iter(platforms)) if len(platforms) == 1 else None
    return platform, findings


def _aggregator_findings(where: str, key: str, job: dict[str, Any], platform: str) -> list[str]:
    """The aggregating job waits on every shard and reaches its verdict over them alone."""
    findings: list[str] = []
    if key not in _needs(job):
        findings.append(f"{where} does not `needs` the shard job `{key}`")
    if str(job.get("if", "")).strip().removeprefix("${{").removesuffix("}}").strip() != ALWAYS:
        findings.append(
            f"{where} does not run `if: {ALWAYS}`, so a shard that failed or was cancelled "
            f"skips the verdict rather than failing it"
        )
    verdict = f"just {VERDICT_RECIPE} gate {platform}"
    if run_commands(job) != [verdict]:
        findings.append(
            f"{where} runs {run_commands(job)}, and the one thing an aggregating job runs "
            f"is `{verdict}`"
        )
    handed = [
        step.get("env", {}).get(NEEDS_VARIABLE)
        for step in steps_of(job)
        if isinstance(step.get("env"), dict)
    ]
    if NEEDS_VALUE not in handed:
        findings.append(
            f"{where} does not hand its verdict `{NEEDS_VARIABLE}: {NEEDS_VALUE}`, so it "
            f"reads no shard's result"
        )
    return findings


def sharded_jobs(repo: Repo) -> list[str]:
    """Every sharded required job is its shards and one aggregator over them."""
    try:
        runners = {platform.id: platform.runner for platform in supported(repo)}
    except MarkerBlockMissingError as error:
        return [str(error)]
    from repo_checks.checks_ci import WorkflowValueError, status_contexts

    try:
        contexts = status_contexts(repo)
    except WorkflowValueError as error:
        return [str(error)]
    findings: list[str] = []
    for path in repo.workflow_paths:
        jobs = jobs_of(load_workflow(path))
        for key, job in jobs.items():
            where = f"{path.name}: job `{key}`"
            if not is_shard_job(job):
                if VERDICT_RECIPE in " ".join(run_commands(job)) and not any(
                    is_shard_job(jobs.get(need, {})) for need in _needs(job)
                ):
                    findings.append(f"{where} runs `just {VERDICT_RECIPE}` and needs no shard job")
                continue
            if str(job.get("name")).startswith(INTEGRATION_SHARD_PREFIX):
                findings.append(
                    f"{where} shards the integration tier, which no selector splits: "
                    f"`repo_checks.gate_shards` plans the gate alone"
                )
                continue
            platform, found = _shard_findings(repo, where, job, runners)
            findings.extend(found)
            if platform is None:
                continue
            required = GATE_JOB.format(platform=platform)
            aggregators = [(name, one) for name, one in jobs.items() if one.get("name") == required]
            if len(aggregators) != 1:
                findings.append(
                    f"{where} shards `{required}`, which {len(aggregators)} jobs of "
                    f"{path.name} report; exactly one aggregating job reports it"
                )
                continue
            reported = [one for one in contexts if one.file == path.name and one.name == required]
            if len(reported) != 1:
                findings.append(
                    f"{where} shards `{required}`, and {len(reported)} cells of {path.name} "
                    f"report it: the unsharded job's matrix leaves a sharded platform out"
                )
            name, aggregator = aggregators[0]
            if _cells(aggregator):
                findings.append(
                    f"{path.name}: job `{name}` reports `{required}` from a platform matrix, "
                    f"so it is no aggregator of `{key}`'s shards"
                )
                continue
            findings.extend(
                _aggregator_findings(f"{path.name}: job `{name}`", key, aggregator, platform)
            )
    return findings
