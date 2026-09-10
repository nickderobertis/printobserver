"""The registry install-path proof: its recipes, its triggers, and the gate's silence.

The three `artifact-route-*` jobs prove an artifact **built from the committed
tree**, and they are the only proof a change can run before anything is
published. They are also green over a repository nothing can install, which is
the state this one was in for two days: the one workflow that did read the real
registries only installed, never ran what it installed, and had failed every
cell of its only run with nobody looking.

So this check holds four sources to each other:

* the recipe set, which is where the tier and the one recipe per route live;
* `release-targets.toml` beside `AGENTS.md`'s install-path section, so every
  route that section states has a recipe proving the target that backs it;
* the committed workflow, whose triggers must be a release's own proof, a
  schedule and a manual invocation — and nothing that fires on a change, which
  over this tier could only ever report what was published before that change;
* `AGENTS.md`, whose recorded schedule must be the one the workflow declares.

Two things about that release-time trigger are the whole reason it is checked
rather than left to a reader. It is the release *workflow having finished*: the
GitHub Release is cut in that workflow's `release` job and the artifacts are
built and published in the two jobs after it, so a proof keyed on the release
being published measures the version before it. And WHICH release it proves is
the one that run itself cut, resolved once by the job `[install_proof]` names
and handed to every job that proves a route — never the newest the forge lists,
which on a repository that finishes a release run on every push is somebody
else's release as often as not.

That gate is deliberately not the triggering run's *conclusion*. A run that cut
its release and then failed to publish the artifacts is exactly the state this
tier exists to find: gated on the conclusion it is skipped and the publish that
did not happen is reported by nothing at all, so this refuses a job that gates
on one.

And it refuses the other direction too: a gate that selected this tier — by
declaring it a tier or by invoking it from `check` — is refused.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from repo_checks import install_path as ip
from repo_checks.checks_ci import proving
from repo_checks.checks_obico import SCHEDULE_PREFIX, SCHEDULE_SHAPE, crons_of, triggers_of
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
)

#: The triggering run's own conclusion, which no job of this proof may gate on.
#: A release-time run is gated on the release it cut instead, so that a run
#: whose release exists and whose publish failed is proven rather than skipped.
CONCLUSION = "github.event.workflow_run.conclusion"


@dataclass(frozen=True, slots=True)
class Declared:
    """What `[install_proof]` says, narrowed once rather than read as it goes.

    `repo-policy.toml` is whatever the TOML reader handed back, so every value
    starts out as `Any`. This is where that stops for this check: `read` below
    is the only place a key of that table is looked up, and a declaration
    missing one of them is one finding naming the key rather than an attribute
    error out of whichever rule happened to read it first.

    The committed workflow stays the mapping the YAML reader answered with,
    because that is the shape every check in this repository reads a workflow
    through — `jobs_of`, `run_commands` and `triggers_of` are that boundary,
    and a model of a GitHub workflow held here would be a second one.
    """

    #: The recipe a person runs to prove every route by hand.
    tier: str
    #: What the recipes proving one route each are named by.
    recipe_prefix: str
    #: The workflow that runs them, under `.github/workflows`.
    workflow: str
    #: The workflow whose FINISHING is the release-time trigger.
    release_workflow: str
    #: The job that resolves which release a release-time run proves.
    release_job: str
    #: The output it publishes that version under.
    release_output: str
    #: The recipe it answers it with.
    release_recipe: str
    #: How the version under test reaches the tier.
    version_env: str
    #: The manual invocation's input a caller names a version in.
    version_input: str
    #: The word meaning "the newest release the forge published".
    release_selector: str
    #: The variable pointing every registry somewhere other than the real ones.
    standin_env: str
    #: The module that reads both of those variables.
    version_source: str
    #: The `AGENTS.md` block recording the schedule it runs on.
    schedule_block: str
    #: The `AGENTS.md` section recording the tier.
    section: str
    #: The only events that workflow may fire on.
    triggers: tuple[str, ...]

    @classmethod
    def read(cls, table: dict[str, Any]) -> Declared | str:
        """The declaration, or the one finding saying what it is missing."""
        named = [field for field in cls.__dataclass_fields__ if field != "triggers"]
        found = {key: str(table.get(key, "")).strip() for key in named}
        missing = sorted(key for key, value in found.items() if not value)
        declared = table.get("triggers")
        events: tuple[str, ...] = ()
        if isinstance(declared, list) and all(
            isinstance(event, str) and event.strip() for event in declared
        ):
            events = tuple(str(event).strip() for event in declared)
        if not events:
            missing.append("triggers")
        if missing:
            return (
                f"`repo-policy.toml`'s `[install_proof]` declares no "
                f"{', '.join(sorted(missing))}, so nothing can say what proves the three "
                f"routes against their registries"
            )
        return cls(**found, triggers=events)


def install_proof(repo: Repo) -> list[str]:
    """The tier proves every route, on a release and a schedule, and not in the gate."""
    table = repo.policy.get("install_proof")
    if not isinstance(table, dict) or not table:
        return ["`repo-policy.toml` declares no `[install_proof]` section"]
    declared = Declared.read(table)
    if isinstance(declared, str):
        return [declared]

    findings = _recipe_findings(repo, declared)
    findings.extend(_gate_findings(repo, declared))

    relative = f".github/workflows/{declared.workflow}"
    if not repo.exists(relative):
        return [
            *findings,
            f"the committed configuration declares no registry install-path proof "
            f"workflow: `repo-policy.toml` names {relative}, which is not there",
        ]

    workflow = load_workflow(repo.path(relative))
    triggers = triggers_of(workflow)
    findings.extend(_trigger_findings(repo, declared, triggers, relative))
    findings.extend(_version_findings(repo, declared, workflow, relative))
    findings.extend(_consumer_findings(repo, declared))
    findings.extend(_job_findings(repo, declared, workflow, relative))
    findings.extend(_schedule_findings(repo, declared, triggers, relative))
    findings.extend(_prose_findings(repo, declared))
    return findings


def _proof_recipes(repo: Repo, policy: Declared) -> dict[str, str]:
    """Which recipe proves each route's own target against its registry.

    Read out of each recipe's own body — the target it names — so a recipe
    pointed at another artifact is one this stops finding for the route it used
    to prove.
    """
    prefix = policy.recipe_prefix
    found: dict[str, str] = {}
    for identifier, names in proving(repo).items():
        for name in names:
            if name.startswith(prefix):
                found[identifier] = name
    return found


def _routed(repo: Repo) -> dict[str, str]:
    """The target behind each route the install-path section states."""
    return {
        str(target.get("route", "")).strip(): str(target.get("id", ""))
        for target in repo.read_toml("release-targets.toml").get("target", [])
        if str(target.get("route", "")).strip()
    }


def _recipe_findings(repo: Repo, policy: Declared) -> list[str]:
    """One recipe per route, and a tier that runs every one of them."""
    declared = recipes(repo.justfile)
    tier = policy.tier
    findings: list[str] = []
    if tier not in declared:
        findings.append(
            f"`repo-policy.toml` names `just {tier}` as the registry install-path "
            f"proof's tier, which the recipe set does not declare"
        )

    proofs = _proof_recipes(repo, policy)
    routed = _routed(repo)
    for route in ip.parse(repo.agents_md).routes:
        identifier = routed.get(route.heading, "")
        if not identifier:
            findings.append(
                f"AGENTS.md's `{ip.SECTION_HEADING}` states route `{route.heading}`, and "
                f"release-targets.toml declares no target behind it, so nothing can say "
                f"which registry that route is taken from"
            )
            continue
        if identifier not in proofs:
            findings.append(
                f"route `{route.heading}` is taken from the registry serving "
                f"`{identifier}`, and no `{policy.recipe_prefix}` recipe proves what "
                f"that registry serves"
            )

    if tier in declared:
        invoked = {
            line.split()[1]
            for line in declared[tier].body
            if line.split()[:1] == ["just"] and len(line.split()) > 1
        }
        findings.extend(
            f"the `{tier}` recipe does not invoke `just {recipe}`, so a run of this tier "
            f"by hand leaves one of the three routes unproven"
            for recipe in sorted(set(proofs.values()))
            if recipe not in invoked
        )
    return findings


def _gate_findings(repo: Repo, policy: Declared) -> list[str]:
    """The ordinary gate selects neither the tier nor the recipes under it."""
    tier = policy.tier
    belongs = {tier, *_proof_recipes(repo, policy).values()}
    findings: list[str] = []
    if tier in repo.policy["gate"]["tiers"]:
        findings.append(
            f"`repo-policy.toml`'s gate.tiers names `{tier}`, which is the registry "
            f"install-path proof: over a change it could only ever report what was "
            f"published before that change"
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
        f"the `check` recipe invokes `just {name}`, which belongs to the registry "
        f"install-path proof and not to the gate"
        for name in sorted(invoked & belongs)
    )
    return findings


def _trigger_findings(
    repo: Repo, policy: Declared, triggers: dict[str, Any], relative: str
) -> list[str]:
    """A release's own proof, a schedule, a manual invocation, and nothing else."""
    permitted = list(policy.triggers)
    findings = [
        f"{relative} fires on `{event}`, which is not one of the triggers the registry "
        f"install-path proof may carry ({', '.join(permitted)}): this tier reads the "
        f"real registries, so over a change it could only report what was already there"
        for event in sorted(triggers)
        if event not in permitted
    ]
    findings.extend(
        f"{relative} declares no `{wanted}` trigger, which the registry install-path "
        f"proof must carry"
        for wanted in permitted
        if wanted not in triggers
    )
    if "schedule" in triggers and not crons_of(triggers):
        findings.append(
            f"{relative} carries a `schedule` trigger that names no cron expression: a "
            f"tier that is out of the gate and on no schedule is a tier nobody runs"
        )
    findings.extend(_release_trigger_findings(repo, policy, triggers, relative))
    findings.extend(_dispatch_findings(policy, triggers, relative))
    return findings


