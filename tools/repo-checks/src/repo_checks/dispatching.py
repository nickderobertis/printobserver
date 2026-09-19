"""One job of one source workflow on one supported platform, by hand.

`.github/workflows/platform-dispatch.yml` takes the name of a job of `ci.yml`
or of `install-path.yml` and one identifier from `AGENTS.md`'s supported-
platform list as the two inputs of a manual dispatch, and runs exactly that
job's commands on exactly that platform's runner. What turns the two inputs
into the one job to run and the runner it runs on, and what then runs that
job's commands, is this module rather than workflow expressions — so a test
can drive it exactly as the workflow does, on a host that is not a runner.

Two verbs, one resolution:

* `resolve` answers the runner label `AGENTS.md`'s list declares for the
  platform, and which source workflow declares the job, as `name=value` lines
  the workflow's own `select` job appends to `$GITHUB_OUTPUT`;
* `run` executes the source job's `run:` steps, in order, on the host it is
  invoked on — every command a step runs, the step's own condition, and
  whether its failure is waived, read off the committed source workflow.

A pair that names no platform-matrixed job of the sources, or no platform of
the list, is refused before anything runs, by both verbs.

# What `run` reads off a source job, and what it refuses

A source job's steps are written for Actions, and Actions is what runs them on
a pull request. This executor runs the same steps by hand, so it admits exactly
the shapes those steps take and refuses a step outside them naming the shape —
which `just check-repo`'s `platform-dispatch` reads off every source job too, so
a source that grows a construct this cannot run is refused where it is written
rather than found by a dispatch that ran something else.

* A `uses:` step is an action, which only Actions can run: the workflow's `run`
  job sets up every tool those actions set up, once, for whichever job it runs,
  and this executor passes them over.
* A `run:` step runs under `bash --noprofile --norc -eo pipefail`, which is the
  shell Actions gives a step that names none — on Windows too, where the
  workflow's `run` job names `bash` for every step.
* `if: always()` runs the step whether or not one before it failed, which is
  how a teardown runs; `if: runner.os == '<Linux|macOS|Windows>'` runs it on
  that family's runner alone; a step naming no condition runs unless one
  before it failed and was not waived. Any other condition is refused.
* `continue-on-error: true` waives the step's failure: its outcome is
  `failure`, and the job's exit is not.
* A step's `env:` carries a literal, `${{ steps.<id>.outcome }}`, or
  `${{ runner.os == '<family>' && steps.<a>.outcome || steps.<b>.outcome }}`,
  which are the shapes the install-route jobs report their waived steps with.
  Any other expression is refused.
* A job-level `env:` value that is a literal is carried; one that is an
  expression is not, because it reads a run this dispatch is not — the
  registry proofs carry a version that way, and carried none prove the newest
  their registry serves.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile
from collections.abc import Callable
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from typing import Any

from repo_checks.model import Repo, policy_table
from repo_checks.parsing import MarkerBlockMissingError, jobs_of, load_workflow, steps_of
from repo_checks.platforms import Platform, ServiceManager, supported
from repo_checks.shell import run as shell_run

#: The exit a refused pair or an unrunnable source job answers with, apart from
#: the exit of a job that ran and failed, which is that job's own and is not
#: rewritten — so a job that itself exits 2 is told from a refusal by the
#: `platform-dispatch: refused:` line a refusal prints and a job does not.
# llmlint: ignore[cli_output_contract] suppressions.toml has the reason.
REFUSED = 2

#: The name `runner.os` answers for each family, keyed by the service manager
#: `AGENTS.md`'s list states for a platform — one per family, which is what
#: makes it the family's name here.
RUNNER_OS = {
    ServiceManager.SYSTEMD: "Linux",
    ServiceManager.LAUNCHD: "macOS",
    ServiceManager.WINDOWS_SERVICE: "Windows",
}

#: What a `run:` step may carry: a `uses:` step carries its action and what it
#: is given, and nothing this executor reads.
RUN_STEP_KEYS = frozenset({"id", "if", "run", "env", "continue-on-error", "name"})
USES_STEP_KEYS = frozenset({"id", "if", "uses", "with", "name"})

#: A step condition on the runner's family, and the one on a step that always runs.
ALWAYS = "always()"
OS_CONDITION = re.compile(r"^runner\.os\s*==\s*'(?P<family>Linux|macOS|Windows)'$")

#: The two shapes a step's `env:` may read another step's outcome through.
OUTCOME = re.compile(r"^\$\{\{\s*steps\.(?P<step>[A-Za-z0-9_-]+)\.outcome\s*\}\}$")
OS_CHOICE = re.compile(
    r"^\$\{\{\s*runner\.os\s*==\s*'(?P<family>Linux|macOS|Windows)'\s*&&\s*"
    r"steps\.(?P<then>[A-Za-z0-9_-]+)\.outcome\s*\|\|\s*"
    r"steps\.(?P<otherwise>[A-Za-z0-9_-]+)\.outcome\s*\}\}$"
)
EXPRESSION = re.compile(r"\$\{\{.*\}\}", re.DOTALL)


class DispatchError(ValueError):
    """A pair that names nothing, or a source job this executor cannot run."""


class Outcome(StrEnum):
    """What one step came to, in the words Actions uses for it."""

    SUCCESS = "success"
    FAILURE = "failure"
    SKIPPED = "skipped"


@dataclass(frozen=True, slots=True)
class Declared:
    """What `repo-policy.toml`'s `[dispatch]` says, narrowed once."""

    workflow: str
    job_input: str
    platform_input: str
    sources: tuple[str, ...]
    resolve_recipe: str
    run_recipe: str

    @classmethod
    def read(cls, repo: Repo) -> Declared | str:
        """The declaration, or the one finding saying what it is missing."""
        table = policy_table(repo, "dispatch")
        if not table:
            return "`repo-policy.toml` declares no `[dispatch]` section"
        keys = ("workflow", "job_input", "platform_input", "resolve_recipe", "run_recipe")
        found = {key: table[key].strip() if isinstance(table.get(key), str) else "" for key in keys}
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
        return cls(
            found["workflow"],
            found["job_input"],
            found["platform_input"],
            sources,
            found["resolve_recipe"],
            found["run_recipe"],
        )


