"""The platform-dispatch workflow: one job, one platform, by hand, and nothing else.

`repo-policy.toml`'s `[dispatch]` names a workflow that takes the name of a
platform-matrixed job of one of two source workflows and one supported
platform as the two inputs of a manual dispatch, and runs exactly that job on
exactly that runner. What turns the two inputs into a job and a runner, and
what then runs the job's commands, is `repo_checks.dispatching` — a committed
script rather than workflow expressions, so a test drives it exactly as the
workflow does. This check holds the workflow to that script, and the script's
sources to what it can run:

* the workflow fires on `workflow_dispatch` and on nothing else, because a
  workflow that ran the gate on a pull request too would be a second gate;
* its inputs are the job input and the platform input, both required choices:
  the platform's options are exactly the supported-platform list, and the
  job's are exactly the platform-matrixed jobs of the source workflows;
* it declares exactly two jobs: one that runs the resolve recipe over the two
  inputs and publishes the runner it answered, and one that needs it, runs on
  that answer, and runs the run recipe over the same two inputs — so nothing
  but the script decides what runs where;
* every source job is one the script can run: its steps are within the shapes
  `repo_checks.dispatching` executes, so a source that grows a construct a
  dispatch by hand cannot run is refused where it is written.
"""

from __future__ import annotations

import re
from typing import Any

from repo_checks.checks_obico import triggers_of
from repo_checks.dispatching import Declared, DispatchError, matrixed_jobs, plan
from repo_checks.model import Repo
from repo_checks.parsing import MarkerBlockMissingError, jobs_of, load_workflow, steps_of
from repo_checks.platforms import supported

#: How the running job reads where to run: the selecting job's published answer.
RUNNER_OUTPUT = re.compile(
    r"^\s*\$\{\{\s*needs\.(?P<job>[A-Za-z_][A-Za-z0-9_-]*)\.outputs\.runner\s*\}\}\s*$"
)


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
            f"workflow cannot dispatch one of its own jobs"
        )
    if not repo.exists(relative):
        return [
            *findings,
            f"the committed configuration declares no platform-dispatch workflow: "
            f"`repo-policy.toml` names {relative}, which is not there",
        ]
    try:
        platforms = [platform.id for platform in supported(repo)]
    except MarkerBlockMissingError as error:
        return [*findings, str(error)]

    workflow = load_workflow(repo.path(relative))
    triggers = triggers_of(workflow)
    findings.extend(
        f"{relative} fires on `{event}`: a workflow that runs the gate and the integration "
        f"tier by hand may fire on a manual dispatch and on nothing else, or it is a "
        f"second gate"
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
        f"{relative} takes a `{extra}` input, and a dispatch names a job and a platform "
        f"and nothing else"
        for extra in sorted(set(inputs) - {declared.job_input, declared.platform_input})
    )
    findings.extend(
        _input_findings(
            relative,
            inputs,
            declared.platform_input,
            platforms,
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
    findings.extend(_job_findings(declared, relative, workflow))
    for source in sources.values():
        try:
            plan(source, "Linux")
        except DispatchError as error:
            findings.append(
                f"{relative} cannot run {source.workflow}'s job `{source.name}` by hand: {error}"
            )
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


def _recipe_steps(job: dict[str, Any], recipe: str) -> list[str]:
    """Every command line of `job` that runs `just <recipe>`."""
    found: list[str] = []
    for step in steps_of(job):
        run = step.get("run")
        if isinstance(run, str):
            found.extend(
                line.strip()
                for line in run.splitlines()
                if line.strip().startswith(f"just {recipe} ")
            )
    return found


def _job_findings(declared: Declared, relative: str, workflow: dict[str, Any]) -> list[str]:
    """Exactly two jobs: one resolving the pair, one running it where that answered."""
    jobs = jobs_of(workflow)
    selecting = [name for name, job in jobs.items() if _recipe_steps(job, declared.resolve_recipe)]
    running = [name for name, job in jobs.items() if _recipe_steps(job, declared.run_recipe)]
    findings: list[str] = []
    if len(selecting) != 1:
        findings.append(
            f"{relative} declares {len(selecting)} jobs running `just {declared.resolve_recipe}`, "
            f"and exactly one job resolves the two inputs to a runner"
        )
    if len(running) != 1:
        findings.append(
            f"{relative} declares {len(running)} jobs running `just {declared.run_recipe}`, "
            f"and exactly one job runs the job the dispatch named"
        )
    findings.extend(
        f"{relative} declares job `{name}`, which neither resolves the dispatch nor runs it: "
        f"a dispatch runs one job and nothing else"
        for name in jobs
        if name not in selecting and name not in running
    )
    if len(selecting) != 1 or len(running) != 1:
        return findings
    select, run = selecting[0], running[0]
    where = f"{relative}: job `{run}`"
    for name, job in ((select, jobs[select]), (run, jobs[run])):
        recipe = declared.resolve_recipe if name == select else declared.run_recipe
        commands = _recipe_steps(job, recipe)
        for command in commands:
            expected = (
                f'just {recipe} "${declared.job_input.upper()}" '
                f'"${declared.platform_input.upper()}"'
            )
            if not command.startswith(expected):
                findings.append(
                    f"{relative}: job `{name}` runs `{command}`, and the script is handed the "
                    f"two inputs as `{expected}`"
                )
        findings.extend(_inputs_handed(relative, name, job, declared))
    outputs = jobs[select].get("outputs")
    if not (isinstance(outputs, dict) and "runner" in outputs):
        findings.append(
            f"{relative}: job `{select}` publishes no `runner` output, which is what the "
            f"running job runs on"
        )
    needs = jobs[run].get("needs")
    needed = needs if isinstance(needs, list) else [needs]
    if select not in needed:
        findings.append(f"{where} does not need job `{select}`, whose answer is where it runs")
    match = RUNNER_OUTPUT.match(str(jobs[run].get("runs-on", "")))
    if match is None or match["job"] != select:
        findings.append(
            f"{where} does not run on `${{{{ needs.{select}.outputs.runner }}}}` (its `runs-on` "
            f"is `{jobs[run].get('runs-on', '')}`), so something other than the script "
            f"decides where a dispatch runs"
        )
    for name in (select, run):
        if "if" in jobs[name] or "strategy" in jobs[name]:
            findings.append(
                f"{relative}: job `{name}` carries a condition or a matrix, and a dispatch "
                f"runs its one job unconditionally on its one runner"
            )
    return findings


def _inputs_handed(relative: str, name: str, job: dict[str, Any], declared: Declared) -> list[str]:
    """The step running the script is handed both inputs, each as its own variable."""
    findings: list[str] = []
    for step in steps_of(job):
        run = step.get("run")
        if not isinstance(run, str) or "just " not in run:
            continue
        environment = step.get("env")
        environment = environment if isinstance(environment, dict) else {}
        for input_name in (declared.job_input, declared.platform_input):
            variable = input_name.upper()
            if str(environment.get(variable, "")).strip() != f"${{{{ inputs.{input_name} }}}}":
                findings.append(
                    f"{relative}: job `{name}`'s step running the script does not hand it "
                    f"`inputs.{input_name}` as `{variable}`"
                )
    return findings