def _release_trigger_findings(
    repo: Repo, policy: Declared, triggers: dict[str, Any], relative: str
) -> list[str]:
    """The release-time trigger is the release workflow having finished."""
    wanted = policy.release_workflow
    named = [
        str(name)
        for name in (triggers.get("workflow_run") or {}).get("workflows", [])
        if isinstance(name, str)
    ]
    findings: list[str] = []
    if wanted not in named:
        findings.append(
            f"{relative} keys a release's own proof on {named or 'no workflow'} rather "
            f"than on `{wanted}` having finished: the release is cut before its artifacts "
            f"are built and published, so a proof keyed anywhere earlier measures the "
            f"version before it"
        )
    committed = {str(load_workflow(path).get("name", "")).strip() for path in repo.workflow_paths}
    if wanted not in committed:
        findings.append(
            f"`repo-policy.toml` names `{wanted}` as the workflow whose finishing is this "
            f"proof's release-time trigger, and no committed workflow carries that name"
        )
    return findings


def _dispatch_findings(policy: Declared, triggers: dict[str, Any], relative: str) -> list[str]:
    """A caller can name the version a manual run proves."""
    inputs = (triggers.get("workflow_dispatch") or {}).get("inputs") or {}
    if str(policy.version_input) in inputs:
        return []
    return [
        f"{relative} declares no `{policy.version_input}` input on its manual "
        f"invocation, so a caller cannot name the version a run proves"
    ]


