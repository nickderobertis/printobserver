"""`ci.yml` reports every context it reported before, each required one held to `gate-time`.

The committed workflow is read beside the same workflow at `BASELINE`, the commit
this budget was built on, and three things are held of it:

* every check context a pull request was reported before is reported still, and
  each of the ten required gate and integration contexts by jobs running every
  Nx target, over every project carrying it, that the context ran at `BASELINE`
  — the two macOS contexts exactly those, so splitting the combined macOS job
  into one job per lane dropped nothing from either;
* every job reporting one of the ten ends with the `gate-time` step, waived by
  no `continue-on-error`, naming its own context, and with nothing but
  `actions: read` added to the permissions it had;
* and that step's own `run` script, run as `ci.yml` states it against a stand-in
  jobs API serving a recorded run over 1,800 s, fails the context on a pull
  request and passes, still reporting the figure, on the release sweep.
"""

from __future__ import annotations

import json
import re
import tarfile
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import pytest
import yaml
from repo_checks.checks_ci import status_contexts
from repo_checks.expect import contains, equal, truth
from repo_checks.gate_jobs import COMBINED_JOB, INTEGRATION_SHARD_JOB, SHARD_JOB
from repo_checks.gate_time_budget import BUDGET, KIND, PLATFORM, STEP_RUN
from repo_checks.model import Repo
from repo_checks.parsing import Recipe, jobs_of, load_workflow, recipes, run_commands, steps_of
from repo_checks.platforms import supported
from repo_checks.shell import run
from standin_jobs_api import Jobs, serving
from treecopy import REPO_ROOT

#: This branch's merge base with `main`: the workflow every context is held against.
BASELINE = "e6be673e2394891588aa467a911ea5827077ce7c"
CI = ".github/workflows/ci.yml"
#: The supported platforms, read off AGENTS.md's list, so a platform it gains is
#: a pair of required contexts every assertion here reaches.
PLATFORMS = tuple(platform.id for platform in supported(Repo(REPO_ROOT)))
REQUIRED = tuple(
    f"{kind} ({platform})" for kind in ("gate", "integration") for platform in PLATFORMS
)
#: What a baseline tree needs for `status_contexts` and for the targets read here.
READ = (".github", "AGENTS.md", "justfile", "repo-policy.toml", "release-targets.toml")

RECORDED = Path(__file__).parent / "recorded"
#: The finished pull-request run whose combined macOS job ran 2,573 s, so both
#: macOS contexts are over 1,800 s; `test_gate_time_budget.py` reads it too.
OVER_BUDGET = 40235536077
REPOSITORY = "nickderobertis/printobserver"

TIER_TARGETS = re.compile(r"gate_tier run \{\{\s*quote\(tier\)\s*\}\} (?P<targets>[\w: -]+)")
NX_TARGETS = re.compile(r"nx run-many -t (?P<targets>[\w,:-]+)")
CALLS = re.compile(r"\bjust (?P<recipe>[a-z][\w-]*)")


@pytest.fixture(scope="module")
def baseline(tmp_path_factory: pytest.TempPathFactory) -> Repo:
    """The files the checks read, as `BASELINE` committed them."""
    root = tmp_path_factory.mktemp("baseline")
    listed = run(["git", "ls-tree", "-r", "--name-only", BASELINE], cwd=REPO_ROOT, check=True)
    projects = [path for path in listed.stdout.splitlines() if path.endswith("project.json")]
    tarball = tmp_path_factory.mktemp("archive") / "baseline.tar"
    run(
        ["git", "archive", "--format=tar", "-o", str(tarball), BASELINE, *READ, *projects],
        cwd=REPO_ROOT,
        check=True,
    )
    with tarfile.open(tarball) as archive:
        archive.extractall(root, filter="data")
    return Repo(root)


@pytest.fixture(scope="module")
def committed_repo() -> Repo:
    """The committed tree."""
    return Repo(REPO_ROOT)