@dataclass(frozen=True, slots=True)
class Source:
    """One platform-matrixed job of one source workflow."""

    workflow: str
    name: str
    job: dict[str, Any]


def matrixed_jobs(repo: Repo, sources: tuple[str, ...]) -> dict[str, Source]:
    """Every platform-matrixed job of the source workflows, by name.

    `Any` at the deserialization boundary: a job is whatever the YAML reader
    handed back, and what this reads out of one is narrowed where it is read.

    Raises:
        DispatchError: If two source workflows both carry a platform-matrixed
            job of one name. A dispatch names a job by that name alone, so the
            pair would run whichever source is listed first and say nothing
            about the other — refused here rather than resolved by order.
    """
    from repo_checks.checks_ci import _matrix_platforms

    found: dict[str, Source] = {}
    for source in sources:
        path = repo.path(f".github/workflows/{source}")
        if not path.is_file():
            continue
        for name, job in jobs_of(load_workflow(path)).items():
            if _matrix_platforms(job) is None:
                continue
            if name in found:
                msg = (
                    f"`{name}` is a platform-matrixed job of both {found[name].workflow} and "
                    f"{source}, so a dispatch naming it would run whichever the sources list "
                    f"first; a job a dispatch can name is a job of exactly one source workflow"
                )
                raise DispatchError(msg)
            found[name] = Source(source, name, job)
    return found


@dataclass(frozen=True, slots=True)
class Resolved:
    """What one (job, platform) pair names."""

    source: Source
    platform: Platform

    @property
    def runner_os(self) -> str:
        """What `runner.os` answers on this platform's runner."""
        return RUNNER_OS[ServiceManager(self.platform.service_manager)]


def resolve(repo: Repo, job: str, platform: str) -> Resolved:
    """The source job and the platform a pair names, or why it names none.

    Raises:
        DispatchError: If the job is no platform-matrixed job of the source
            workflows, or the platform is none the supported-platform list
            names — either of which is refused before anything runs.
    """
    declared = Declared.read(repo)
    if isinstance(declared, str):
        raise DispatchError(declared)
    jobs = matrixed_jobs(repo, declared.sources)
    try:
        platforms = {one.id: one for one in supported(repo)}
    except MarkerBlockMissingError as error:
        raise DispatchError(str(error)) from error
    refused = []
    if job not in jobs:
        refused.append(
            f"`{job}` is no platform-matrixed job of {', '.join(declared.sources)}; "
            f"the jobs a dispatch can name are {', '.join(sorted(jobs))}"
        )
    if platform not in platforms:
        refused.append(
            f"`{platform}` is no platform AGENTS.md's supported-platform list names; "
            f"the platforms a dispatch can name are {', '.join(platforms)}"
        )
    if refused:
        raise DispatchError("; ".join(refused))
    return Resolved(jobs[job], platforms[platform])


