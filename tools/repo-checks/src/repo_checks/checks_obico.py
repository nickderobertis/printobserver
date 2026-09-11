"""The scheduled Obico tier's workflow, its schedule, and the gate's silence about it.

This tier reconciles what a live self-hosted Obico posts against the sample this
repository commits as its claim about that producer, so it builds Obico's own
images, starts four containers and waits a real failure alert out. That is not
something every gate run can pay for — and a tier left out of the gate with
nothing saying so is a tier nobody runs. So this check holds three sources to
each other:

* the recipe set, which is where the tier and the two recipes bracketing it live;
* the committed workflow, whose triggers must be exactly the schedule it names
  and a manual invocation — no push, no change-request event, nothing that fires
  on a change, because a change-fired copy of this job is the gate again under
  another name;
* `AGENTS.md`, whose recorded schedule must be the one the workflow declares, so
  a schedule stated in prose cannot drift from the one that actually fires.

And it refuses the other direction too: a gate that *did* select this tier — by
declaring it a tier or by invoking it from `check` — is refused, because that is
the cost this whole arrangement exists to keep out of every run.
"""

from __future__ import annotations

from typing import Any

from repo_checks.model import Repo
from repo_checks.parsing import (
    MarkerBlockMissingError,
    fenced_commands,
    jobs_of,
    load_workflow,
    marker_block,
    recipes,
    run_commands,
    section,
    steps_of,
)

# One recorded schedule line, as `AGENTS.md` writes it: `- cron: \`17 4 * * 1\``.
SCHEDULE_PREFIX = "- cron:"
SCHEDULE_SHAPE = "- cron: `<five cron fields>`"


def obico_tier(repo: Repo) -> list[str]:
    """The tier runs on a schedule, on nothing that fires on a change, and not in the gate."""
    policy = repo.policy.get("obico")
    if not policy:
        return ["`repo-policy.toml` declares no `[obico]` section"]

    findings = _recipe_findings(repo, policy)
    findings.extend(_gate_findings(repo, policy))

    relative = f".github/workflows/{policy['workflow']}"
    if not repo.exists(relative):
        return [
            *findings,
            f"the committed configuration declares no scheduled Obico tier workflow: "
            f"`repo-policy.toml` names {relative}, which is not there",
        ]

    workflow = load_workflow(repo.path(relative))
    triggers = triggers_of(workflow)
    findings.extend(_trigger_findings(policy, triggers, relative))
    findings.extend(_job_findings(repo, policy, workflow, relative))
    findings.extend(_schedule_findings(repo, policy, triggers, relative))
    findings.extend(_prose_findings(repo, policy))
    return findings


def _recipe_findings(repo: Repo, policy: dict[str, Any]) -> list[str]:
    """The three recipes `repo-policy.toml` names are recipes the justfile declares."""
    declared = set(recipes(repo.justfile))
    return [
        f"`repo-policy.toml` names `just {policy[key]}` as the scheduled Obico tier's "
        f"{key.replace('_', ' ')}, which the recipe set does not declare"
        for key in ("tier", "bring_up", "bring_down")
        if str(policy[key]) not in declared
    ]


def _gate_findings(repo: Repo, policy: dict[str, Any]) -> list[str]:
    """The ordinary gate selects neither the tier nor the recipes bracketing it."""
    tier = str(policy["tier"])
    bracketing = {str(policy["bring_up"]), str(policy["bring_down"])}
    findings: list[str] = []
    if tier in repo.policy["gate"]["tiers"]:
        findings.append(
            f"`repo-policy.toml`'s gate.tiers names `{tier}`, which is the scheduled "
            f"Obico tier: every gate run would then build Obico's images and wait a "
            f"real failure alert out"
        )
    check = recipes(repo.justfile).get("check")
    if check is None:
        return findings
    invoked = {
        line.split()[1]
        for line in check.body
        if line.split()[:1] == ["just"] and len(line.split()) > 1
    } | set(check.dependencies)
    findings.extend(
        f"the `check` recipe invokes `just {name}`, which belongs to the scheduled "
        f"Obico tier and not to the gate"
        for name in sorted(invoked & ({tier} | bracketing))
    )
    return findings


def triggers_of(workflow: dict[str, Any]) -> dict[str, Any]:
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


def _trigger_findings(policy: dict[str, Any], triggers: dict[str, Any], relative: str) -> list[str]:
    """Exactly the schedule and the manual invocation, and nothing else at all."""
    permitted: list[str] = [str(name) for name in policy["triggers"]]
    findings = [
        f"{relative} fires on `{event}`, which is not one of the triggers the "
        f"scheduled Obico tier may carry ({', '.join(permitted)}): this tier must not "
        f"fire on a change"
        for event in sorted(triggers)
        if event not in permitted
    ]
    findings.extend(
        f"{relative} declares no `{wanted}` trigger, which the scheduled Obico tier must carry"
        for wanted in permitted
        if wanted not in triggers
    )
    findings.extend(_cron_findings(triggers, relative))
    return findings