def workflow(repo: Repo) -> dict[str, Any]:
    """`ci.yml` as YAML reads it, `Any`-valued at that boundary."""
    return load_workflow(repo.root / CI)


def internal(name: str) -> bool:
    """Whether a context is a job's own rather than a check a change is held to.

    A shard or the combined macOS job is internal to the context it is behind,
    so it may be renamed, split or retired; nothing else may.
    """
    for template in (COMBINED_JOB, SHARD_JOB, INTEGRATION_SHARD_JOB):
        pattern = re.escape(template)
        for field in ("platform", "index", "count"):
            pattern = pattern.replace(re.escape("{" + field + "}"), r"[^,()]+")
        if re.fullmatch(pattern, name):
            return True
    return False


def pull_request_contexts(repo: Repo) -> dict[str, str]:
    """Every check context a pull request's run of a workflow reports, and the job reporting it."""
    reported: dict[str, str] = {}
    for context in status_contexts(repo):
        triggers = load_workflow(repo.root / ".github/workflows" / context.file).get("on", {})
        if "pull_request" in (triggers if isinstance(triggers, dict | list) else [triggers]):
            reported[context.name] = f"{context.file}:{context.job}"
    return reported


def _expand(recipe: str, bodies: dict[str, Recipe], seen: set[str]) -> set[str]:
    """Every Nx target `just <recipe>` reaches, through the recipes it calls."""
    if recipe in seen or recipe not in bodies:
        return set()
    seen.add(recipe)
    targets: set[str] = set()
    for line in bodies[recipe].body:
        for found in TIER_TARGETS.finditer(line):
            targets.update(found["targets"].split())
        for found in NX_TARGETS.finditer(line):
            targets.update(found["targets"].split(","))
        for called in CALLS.finditer(line):
            targets |= _expand(called["recipe"], bodies, seen)
    return targets


def behind(repo: Repo, context: str) -> list[str]:
    """The keys of the jobs behind `context`: its reporting job and every job it needs.

    The classifier deciding whether the macOS lane is owed is no work of the
    context's own, and is left out.
    """
    jobs = jobs_of(workflow(repo))
    reporting = {c.name: c.job for c in status_contexts(repo) if c.file == "ci.yml"}[context]
    keys, pending = [], [reporting]
    while pending:
        key = pending.pop()
        if key in keys or key == "classify":
            continue
        keys.append(key)
        needs = jobs[key].get("needs", [])
        pending.extend([needs] if isinstance(needs, str) else needs)
    return keys


def _lane_commands(job: dict[str, Any], kind: str) -> list[str]:
    """The commands a job behind a context runs for it.

    The combined job the baseline ran macOS in did both kinds' work, each lane
    in a step whose `id` is its kind, so only that lane's step is that kind's.
    """
    name = job.get("name")
    platform = name.split(" ", 1)[0] if isinstance(name, str) else ""
    if name != COMBINED_JOB.format(platform=platform):
        return run_commands(job)
    return [str(step["run"]) for step in steps_of(job) if step.get("id") == kind]


def nx_work(repo: Repo, context: str) -> set[tuple[str, str]]:
    """Every (project, target) the jobs behind `context` run, at the tier its run selects.

    A shard runs its share of `just check`'s tiers, and `just check-repo`'s
    `sharded-jobs` holds the shards to running every one of them together, so a
    shard is read as the `check` it is part of. The tier is the same expression
    on every side, `just gate-tier`, so it is read as the one it selects.
    """
    bodies = recipes((repo.root / "justfile").read_text(encoding="utf-8"))
    jobs = jobs_of(workflow(repo))
    kind = context.split(" ", 1)[0]
    targets: set[str] = set()
    for key in behind(repo, context):
        for command in _lane_commands(jobs[key], kind):
            called = CALLS.match(command)
            if called is None:
                continue
            recipe = "check" if called["recipe"] == "check-shard" else called["recipe"]
            targets |= _expand(recipe, bodies, set())
    carrying = set()
    for project in repo.root.glob("**/project.json"):
        if "node_modules" in project.parts:
            continue
        declared = json.loads(project.read_text(encoding="utf-8"))
        carrying |= {
            (declared["name"], target)
            for target in declared.get("targets", {})
            if target in targets
        }
    return carrying