@dataclass(frozen=True, slots=True)
class Step:
    """One `run:` step of a source job, as this executor runs it."""

    #: Its `id`, or its position where it has none.
    id: str
    command: str
    #: `always()`, a runner family, or `""` for a step naming no condition.
    condition: str
    waived: bool
    environment: dict[str, str] = field(default_factory=dict)


def plan(source: Source, runner_os: str) -> list[Step]:
    """The steps `run` executes for one source job, in order.

    Raises:
        DispatchError: If the job's own `env` or a step carries a shape this
            executor cannot run, naming the job or the step and the shape.
    """
    job_environment(source)
    planned: list[Step] = []
    for position, step in enumerate(steps_of(source.job), start=1):
        where = f"{source.workflow}'s job `{source.name}`, step {position}"
        keys = set(step)
        if "uses" in step:
            _refuse_extra(where, keys - USES_STEP_KEYS)
            continue
        if not isinstance(step.get("run"), str):
            raise DispatchError(f"{where} neither runs a command nor uses an action")
        _refuse_extra(where, keys - RUN_STEP_KEYS)
        condition = _condition(where, step.get("if"))
        waived = step.get("continue-on-error", False)
        if waived is not True and waived is not False:
            raise DispatchError(f"{where} sets `continue-on-error` to something other than a bool")
        environment = _step_environment(where, step.get("env"), runner_os)
        identifier = step.get("id")
        planned.append(
            Step(
                identifier if isinstance(identifier, str) and identifier else f"step-{position}",
                step["run"],
                condition,
                waived,
                environment,
            )
        )
    return planned


def _refuse_extra(where: str, extra: set[str]) -> None:
    if extra:
        listed = ", ".join(f"`{key}`" for key in sorted(extra))
        raise DispatchError(f"{where} carries {listed}, which a dispatch by hand cannot run")


def _condition(where: str, declared: object) -> str:
    """A step's condition, as one of the three shapes this executor runs."""
    if declared is None:
        return ""
    if not isinstance(declared, str):
        raise DispatchError(f"{where} carries an `if` that is not a string")
    text = declared.strip()
    if text == ALWAYS:
        return ALWAYS
    match = OS_CONDITION.match(text)
    if match is None:
        raise DispatchError(
            f"{where} is conditioned on `{text}`, and a dispatch by hand runs a step on "
            f"`{ALWAYS}`, on `runner.os == '<family>'`, or unconditionally"
        )
    return match["family"]


def _step_environment(where: str, declared: object, runner_os: str) -> dict[str, str]:
    """A step's own `env:`, with each value as the executor will read it.

    An outcome expression is kept as it is written and read when the step runs,
    because the outcome it names is not known until then; a literal is kept as
    a literal. Anything else is refused.
    """
    environment: dict[str, str] = {}
    for name, text in _scalar_environment(where, declared).items():
        if EXPRESSION.search(text) and not (
            OUTCOME.match(text.strip()) or OS_CHOICE.match(text.strip())
        ):
            raise DispatchError(
                f"{where} sets `{name}` to `{text}`, and a dispatch by hand reads a step's "
                f"environment as a literal, as `steps.<id>.outcome`, or as a `runner.os` "
                f"choice between two outcomes"
            )
        environment[name] = text
    return environment


def _scalar_environment(where: str, declared: object) -> dict[str, str]:
    """An `env:` mapping as written, each value a scalar rendered as text.

    Raises:
        DispatchError: If it is not a mapping, or a value is not a scalar —
            a list or a mapping is nothing a process environment can carry,
            and rendering one as text would hand a step a value nobody wrote.
    """
    if declared is None:
        return {}
    if not isinstance(declared, dict):
        raise DispatchError(f"{where} carries an `env` that is not a mapping")
    environment: dict[str, str] = {}
    for name, value in declared.items():
        if not isinstance(value, str | int | float | bool):
            raise DispatchError(f"{where} sets `{name}` to something other than a scalar")
        environment[str(name)] = str(value)
    return environment