def _proving_jobs(repo: Repo, policy: Declared, jobs: dict[str, dict[str, Any]]) -> set[str]:
    """Every job that runs one of the recipes proving a route against its registry."""
    wanted = set(_proof_recipes(repo, policy).values())
    return {
        name
        for name, job in jobs.items()
        if any(f"just {recipe}" in run_commands(job) for recipe in wanted)
    }


def _version_findings(
    repo: Repo, policy: Declared, workflow: dict[str, Any], relative: str
) -> list[str]:
    """The version under test is the release the run cut, and one answer for the run.

    One answer, and it is a JOB's rather than the workflow's, because the
    release it names is resolved by a job — nothing outside one can read that.
    So this asks for what a workflow-level declaration used to give: every job
    proving a route says which version it proves, they all say the same thing,
    and no other job says anything.
    """
    variable = policy.version_env
    jobs = jobs_of(workflow)
    proving_jobs = _proving_jobs(repo, policy, jobs)
    resolved = f"needs.{policy.release_job}.outputs.{policy.release_output}"
    findings: list[str] = []
    if variable in (workflow.get("env") or {}):
        findings.append(
            f"{relative} declares `{variable}` for the whole workflow, and the version "
            f"under test is the release the `{policy.release_job}` job resolved — which "
            f"nothing outside a job can read"
        )
    stated: set[str] = set()
    for name in sorted(proving_jobs):
        declared = jobs[name].get("env") or {}
        if variable not in declared:
            findings.append(
                f"{relative}: job `{name}` proves a route and declares no `{variable}`, "
                f"so nothing says which version it proves"
            )
            continue
        expression = " ".join(str(declared[variable]).split())
        stated.add(expression)
        if f"inputs.{policy.version_input}" not in expression:
            findings.append(
                f"{relative}: job `{name}`'s `{variable}` is `{expression}`, which does "
                f"not take the version a caller named (`inputs.{policy.version_input}`)"
            )
        if resolved not in expression:
            findings.append(
                f"{relative}: job `{name}`'s `{variable}` is `{expression}`, which is not "
                f"the release the triggering run cut (`{resolved}`): a release's own "
                f"proof would then prove whatever a registry happened to serve newest"
            )
    if len(stated) > 1:
        findings.append(
            f"{relative}: the jobs proving a route take "
            f"{', '.join(f'`{one}`' for one in sorted(stated))} as `{variable}`, and the "
            f"version under test is one answer for the whole run"
        )
    findings.extend(
        f"{relative}: job `{name}` declares `{variable}` and proves no route with it"
        for name, job in sorted(jobs.items())
        if name not in proving_jobs and variable in (job.get("env") or {})
    )
    return findings