def test_every_context_a_pull_request_reported_is_reported_still(
    baseline: Repo, committed_repo: Repo
) -> None:
    """`llmlint`, `skill-install`, `supply-chain`, `pr-title` and the ten among them."""
    before, now = pull_request_contexts(baseline), pull_request_contexts(committed_repo)
    retired = [name for name in set(before) - set(now) if not internal(name)]

    equal(sorted(retired), [], describing="contexts no longer reported")
    for name in (*REQUIRED, "llmlint", "skill-install", "supply-chain", "pr-title"):
        truth(name in now, describing=f"{name} reported on a pull request")


@pytest.mark.parametrize("context", REQUIRED)
def test_each_required_context_runs_all_the_nx_work_it_ran(
    baseline: Repo, committed_repo: Repo, context: str
) -> None:
    """No project and no target any required context ran at the baseline is dropped."""
    before, now = nx_work(baseline, context), nx_work(committed_repo, context)

    truth(bool(before), describing=f"{context} ran Nx work at the baseline")
    equal(sorted(before - now), [], describing=f"what {context} no longer runs")


@pytest.mark.parametrize("context", ["gate (macos-aarch64)", "integration (macos-aarch64)"])
def test_the_macos_lever_runs_exactly_the_work_macos_ran(
    baseline: Repo, committed_repo: Repo, context: str
) -> None:
    """Each lane, once a step of the combined job, is the whole of its own macOS jobs' work."""
    now, before = nx_work(committed_repo, context), nx_work(baseline, context)
    truth(bool(before), describing=f"{context} ran Nx work at the baseline")
    equal(sorted(now ^ before), [], describing=f"what {context} runs that it did not, or no longer")
    equal(
        behind(committed_repo, context),
        {
            "gate (macos-aarch64)": ["gate-macos", "gate-shard-macos-aarch64"],
            "integration (macos-aarch64)": ["integration-macos"],
        }[context],
        describing=f"the jobs behind {context}",
    )


def _permissions(workflow_document: dict[str, Any], job: dict[str, Any]) -> dict[str, str]:
    """A job's effective permissions: its own, or the workflow's where it declares none."""
    declared = job.get("permissions", workflow_document.get("permissions", {}))
    return dict(declared) if isinstance(declared, dict) else {}


def reporting_jobs(repo: Repo) -> dict[str, str]:
    """Each job key reporting one of the ten required contexts, and the kind it reports."""
    return {
        context.job: context.name.split(" ", 1)[0]
        for context in status_contexts(repo)
        if context.file == "ci.yml" and context.name in REQUIRED
    }


def test_every_required_job_ends_with_the_gate_time_step(
    baseline: Repo, committed_repo: Repo
) -> None:
    """The step is last, enforced, names its own context, and adds `actions: read` alone."""
    now_workflow, before_workflow = workflow(committed_repo), workflow(baseline)
    now_jobs, before_jobs = jobs_of(now_workflow), jobs_of(before_workflow)
    reporting = reporting_jobs(committed_repo)

    equal(
        sorted(reporting),
        sorted(
            [
                "gate",
                "gate-windows-aarch64",
                "gate-windows-x86_64",
                "gate-macos",
                "integration",
                "integration-macos",
            ]
        ),
    )
    for key, kind in reporting.items():
        job = now_jobs[key]
        last = steps_of(job)[-1]
        equal(last.get("name"), BUDGET, describing=f"{key}'s last step")
        equal(last.get("run"), STEP_RUN, describing=f"{key}'s last step's run")
        truth("continue-on-error" not in last, describing=f"{key}'s budget step is not waived")
        truth("if" not in last, describing=f"{key}'s budget step runs where its job succeeded")
        environment = {**job.get("env", {}), **last.get("env", {})}
        equal(environment.get(KIND), kind, describing=f"{key}'s kind")
        truth(PLATFORM in environment, describing=f"{key} names its platform")
        before = _permissions(before_workflow, before_jobs[key])
        equal(
            _permissions(now_workflow, job),
            {**before, "actions": "read"},
            describing=f"{key}'s permissions",
        )


