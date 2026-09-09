"""The printer integration tier's continuous-integration job.

The tier drives a real OctoPrint, which is why it is not one of `just check`'s
tiers and why it needs a job of its own. This check holds that job to three
sources it does not own: the recipe set (every step it runs is one of this
repository's recipes, the bring-up and the bring-down among them), `AGENTS.md`'s
supported-platform list (every platform it names, minus the exclusions recorded
beside it, each with a reason), and the change events the base branch receives
(every one of them, with no branch, path or condition narrowing it below every
change).

The supported-platform list is a source this node does not own, so two of the
findings here are about the list rather than the job: a list that *gained* a
platform refuses an unchanged matrix, and a list that *lost* one it carried at
the base revision is refused outright. A check that compared the job only
against the list beside it would be satisfied by editing both.

One finding here is about the graph rather than the job, and it is about the
one thing this tier has that no other tier has: a single physical machine.
`just octoprint-up` starts one OctoPrint with one virtual printer, and every
project's `test-integration` target drives *that* printer — one of them cancels
the print another is asserting on. So the tier's target is declared
unparallelisable, and this check refuses a graph that leaves two of them free to
run at once. It cost a publication to learn: the adapter's tier cancelled the
hold print while `octoprint-env`'s tier was part-way through asserting the print
was there, and the tier that failed was the one that had done nothing wrong.
"""

from __future__ import annotations

import json
import re
from typing import Any

from repo_checks.checks_ci import PLATFORM_LINE, Platform, platforms_of
from repo_checks.model import Repo
from repo_checks.parsing import (
    MarkerBlockMissingError,
    jobs_of,
    load_workflow,
    marker_block,
    recipes,
    run_commands,
    steps_of,
)
from repo_checks.shell import run

# One recorded exclusion: a platform the virtual printer is unavailable on, and
# the reason it is. A line that starts like one and does not carry a reason is
# refused rather than ignored.
EXCLUSION = re.compile(r"^- `(?P<id>[^`]*)`(?: — (?P<reason>.*))?$")
EXCLUSION_SHAPE = "- `<platform>` — <why the virtual printer is unavailable there>"

# The change events the base branch receives. A job that ran on a subset of
# these does not run on every change.
CHANGE_EVENTS = ("pull_request", "push")
PATH_FILTERS = ("paths", "paths-ignore")
BRANCH_FILTERS = ("branches", "branches-ignore")


def integration_tier(repo: Repo, base: str | None = None) -> list[str]:
    """The committed job runs the tier, on every platform, on every change."""
    policy = repo.policy.get("integration")
    if not policy:
        return ["`repo-policy.toml` declares no `[integration]` section"]

    findings = _recipe_findings(repo, policy)
    findings.extend(_one_machine_findings(repo, policy))
    try:
        declared = platforms_of(repo)
        excluded, shape = _exclusions(repo, policy)
    except MarkerBlockMissingError as error:
        return [*findings, str(error)]
    findings.extend(shape)
    if not declared:
        return [*findings, "AGENTS.md's supported-platform list is empty"]
    findings.extend(
        f"AGENTS.md records the virtual printer as unavailable on `{name}`, which its "
        f"supported-platform list does not name"
        for name in sorted(excluded)
        if name not in {platform.id for platform in declared}
    )

    found = _integration_job(repo, str(policy["bring_up"]))
    if found is None:
        return [
            *findings,
            f"the committed configuration declares no printer-integration job "
            f"(a job whose steps run `just {policy['bring_up']}`)",
        ]
    file_name, job_name, job, workflow = found
    where = f"{file_name}: the integration job `{job_name}`"
    findings.extend(_step_findings(repo, policy, job, where))
    findings.extend(_matrix_findings(declared, excluded, job, where))
    findings.extend(_trigger_findings(repo, workflow, job, file_name, where))
    findings.extend(_narrowing_findings(repo, base, declared))
    return findings


def _recipe_findings(repo: Repo, policy: dict[str, Any]) -> list[str]:
    """The three recipes `repo-policy.toml` names are recipes the justfile declares."""
    declared = set(recipes(repo.justfile))
    return [
        f"`repo-policy.toml` names `just {policy[key]}` as the integration tier's "
        f"{key.replace('_', ' ')}, which the recipe set does not declare"
        for key in ("tier", "bring_up", "bring_down")
        if str(policy[key]) not in declared
    ]