def _consumer_findings(repo: Repo, policy: Declared) -> list[str]:
    """The module reading the two variables declares the names this file does.

    The rest of this check reconciles the policy, the workflow and the prose,
    and all three could agree while the code reading them named something else
    — which is a tier that runs, proves the wrong version, and says so nowhere.
    """
    if not repo.exists(policy.version_source):
        return [
            f"`repo-policy.toml` names {policy.version_source} as what reads the version "
            f"under test, and this repository commits no such file"
        ]
    source = repo.read(policy.version_source)
    # The quoted literal rather than the name anywhere in the file: what
    # reaches an environment lookup is the string, and a module that renamed it
    # while still discussing the old name in a docstring is exactly the drift
    # this closes.
    return [
        f"{policy.version_source} declares no `{variable}`, which is the name "
        f"`repo-policy.toml` and the committed workflow use for it"
        for variable in (policy.version_env, policy.standin_env)
        if f'"{variable}"' not in source
    ]


def _job_findings(
    repo: Repo, policy: Declared, workflow: dict[str, Any], relative: str
) -> list[str]:
    """Every proof recipe has a job, and every job proves the release its run cut."""
    declared = set(recipes(repo.justfile))
    jobs = jobs_of(workflow)
    findings: list[str] = []
    for recipe in sorted(set(_proof_recipes(repo, policy).values())):
        running = [name for name, job in jobs.items() if f"just {recipe}" in run_commands(job)]
        if not running:
            findings.append(
                f"{relative} declares no job that runs `just {recipe}`, so nothing proves "
                f"that route against its own registry"
            )
        if recipe not in declared:
            findings.append(f"`just {recipe}` is not a recipe the recipe set declares")

    resolving = jobs.get(policy.release_job)
    if resolving is None:
        findings.append(
            f"{relative} declares no `{policy.release_job}` job, and nothing else can say "
            f"which release the triggering run cut"
        )
    elif f"just {policy.release_recipe}" not in " ".join(run_commands(resolving)):
        findings.append(
            f"{relative}: job `{policy.release_job}` does not run `just "
            f"{policy.release_recipe}`, which is what reads the release a run cut off the "
            f"tag it left"
        )
    if policy.release_recipe not in declared:
        findings.append(f"`just {policy.release_recipe}` is not a recipe the recipe set declares")

    gate = f"needs.{policy.release_job}.outputs.{policy.release_output}"
    for job_name, job in sorted(jobs.items()):
        condition = " ".join(str(job.get("if", "")).split())
        if job_name != policy.release_job and gate not in condition:
            findings.append(
                f"{relative}: job `{job_name}` is not gated on `{gate}`, so a run that "
                f"cut no release at all would prove whatever a registry served newest"
            )
        # A conclusion gate is what this replaced, and it is refused rather
        # than merely not asked for: a run that cut its release and then failed
        # to publish it is the one state this tier exists to find, and gating
        # on the conclusion skips exactly that run.
        if CONCLUSION in condition:
            findings.append(
                f"{relative}: job `{job_name}` is gated on `{CONCLUSION}`, so the run "
                f"that cut a release and failed to publish it — the one state this tier "
                f"exists to find — is skipped and reported by nothing"
            )
    return findings