@pytest.fixture
def over_budget() -> Iterator[str]:
    """The stand-in jobs API serving the recorded run whose macOS contexts are over."""
    document = json.loads(
        (RECORDED / f"jobs-{OVER_BUDGET}-attempt-1.json").read_text(encoding="utf-8")
    )
    with serving(Jobs(REPOSITORY, OVER_BUDGET, 1, document)) as address:
        yield address


def _step(repo: Repo, key: str) -> tuple[str, dict[str, str]]:
    """The `gate-time` step of job `key`: its `run` script, and the literal env it runs under."""
    job = jobs_of(workflow(repo))[key]
    step = steps_of(job)[-1]
    environment = {
        name: str(value)
        for name, value in {**job.get("env", {}), **step.get("env", {})}.items()
        if "${{" not in str(value)
    }
    return step["run"], environment


@pytest.mark.parametrize(
    ("key", "head", "status", "said"),
    [
        ("gate-macos", "nick/255/printobserver-release-declaration", 1, "gate-time: over: 2579 s"),
        ("integration-macos", "nick/255/x", 1, "gate-time: over: 2580 s"),
        (
            "gate-macos",
            "release-plz-2026-10-25T21-00-00Z",
            0,
            "over, reported and not enforced on the release sweep: 2579 s",
        ),
    ],
)
def test_the_step_s_own_script_fails_an_over_budget_context_but_on_the_release_sweep(
    committed_repo: Repo,
    over_budget: str,
    monkeypatch: pytest.MonkeyPatch,
    key: str,
    head: str,
    status: int,
    said: str,
) -> None:
    """`run: just gate-time`, exactly as `ci.yml` states it, over a recorded 2,579 s path."""
    script, environment = _step(committed_repo, key)
    for name, value in {
        **environment,
        "GITHUB_API_URL": over_budget,
        "GITHUB_REPOSITORY": REPOSITORY,
        "GITHUB_RUN_ID": str(OVER_BUDGET),
        "GITHUB_RUN_ATTEMPT": "1",
        "GITHUB_EVENT_NAME": "pull_request",
        "GITHUB_HEAD_REF": head,
    }.items():
        monkeypatch.setenv(name, value)
    monkeypatch.delenv("ONEBUDGETSPEC_RESULT", raising=False)

    ran = run(["bash", "-c", script], cwd=REPO_ROOT, timeout=300)

    equal(ran.returncode, status, describing=f"the step's exit:\n{ran.stdout}\n{ran.stderr}")
    contains(ran.stdout + ran.stderr, said)
    contains(ran.stdout, '"name": "macos-aarch64 (gate, integration)"')


def test_every_required_job_s_step_runs_the_same_script(committed_repo: Repo) -> None:
    """One script for all ten, so running the macOS jobs' runs every one of them."""
    scripts = {key: _step(committed_repo, key)[0] for key in reporting_jobs(committed_repo)}

    equal(set(scripts.values()), {STEP_RUN})


def test_the_baseline_read_here_is_the_workflow_before_the_budget(baseline: Repo) -> None:
    """At the baseline no required job ran a budget step, so the comparison is a real one."""
    for key in reporting_jobs(baseline):
        commands = run_commands(jobs_of(workflow(baseline))[key])
        truth(STEP_RUN not in commands, describing=f"{key} at the baseline")
    truth(
        yaml.safe_load((baseline.root / CI).read_text(encoding="utf-8")) is not None,
        describing="the baseline ci.yml",
    )
