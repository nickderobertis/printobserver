"""The platform-dispatch workflow: one job, one platform, by hand, and nothing else.

`repo-policy.toml`'s `[dispatch]` names a workflow whose every job is a copy of a
platform-matrixed job of one of two source workflows, with the matrix replaced
by the one cell a manual dispatch names in its two inputs. A copy is what makes
a single hosted cell runnable on a branch without a push starting every cell of
every job — and a copy is exactly what drifts, so this check holds each one to
its source rather than trusting it:

* the workflow fires on `workflow_dispatch` and on nothing else, because a copy
  of the gate that also ran on a pull request would be a second gate;
* the platform input's options are exactly the supported-platform list, and the
  job input's are exactly the platform-matrixed jobs of the source workflows;
* every job is selected by the job input naming it, and one job per option;
* every job runs on the runner `AGENTS.md`'s list declares for the platform the
  dispatch named, read off the one map every job resolves the platform through;
* every job's steps are its source job's steps, and it carries nothing else.

Every other check that classifies a job by its steps reads this workflow not at
all — `dispatch_workflow` is how they know which one to pass over — because
classified by their steps its jobs would be a second gate and a second
integration tier with no matrix of their own.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from repo_checks.checks_obico import triggers_of
from repo_checks.model import Repo, policy_table
from repo_checks.parsing import MarkerBlockMissingError, jobs_of, load_workflow
from repo_checks.platforms import supported

#: How one job of the dispatch workflow selects itself: the job input naming
#: it, and nothing else in the condition.
SELECTION = re.compile(
    r"^\s*inputs\.(?P<input>[A-Za-z_][A-Za-z0-9_-]*)\s*==\s*'(?P<job>[^']*)'\s*$"
)

#: How every job resolves the platform named to a runner: one JSON map, indexed
#: by the platform input. The map is read back here and held to the list.
RUNNER_MAP = re.compile(
    r"^\s*\$\{\{\s*fromJSON\('(?P<map>[^']*)'\)\[inputs\.(?P<input>[A-Za-z_][A-Za-z0-9_-]*)\]\s*\}\}\s*$"
)

#: What one job of the dispatch workflow may carry: its name, what selects it,
#: where it runs and what it runs. Anything else — a matrix, a `needs`, an
#: `env` — is something the source job's cell did not have on its own.
PERMITTED_KEYS = frozenset({"name", "if", "runs-on", "steps"})


@dataclass(frozen=True, slots=True)
class Declared:
    """What `[dispatch]` says, narrowed once."""

    workflow: str
    job_input: str
    platform_input: str
    sources: tuple[str, ...]

    @classmethod
    def read(cls, repo: Repo) -> Declared | str:
        """The declaration, or the one finding saying what it is missing."""
        table = policy_table(repo, "dispatch")
        if not table:
            return "`repo-policy.toml` declares no `[dispatch]` section"
        found = {
            key: table[key].strip() if isinstance(table.get(key), str) else ""
            for key in ("workflow", "job_input", "platform_input")
        }
        listed = table.get("sources")
        sources = (
            tuple(str(one).strip() for one in listed)
            if isinstance(listed, list) and listed and all(isinstance(one, str) for one in listed)
            else ()
        )
        missing = sorted(key for key, value in found.items() if not value)
        if not sources:
            missing.append("sources")
        if missing:
            return (
                f"`repo-policy.toml`'s `[dispatch]` declares no {', '.join(sorted(missing))}, so "
                f"nothing can say which workflow runs one job on one platform by hand"
            )
        return cls(found["workflow"], found["job_input"], found["platform_input"], sources)


def dispatch_workflow(repo: Repo) -> Path | None:
    """The committed workflow `[dispatch]` names, for the checks that pass it over.

    `None` where the declaration is absent or names nothing that is there —
    then there is nothing to pass over, and `platform_dispatch` is what says
    so.
    """
    declared = Declared.read(repo)
    if isinstance(declared, str):
        return None
    path = repo.path(f".github/workflows/{declared.workflow}")
    return path if path.is_file() else None


def matrixed_jobs(repo: Repo, sources: tuple[str, ...]) -> dict[str, tuple[str, dict[str, Any]]]:
    """Every platform-matrixed job of the source workflows, by name.

    `Any` at the deserialization boundary: a job is whatever the YAML reader
    handed back, and what this check reads out of one is narrowed where it is
    read.
    """
    from repo_checks.checks_ci import _matrix_platforms

    found: dict[str, tuple[str, dict[str, Any]]] = {}
    for source in sources:
        path = repo.path(f".github/workflows/{source}")
        if not path.is_file():
            continue
        for name, job in jobs_of(load_workflow(path)).items():
            if _matrix_platforms(job) is not None:
                found[name] = (source, job)
    return found


def platform_dispatch(repo: Repo) -> list[str]:
    """One job of one source workflow on one supported platform, by hand, and no other."""
    declared = Declared.read(repo)
    if isinstance(declared, str):
        return [declared]
    relative = f".github/workflows/{declared.workflow}"
    findings = [
        f"`repo-policy.toml`'s `[dispatch]` names {source} as a source workflow, and this "
        f"repository commits no such workflow"
        for source in declared.sources
        if not repo.exists(f".github/workflows/{source}")
    ]
    if declared.workflow in declared.sources:
        findings.append(
            f"`repo-policy.toml`'s `[dispatch]` names {relative} as its own source, and a "
            f"workflow's jobs cannot be copies of themselves"
        )
    if not repo.exists(relative):
        return [
            *findings,
            f"the committed configuration declares no platform-dispatch workflow: "
            f"`repo-policy.toml` names {relative}, which is not there",
        ]
    try:
        platforms = {platform.id: platform.runner for platform in supported(repo)}
    except MarkerBlockMissingError as error:
        return [*findings, str(error)]

    workflow = load_workflow(repo.path(relative))
    triggers = triggers_of(workflow)
    findings.extend(
        f"{relative} fires on `{event}`: a workflow whose jobs are copies of the gate and "
        f"the integration tier may fire on a manual dispatch and on nothing else, or it "
        f"is a second gate"
        for event in sorted(triggers)
        if event != "workflow_dispatch"
    )
    if "workflow_dispatch" not in triggers:
        return [
            *findings,
            f"{relative} declares no `workflow_dispatch` trigger, which is the whole of "
            f"what it is for",
        ]

    inputs = triggers.get("workflow_dispatch")
    inputs = inputs.get("inputs") if isinstance(inputs, dict) else None
    inputs = inputs if isinstance(inputs, dict) else {}
    sources = matrixed_jobs(repo, declared.sources)
    findings.extend(
        _input_findings(
            relative,
            inputs,
            declared.platform_input,
            list(platforms),
            "the platform",
            "AGENTS.md's supported-platform list names",
        )
    )
    findings.extend(
        _input_findings(
            relative,
            inputs,
            declared.job_input,
            list(sources),
            "the job",
            f"the platform-matrixed jobs of {', '.join(declared.sources)} are",
        )
    )
    findings.extend(_job_findings(declared, relative, workflow, platforms, sources))
    return findings


def _options(inputs: dict[str, Any], name: str) -> list[str] | None:
    """The options one choice input offers, or `None` where it is not one."""
    declared = inputs.get(name)
    if not isinstance(declared, dict) or declared.get("type") != "choice":
        return None
    listed = declared.get("options")
    if not isinstance(listed, list):
        return None
    return [str(one) for one in listed]


def _input_findings(
    relative: str,
    inputs: dict[str, Any],
    name: str,
    wanted: list[str],
    what: str,
    whose: str,
) -> list[str]:
    """One choice input offers exactly `wanted`, and a caller must name one.

    `whose` says where `wanted` comes from, in the words a finding names it by.
    """
    options = _options(inputs, name)
    if options is None:
        return [
            f"{relative} declares no `{name}` input of type `choice` with options on its "
            f"manual dispatch, so a caller cannot choose {what} to run"
        ]
    findings = [
        f"{relative}'s `{name}` input offers `{extra}`, which is not one {whose}"
        for extra in options
        if extra not in wanted
    ]
    findings.extend(
        f"{relative}'s `{name}` input does not offer `{missing}`, which is one {whose}"
        for missing in wanted
        if missing not in options
    )
    findings.extend(
        f"{relative}'s `{name}` input offers `{twice}` more than once"
        for twice in sorted({one for one in options if options.count(one) > 1})
    )
    if not (isinstance(inputs.get(name), dict) and inputs[name].get("required") is True):
        findings.append(
            f"{relative}'s `{name}` input is not required, and a dispatch naming no "
            f"{what.removeprefix('the ')} would run on a default nobody chose"
        )
    return findings


def _job_findings(
    declared: Declared,
    relative: str,
    workflow: dict[str, Any],
    platforms: dict[str, str],
    sources: dict[str, tuple[str, dict[str, Any]]],
) -> list[str]:
    """Every job is a selected, single-cell copy of its source, and every option has one."""
    findings: list[str] = []
    selected: dict[str, str] = {}
    for name, job in jobs_of(workflow).items():
        where = f"{relative}: job `{name}`"
        extra = sorted(set(job) - PERMITTED_KEYS)
        if extra:
            findings.append(
                f"{where} carries {', '.join(f'`{key}`' for key in extra)}, which the one "
                f"cell of its source job does not: a copy carries its name, what selects "
                f"it, where it runs and what it runs"
            )
        match = SELECTION.match(str(job.get("if", "")))
        if match is None or match["input"] != declared.job_input:
            findings.append(
                f"{where} is not selected by `inputs.{declared.job_input} == '<job>'` alone "
                f"(its condition is `{job.get('if', '')}`), so a dispatch naming another "
                f"job could run it too"
            )
        else:
            chosen = match["job"]
            if chosen in selected:
                findings.append(
                    f"{where} is selected by `{chosen}`, which already selects job "
                    f"`{selected[chosen]}`: a dispatch naming it would run both"
                )
            selected.setdefault(chosen, name)
            if chosen not in sources:
                findings.append(
                    f"{where} is selected by `{chosen}`, which is no platform-matrixed job "
                    f"of {', '.join(declared.sources)}"
                )
            else:
                source, original = sources[chosen]
                if job.get("steps") != original.get("steps"):
                    findings.append(
                        f"{where} runs steps other than {source}'s job `{chosen}` runs: a "
                        f"copy that drifted from its source runs something the source "
                        f"never did"
                    )
        findings.extend(_runner_findings(declared, where, job, platforms))
    findings.extend(
        f"{relative} declares no job selected by `inputs.{declared.job_input} == "
        f"'{option}'`, so a dispatch naming `{option}` runs nothing"
        for option in sources
        if option not in selected
    )
    return findings


def _runner_findings(
    declared: Declared, where: str, job: dict[str, Any], platforms: dict[str, str]
) -> list[str]:
    """The job runs on the runner the list declares for the platform the dispatch named."""
    match = RUNNER_MAP.match(str(job.get("runs-on", "")))
    if match is None or match["input"] != declared.platform_input:
        return [
            f"{where} does not resolve `inputs.{declared.platform_input}` to a runner through "
            f"one `fromJSON('<map>')[inputs.{declared.platform_input}]` expression "
            f"(its `runs-on` is `{job.get('runs-on', '')}`)"
        ]
    try:
        mapped = json.loads(match["map"])
    except json.JSONDecodeError:
        return [f"{where} resolves the platform through a map that is not JSON"]
    if not isinstance(mapped, dict):
        return [f"{where} resolves the platform through something other than a map"]
    findings = [
        f"{where} maps `{platform}` to `{runner}`, and AGENTS.md declares `{platforms[platform]}`"
        for platform, runner in mapped.items()
        if platform in platforms and runner != platforms[platform]
    ]
    findings.extend(
        f"{where} maps `{platform}`, which AGENTS.md's supported-platform list does not name"
        for platform in mapped
        if platform not in platforms
    )
    findings.extend(
        f"{where} maps no runner for `{platform}`, which AGENTS.md's supported-platform list "
        f"names, so a dispatch naming it runs nowhere"
        for platform in platforms
        if platform not in mapped
    )
    return findings