def _one_machine_findings(repo: Repo, policy: dict[str, Any]) -> list[str]:
    """No two of the tier's tasks run at once, because there is one printer."""
    tier = str(policy["tier"])
    why = (
        f"the tier drives the one printer `just {policy['bring_up']}` starts, so two of its "
        f"tasks running at once drive one machine from two places"
    )
    graph = json.loads(repo.read("nx.json"))
    findings: list[str] = []
    if ((graph.get("targetDefaults") or {}).get(tier) or {}).get("parallelism") is not False:
        findings.append(
            f'nx.json does not declare the `{tier}` target `"parallelism": false`: {why}'
        )
    for path in repo.project_paths:
        project = json.loads(path.read_text(encoding="utf-8"))
        target = (project.get("targets") or {}).get(tier)
        if isinstance(target, dict) and target.get("parallelism") is not None:
            findings.append(
                f"{project.get('name', path.parent.name)}'s `{tier}` target overrides "
                f"`parallelism`, which nx.json declares for every project: {why}"
            )
    return findings


def _exclusions(repo: Repo, policy: dict[str, Any]) -> tuple[dict[str, str], list[str]]:
    """The platforms the virtual printer is recorded unavailable on, and their reasons.

    Raises:
        MarkerBlockMissingError: If `AGENTS.md` carries no such block.
    """
    excluded: dict[str, str] = {}
    findings: list[str] = []
    for line in marker_block(repo.agents_md, str(policy["exclusions_block"])):
        if not line.startswith("- "):
            continue
        match = EXCLUSION.match(line)
        if match is None:
            findings.append(
                f"AGENTS.md records the exclusion `{line}`, which is not of the form "
                f"`{EXCLUSION_SHAPE}`"
            )
            continue
        reason = (match["reason"] or "").strip()
        if not reason:
            findings.append(
                f"AGENTS.md records `{match['id']}` as a platform where the virtual "
                f"printer is unavailable, with no reason it is"
            )
            continue
        excluded[match["id"]] = reason
    return excluded, findings


def _integration_job(
    repo: Repo, bring_up: str
) -> tuple[str, str, dict[str, Any], dict[str, Any]] | None:
    """The committed job that brings the environment up, if there is one."""
    for path in repo.workflow_paths:
        workflow = load_workflow(path)
        for job_name, job in jobs_of(workflow).items():
            if f"just {bring_up}" in run_commands(job):
                return path.name, job_name, job, workflow
    return None


def _step_findings(
    repo: Repo, policy: dict[str, Any], job: dict[str, Any], where: str
) -> list[str]:
    """Every step runs a declared recipe, the bring-up and bring-down among them."""
    declared = set(recipes(repo.justfile))
    commands = run_commands(job)
    findings: list[str] = []
    for command in commands:
        words = command.split()
        if words[:1] != ["just"] or len(words) < 2:
            findings.append(
                f"{where} runs `{command}`, which is not one of this repository's recipes"
            )
        elif words[1] not in declared:
            findings.append(
                f"{where} runs `just {words[1]}`, which the recipe set does not declare"
            )

    required = [str(policy["bring_up"]), str(policy["tier"]), str(policy["bring_down"])]
    positions = [
        commands.index(f"just {recipe}") if f"just {recipe}" in commands else -1
        for recipe in required
    ]
    findings.extend(
        f"{where} does not run `just {recipe}`"
        for recipe, position in zip(required, positions, strict=True)
        if position < 0
    )
    if all(position >= 0 for position in positions) and positions != sorted(positions):
        findings.append(
            f"{where} runs {', '.join(f'`just {recipe}`' for recipe in required)} out of "
            f"order: the tier runs between the bring-up and the bring-down"
        )
    return findings


def _matrix_findings(
    declared: list[Platform], excluded: dict[str, str], job: dict[str, Any], where: str
) -> list[str]:
    """The matrix is the supported-platform list, minus the recorded exclusions."""
    matrix = (job.get("strategy") or {}).get("matrix")
    entries = matrix.get("platform") if isinstance(matrix, dict) else None
    if not isinstance(entries, list):
        return [f"{where} declares no platform matrix"]
    named = [entry.get("id") for entry in entries if isinstance(entry, dict)]
    declared_ids = [platform.id for platform in declared]

    findings = [
        f"{where}'s matrix names platform `{found}`, which AGENTS.md's "
        f"supported-platform list does not"
        for found in named
        if found not in declared_ids
    ]
    findings.extend(
        f"{where}'s matrix omits platform `{wanted}`, which AGENTS.md's "
        f"supported-platform list names and no exclusion records"
        for wanted in declared_ids
        if wanted not in named and wanted not in excluded
    )
    findings.extend(
        f"{where}'s matrix names platform `{found}`, which AGENTS.md records as one "
        f"where the virtual printer is unavailable ({excluded[str(found)]})"
        for found in named
        if str(found) in excluded
    )
    return findings