def crons_of(triggers: dict[str, Any]) -> list[str]:
    """Every cron expression a workflow's schedule declares, in order."""
    schedule = triggers.get("schedule")
    if not isinstance(schedule, list):
        return []
    return [
        str(entry["cron"]).strip()
        for entry in schedule
        if isinstance(entry, dict) and entry.get("cron")
    ]


def _cron_findings(triggers: dict[str, Any], relative: str) -> list[str]:
    """The schedule names at least one cron expression."""
    if "schedule" not in triggers:
        return []
    if not crons_of(triggers):
        return [
            f"{relative} carries a `schedule` trigger that names no cron expression: "
            f"a tier that is out of the gate and on no schedule is a tier nobody runs"
        ]
    return []


def _job_findings(
    repo: Repo, policy: dict[str, Any], workflow: dict[str, Any], relative: str
) -> list[str]:
    """One job runs the bring-up, the tier and the bring-down, in that order."""
    declared = set(recipes(repo.justfile))
    required = [str(policy["bring_up"]), str(policy["tier"]), str(policy["bring_down"])]
    for job_name, job in jobs_of(workflow).items():
        commands = run_commands(job)
        if f"just {policy['tier']}" not in commands:
            continue
        where = f"{relative}: the job `{job_name}`"
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
                f"{where} runs {', '.join(f'`just {r}`' for r in required)} out of "
                f"order: the tier runs between the bring-up and the bring-down"
            )
        findings.extend(_teardown_findings(repo, policy, job, where))
        return findings
    return [
        f"{relative} declares no job that runs `just {policy['tier']}`: the scheduled "
        f"workflow does not run the tier it exists for"
    ]


def _teardown_findings(
    repo: Repo, policy: dict[str, Any], job: dict[str, Any], where: str
) -> list[str]:
    """The bring-down runs whatever happened, and no other step is conditional."""
    del repo
    bring_down = f"just {policy['bring_down']}"
    findings: list[str] = []
    for step in steps_of(job):
        run_line = str(step.get("run", "")).strip()
        condition = step.get("if")
        if run_line == bring_down:
            if condition is None:
                findings.append(
                    f"{where} runs `{bring_down}` unconditionally rather than under "
                    f"`if: always()`: a tier that failed would leave its containers up"
                )
            continue
        if condition is not None:
            findings.append(
                f"{where} runs `{run_line or step.get('uses')}` under the condition "
                f"`{condition}`, which skips it on some runs"
            )
    return findings


def _schedule_findings(
    repo: Repo, policy: dict[str, Any], triggers: dict[str, Any], relative: str
) -> list[str]:
    """`AGENTS.md` records the schedule the committed workflow actually declares."""
    try:
        lines = marker_block(repo.agents_md, str(policy["schedule_block"]))
    except MarkerBlockMissingError as error:
        return [str(error)]

    recorded: list[str] = []
    findings: list[str] = []
    for line in lines:
        if not line.startswith("- "):
            continue
        if not line.startswith(SCHEDULE_PREFIX):
            findings.append(
                f"AGENTS.md records the schedule line `{line}`, which is not of the "
                f"form `{SCHEDULE_SHAPE}`"
            )
            continue
        recorded.append(line[len(SCHEDULE_PREFIX) :].strip().strip("`").strip())

    declared = crons_of(triggers)
    if not recorded:
        findings.append(
            f"AGENTS.md's `{policy['schedule_block']}` block records no schedule, and "
            f"{relative} declares {', '.join(f'`{c}`' for c in declared) or 'none'}"
        )
    findings.extend(
        f"AGENTS.md records the schedule `{cron}`, which {relative} does not declare"
        for cron in recorded
        if cron not in declared
    )
    findings.extend(
        f"{relative} declares the schedule `{cron}`, which AGENTS.md's "
        f"`{policy['schedule_block']}` block does not record"
        for cron in declared
        if cron not in recorded
    )
    return findings


def _prose_findings(repo: Repo, policy: dict[str, Any]) -> list[str]:
    """`AGENTS.md` records the tier, and states every recipe a reader runs by hand.

    Stated as a *pasteable command* rather than as a mention: a section that
    explains what `just obico-up` does somewhere in a paragraph has not told a
    developer how to run the tier, and a reader who has to reconstruct the three
    recipes from prose is a reader who runs two of them.
    """
    heading = str(policy["section"])
    body = section(repo.agents_md, heading)
    if not body.strip():
        return [f"AGENTS.md carries no `## {heading}` section recording this tier"]
    stated = fenced_commands(body)
    return [
        f"AGENTS.md's `## {heading}` section states no `just {policy[key]}` command, "
        f"which is how a reader runs this tier by hand"
        for key in ("bring_up", "tier", "bring_down")
        if f"just {policy[key]}" not in stated
    ]