def job_environment(source: Source) -> dict[str, str]:
    """The job-level `env:` values a dispatch carries: the literal ones.

    An expression at job level is left out rather than refused, because the
    one the committed workflows carry names the version a release published,
    and a dispatch by hand proves the newest instead. What the job declares is
    held to the same shape as a step's: a mapping of scalars, or a refusal
    naming the job.

    Raises:
        DispatchError: If the job's `env` is not a mapping of scalars.
    """
    where = f"{source.workflow}'s job `{source.name}`"
    declared = _scalar_environment(where, source.job.get("env"))
    return {name: text for name, text in declared.items() if not EXPRESSION.search(text)}


def _value(text: str, outcomes: dict[str, Outcome], runner_os: str) -> str:
    """One step environment value, with its outcome expression read."""
    stripped = text.strip()
    if match := OUTCOME.match(stripped):
        return str(outcomes.get(match["step"], Outcome.SKIPPED))
    if match := OS_CHOICE.match(stripped):
        chosen = match["then"] if match["family"] == runner_os else match["otherwise"]
        return str(outcomes.get(chosen, Outcome.SKIPPED))
    return text


def execute(
    resolved: Resolved,
    *,
    cwd: Path,
    environment: dict[str, str] | None = None,
    say: Callable[[str], object] = print,
) -> int:
    """Run the source job's steps on this host, and answer the job's exit.

    Every step's command runs under bash, with the job's literal environment and
    its own, and what it prints reaches the caller's streams as it happens. A
    step that fails and is not waived fails the job and stops every step after
    it but those on `always()`; the exit is that first failure's.

    Raises:
        DispatchError: If the source job carries a shape this cannot run.
    """
    steps = plan(resolved.source, resolved.runner_os)
    inherited = dict(os.environ if environment is None else environment)
    inherited.update(job_environment(resolved.source))
    outcomes: dict[str, Outcome] = {}
    failed = 0
    for step in steps:
        # What Actions does: a step naming no condition, or a family's, runs
        # only while nothing before it has failed; `always()` runs regardless.
        here = step.condition in {"", ALWAYS} or step.condition == resolved.runner_os
        if not here or (failed and step.condition != ALWAYS):
            outcomes[step.id] = Outcome.SKIPPED
            continue
        say(f"platform-dispatch: {resolved.source.name} on {resolved.platform.id}: {step.command}")
        environment_here = dict(inherited)
        environment_here.update(
            {
                name: _value(value, outcomes, resolved.runner_os)
                for name, value in step.environment.items()
            }
        )
        code = _run_under_bash(step.command, cwd, environment_here)
        if code == 0:
            outcomes[step.id] = Outcome.SUCCESS
            continue
        outcomes[step.id] = Outcome.FAILURE
        named = f"`{step.command.strip()}`"
        if step.waived:
            say(f"platform-dispatch: {named} failed with {code}, and its failure is waived")
        elif not failed:
            failed = code
            say(f"platform-dispatch: {named} failed with {code}")
    return failed


def _run_under_bash(command: str, cwd: Path, environment: dict[str, str]) -> int:
    """One step's command, run the way Actions runs a step naming no shell."""
    with tempfile.NamedTemporaryFile(
        "w", suffix=".sh", prefix="platform-dispatch-", delete=False, encoding="utf-8"
    ) as script:
        script.write(command)
        if not command.endswith("\n"):
            script.write("\n")
        path = Path(script.name)
    try:
        completed = shell_run(
            ["bash", "--noprofile", "--norc", "-eo", "pipefail", str(path)],
            cwd=cwd,
            env=environment,
            capture=False,
        )
    finally:
        path.unlink(missing_ok=True)
    return completed.returncode


def main(argv: list[str] | None = None) -> int:
    """`resolve` or `run` one (job, platform) pair, as the dispatch workflow does."""
    parser = argparse.ArgumentParser(prog="platform-dispatch", description=__doc__)
    parser.add_argument("verb", choices=("resolve", "run"))
    parser.add_argument("--job", required=True, help="a job of a source workflow")
    parser.add_argument("--platform", required=True, help="a supported platform's identifier")
    parser.add_argument("--root", default=".", help="the tree to read (default: the cwd)")
    parsed = parser.parse_args(argv)
    repo = Repo(Path(parsed.root))
    try:
        resolved = resolve(repo, parsed.job, parsed.platform)
        if parsed.verb == "resolve":
            print(f"runner={resolved.platform.runner}")
            print(f"source={resolved.source.workflow}")
            return 0
        return execute(resolved, cwd=repo.root)
    except DispatchError as error:
        print(f"platform-dispatch: refused: {error}", file=sys.stderr)
        return REFUSED


if __name__ == "__main__":
    sys.exit(main())