def _triggers(workflow: dict[str, Any]) -> dict[str, Any]:
    """The events a workflow fires on, whatever shape they are written in."""
    match workflow.get("on"):
        case str() as event:
            return {event: None}
        case list() as events:
            return dict.fromkeys(str(event) for event in events)
        case dict() as mapping:
            return mapping
        case _:
            return {}


def _trigger_findings(
    repo: Repo, workflow: dict[str, Any], job: dict[str, Any], file_name: str, where: str
) -> list[str]:
    """Nothing narrows the job below every change to this repository."""
    base_branch = str(repo.policy["repository"]["base_branch"])
    triggers = _triggers(workflow)
    findings = [
        f"{file_name} does not fire on `{event}`, which is one of the change events "
        f"the base branch receives, so {where} does not run on every change"
        for event in CHANGE_EVENTS
        if event not in triggers
    ]

    for event, spec in triggers.items():
        if not isinstance(spec, dict):
            continue
        findings.extend(
            f"{file_name}'s `{event}` trigger is restricted by changed path "
            f"(`{filtered}`), so {where} does not run on every change"
            for filtered in PATH_FILTERS
            if filtered in spec
        )
        for filtered in BRANCH_FILTERS:
            branches = spec.get(filtered)
            if branches is None:
                continue
            named = [str(branch) for branch in branches] if isinstance(branches, list) else []
            if filtered == "branches" and event == "push" and base_branch in named:
                # Every change lands on the base branch, so a push trigger that
                # names it is not a narrowing: it is that branch.
                continue
            findings.append(
                f"{file_name}'s `{event}` trigger is restricted to branches "
                f"({', '.join(named) or branches}), so {where} does not run on every change"
            )

    if "if" in job:
        findings.append(
            f"{where} carries the condition `{job['if']}`, which skips it for some changes"
        )
    findings.extend(_step_condition_findings(repo, job, where))
    return findings


def _step_condition_findings(repo: Repo, job: dict[str, Any], where: str) -> list[str]:
    """No step is conditional, save the bring-down, which runs whatever happened."""
    bring_down = f"just {repo.policy['integration']['bring_down']}"
    findings: list[str] = []
    for step in steps_of(job):
        condition = step.get("if")
        run_line = str(step.get("run", "")).strip()
        if condition is None or run_line == bring_down:
            continue
        findings.append(
            f"{where} runs `{run_line or step.get('uses')}` under the condition "
            f"`{condition}`, which skips it for some changes"
        )
    return findings


def _narrowing_findings(repo: Repo, base: str | None, declared: list[Platform]) -> list[str]:
    """The supported-platform list still carries everything it carried at the base."""
    revision = base or _default_base(repo)
    if revision is None:
        return []
    before = run(["git", "show", f"{revision}:AGENTS.md"], cwd=repo.root)
    if before.returncode != 0:
        return []
    try:
        previously = _platform_ids(before.stdout)
    except MarkerBlockMissingError:
        return []
    named = {platform.id for platform in declared}
    return [
        f"AGENTS.md's supported-platform list no longer names `{lost}`, which it named "
        f"at {revision}: the list is the source the integration job's matrix is derived "
        f"from, and narrowing both together is not narrowing neither"
        for lost in previously
        if lost not in named
    ]


def _platform_ids(agents_md: str) -> list[str]:
    """Every platform id one revision of `AGENTS.md` names.

    Raises:
        MarkerBlockMissingError: If that revision carries no such block.
    """
    ids: list[str] = []
    for line in marker_block(agents_md, "supported-platforms"):
        match = PLATFORM_LINE.match(line)
        if match:
            ids.append(match["id"])
    return ids


def _default_base(repo: Repo) -> str | None:
    """The revision this change was cut from, where git can say."""
    branch = repo.policy["repository"]["base_branch"]
    found = run(["git", "merge-base", "HEAD", f"origin/{branch}"], cwd=repo.root)
    return found.stdout.strip() or None if found.returncode == 0 else None