def _schedule_findings(
    repo: Repo, policy: Declared, triggers: dict[str, Any], relative: str
) -> list[str]:
    """`AGENTS.md` records the schedule the committed workflow actually declares."""
    try:
        lines = marker_block(repo.agents_md, str(policy.schedule_block))
    except MarkerBlockMissingError as error:
        return [str(error)]

    recorded: list[str] = []
    findings: list[str] = []
    for line in lines:
        if not line.startswith("- "):
            continue
        if not line.startswith(SCHEDULE_PREFIX):
            findings.append(
                f"AGENTS.md records the schedule line `{line}`, which is not of the form "
                f"`{SCHEDULE_SHAPE}`"
            )
            continue
        recorded.append(line[len(SCHEDULE_PREFIX) :].strip().strip("`").strip())

    declared = crons_of(triggers)
    if not recorded:
        findings.append(
            f"AGENTS.md's `{policy.schedule_block}` block records no schedule, and "
            f"{relative} declares {', '.join(f'`{cron}`' for cron in declared) or 'none'}"
        )
    findings.extend(
        f"AGENTS.md records the schedule `{cron}`, which {relative} does not declare"
        for cron in recorded
        if cron not in declared
    )
    findings.extend(
        f"{relative} declares the schedule `{cron}`, which AGENTS.md's "
        f"`{policy.schedule_block}` block does not record"
        for cron in declared
        if cron not in recorded
    )
    return findings


def _prose_findings(repo: Repo, policy: Declared) -> list[str]:
    """`AGENTS.md` records the tier, and states the command a reader runs by hand.

    As a *pasteable command* rather than as a mention: a section explaining what
    the tier does has not told a developer how to run it.
    """
    heading = policy.section
    body = section(repo.agents_md, heading)
    if not body.strip():
        return [f"AGENTS.md carries no `## {heading}` section recording this tier"]
    stated = " ".join(fenced_commands(body))
    findings: list[str] = []
    if f"just {policy.tier}" not in stated:
        findings.append(
            f"AGENTS.md's `## {heading}` section states no `just {policy.tier}` "
            f"command, which is how a reader runs this tier by hand"
        )
    if policy.version_env not in body:
        findings.append(
            f"AGENTS.md's `## {heading}` section does not say how the version under test "
            f"is named (`{policy.version_env}`)"
        )
    return findings
