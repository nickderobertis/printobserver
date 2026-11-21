"""Which projects a gate tier reaches, and which tier a CI run is for.

The base is resolved by real git over real repositories built here, and the
selection is read through the real `just gate-tier` recipe fed the event a
forge run would carry. The committed workflow is parsed for the half only a
forge can run: that its `lint` and `test` jobs run their halves of the gate at
the affected tier against an explicitly derived base, and that no condition or
`needs` edge can leave a required context unreported on a pull request.
"""

from __future__ import annotations

import os
import stat
from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.checks_ci import status_contexts
from repo_checks.expect import contains, equal, failing, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.gate_tier import (
    AFFECTED,
    ALL,
    TierError,
    release_branch_prefix,
    resolve,
    select,
)
from repo_checks.gate_tier import main as gate_tier_main
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow, marker_block, run_commands, steps_of
from repo_checks.shell import DISCOVERY_CEILING, discovered_no_higher_than, run
from treecopy import REPO_ROOT, Tree

IDENTITY = ("-c", "user.email=tier@printobserver.test", "-c", "user.name=tier")
POLICY = '[repository]\nbase_branch = "main"\n'
PROJECT = '{{"name": "{name}", "root": "{root}", "targets": {{}}}}\n'

#: The release pull request's head branch as `release-plz` names one.
RELEASE_BRANCH = "release-plz-2026-10-06T12-00-00Z"


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` and answer what it printed."""
    return run(["git", *IDENTITY, *arguments], cwd=root, check=True).stdout.strip()


def repository(tmp_path: Path, *, branch: str = "main") -> Repo:
    """A repository of two projects, `alpha` and `beta`, one commit on `branch`."""
    root = tmp_path / "repository"
    for name in ("alpha", "beta"):
        (root / name).mkdir(parents=True)
        (root / name / "project.json").write_text(
            PROJECT.format(name=name, root=name), encoding="utf-8"
        )
        (root / name / "source.txt").write_text(f"{name}\n", encoding="utf-8")
    (root / "repo-policy.toml").write_text(POLICY, encoding="utf-8")
    init_repository(root, branch=branch)
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "chore: two projects")
    return Repo(root)


def on_a_branch(repo: Repo, changed: str) -> str:
    """Commit a change to `changed` on a branch off the base, and answer the base's SHA."""
    base = git(repo.root, "rev-parse", "HEAD")
    git(repo.root, "checkout", "-q", "-b", "change")
    path = repo.path(changed)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("changed\n", encoding="utf-8")
    git(repo.root, "add", "-A")
    git(repo.root, "commit", "-q", "-m", "feat: a change")
    return base


def test_the_affected_tier_diffs_against_the_merge_base_with_the_base_branch(
    tmp_path: Path,
) -> None:
    """With no NX_BASE, the base is the fork point from `main` — never Nx's own default."""
    repo = repository(tmp_path)
    base = on_a_branch(repo, "alpha/source.txt")

    scope = resolve(repo, AFFECTED, {})

    equal(scope.base, base, describing="the base the affected tier diffs against")
    contains(scope.reason, "the merge base of HEAD with main")
    equal(scope.nx("lint")[:4], ["bunx", "nx", "affected", f"--base={base}"])


def test_a_named_base_is_used_as_named(tmp_path: Path) -> None:
    """`nx-set-shas` names the base in CI; a person may name one by ref."""
    repo = repository(tmp_path)
    on_a_branch(repo, "alpha/source.txt")
    first = git(repo.root, "rev-parse", "HEAD~1")

    equal(resolve(repo, AFFECTED, {"NX_BASE": first[:12]}).base, first)
    equal(resolve(repo, AFFECTED, {"NX_BASE": "main"}).base, first)


@pytest.mark.parametrize(
    "named",
    ["main;touch pwned", "$(id)", "-main", "main..HEAD", "refs//heads", "main/", "a b", "HEAD~1"],
)
def test_a_base_that_is_neither_a_ref_name_nor_a_sha_is_refused(tmp_path: Path, named: str) -> None:
    """It came from the environment, so it is validated before git or nx sees it."""
    repo = repository(tmp_path)

    with pytest.raises(TierError, match="neither a plain ref name nor a commit SHA"):
        resolve(repo, AFFECTED, {"NX_BASE": named})


def test_a_base_naming_no_commit_is_refused(tmp_path: Path) -> None:
    """A well-formed base nobody can resolve is not quietly swapped for another."""
    repo = repository(tmp_path)

    with pytest.raises(TierError, match="names no commit"):
        resolve(repo, AFFECTED, {"NX_BASE": "no-such-branch"})


def test_a_head_is_validated_like_a_base(tmp_path: Path) -> None:
    """`nx-set-shas` exports a head too, and it reaches git and nx the same way."""
    repo = repository(tmp_path)

    with pytest.raises(TierError, match="NX_HEAD="):
        resolve(repo, AFFECTED, {"NX_HEAD": "$(id)"})


def test_a_named_head_bounds_the_change_at_that_commit(tmp_path: Path) -> None:
    """What lands after the named head is not this run's change, and what lands before is."""
    repo = repository(tmp_path)
    base = on_a_branch(repo, "alpha/source.txt")
    head = git(repo.root, "rev-parse", "HEAD")
    repo.path("justfile").write_text("later\n", encoding="utf-8")
    git(repo.root, "add", "-A")
    git(repo.root, "commit", "-q", "-m", "chore: a later change no project owns")

    bounded = resolve(repo, AFFECTED, {"NX_BASE": base, "NX_HEAD": head[:12]})

    equal((bounded.base, bounded.head), (base, head), describing="the range a named head bounds")
    equal(
        bounded.nx("lint"),
        ["bunx", "nx", "affected", f"--base={base}", f"--head={head}", "-t", "lint"],
    )
    unbounded = resolve(repo, AFFECTED, {"NX_BASE": base})
    equal(unbounded.base, None, describing="the same base with the later commit in range")
    contains(unbounded.reason, "touches justfile")


def test_a_base_branch_that_is_not_a_ref_name_is_refused(tmp_path: Path) -> None:
    """The policy's base branch reaches git as the merge base's ref, so it is validated too."""
    repo = repository(tmp_path)
    repo.path("repo-policy.toml").write_text(
        '[repository]\nbase_branch = "main;touch pwned"\n', encoding="utf-8"
    )

    with pytest.raises(TierError, match="is not a plain ref name"):
        resolve(Repo(repo.root), AFFECTED, {})


def test_with_no_derivable_base_the_whole_graph_runs(tmp_path: Path) -> None:
    """No base branch to fork from is no change to scope by: fail closed, and say so."""
    repo = repository(tmp_path, branch="elsewhere")

    scope = resolve(repo, AFFECTED, {})

    equal(scope.base, None, describing="the base of a tree with no `main` to fork from")
    contains(scope.reason, "no merge base of HEAD with origin/main or main")
    equal(scope.nx("lint"), ["bunx", "nx", "run-many", "-t", "lint"])


def test_a_change_no_project_owns_runs_the_whole_graph(tmp_path: Path) -> None:
    """The graph cannot say who reads the justfile, so everyone runs."""
    repo = repository(tmp_path)
    on_a_branch(repo, "justfile")

    scope = resolve(repo, AFFECTED, {})

    equal(scope.base, None)
    contains(scope.reason, "touches justfile, which no project owns")


def test_an_uncommitted_change_no_project_owns_runs_the_whole_graph(tmp_path: Path) -> None:
    """Locally the head is the working tree, untracked files and all, as it is for Nx."""
    repo = repository(tmp_path)
    on_a_branch(repo, "alpha/source.txt")
    repo.path("AGENTS.md").write_text("new\n", encoding="utf-8")

    contains(resolve(repo, AFFECTED, {}).reason, "touches AGENTS.md")


def test_a_change_git_cannot_list_runs_the_whole_graph(tmp_path: Path) -> None:
    """A diff git refused is no change at all to scope by, not an empty one."""
    repo = repository(tmp_path)
    base = on_a_branch(repo, "alpha/source.txt")
    tree = git(repo.root, "rev-parse", f"{base}^{{tree}}")
    loose = repo.root / ".git" / "objects" / tree[:2] / tree[2:]
    # git writes a loose object read-only, which Windows refuses to delete.
    loose.chmod(stat.S_IWRITE | stat.S_IREAD)
    loose.unlink()

    scope = resolve(repo, AFFECTED, {})

    equal(scope.base, None, describing="the base of a change git could not list")
    contains(scope.reason, "git could not list the change since")


def test_the_full_sweep_needs_no_base(tmp_path: Path) -> None:
    """`all` is every project whatever the environment names as a base."""
    repo = repository(tmp_path, branch="elsewhere")

    equal(
        resolve(repo, ALL, {"NX_BASE": "$(id)"}).nx("test"),
        ["bunx", "nx", "run-many", "-t", "test"],
    )


def test_an_unknown_tier_is_refused(tmp_path: Path) -> None:
    """A mistyped tier aborts rather than quietly buying a weaker one."""
    with pytest.raises(TierError, match="unknown tier 'everything'"):
        resolve(repository(tmp_path), "everything", {})


@pytest.mark.parametrize("target", ["--parallel=64", "-t", "$(id)", "lint;id"])
def test_a_target_that_is_not_a_target_name_is_refused_before_nx_runs(
    target: str, capsys: pytest.CaptureFixture[str]
) -> None:
    """A value past `--` reaches Nx's own argument list, so one naming no target stops there."""
    equal(gate_tier_main(["run", "all", "--root", str(REPO_ROOT), "--", "lint", target]), 2)

    stderr = capsys.readouterr().err
    contains(stderr, f"{target!r} is not a target name")
    truth("bunx nx" not in stderr, describing=f"no Nx invocation announced, in {stderr!r}")


def gate_tier(event: str, head: str = "") -> str:
    """What the committed `just gate-tier` answers for one forge event."""
    environment = {k: v for k, v in os.environ.items() if not k.startswith("GITHUB_")}
    environment |= {"GITHUB_EVENT_NAME": event, "GITHUB_HEAD_REF": head}
    answered = run(["just", "gate-tier"], cwd=REPO_ROOT, env=environment, timeout=300)
    equal(answered.returncode, 0, describing=f"`just gate-tier`: {answered.stderr}")
    return answered.stdout.strip()


def test_the_release_pull_request_selects_the_sweep() -> None:
    """What `just budgets-release-pr` tells the release pull request apart by."""
    equal(gate_tier("pull_request", RELEASE_BRANCH), ALL)


def test_an_ordinary_pull_request_selects_the_affected_tier() -> None:
    """A change pays for the projects its diff can reach."""
    equal(gate_tier("pull_request", "feat/a-change"), AFFECTED)


def test_a_push_to_the_base_branch_selects_the_affected_tier() -> None:
    """A push to `main` is never the release pull request, so it is never told apart as one."""
    equal(gate_tier("push"), AFFECTED)


def test_a_hand_dispatch_of_a_gate_cell_selects_the_sweep() -> None:
    """A run with no change to scope by fails closed to the whole graph."""
    equal(gate_tier("workflow_dispatch"), ALL)
    equal(select(Repo(REPO_ROOT), {}), ALL)


def test_a_release_branch_prefix_release_plz_is_configured_with_is_honoured(
    tree: Callable[[], Tree],
) -> None:
    """The prefix is read off `release-plz.toml`, not restated beside it."""
    copy = tree()
    copy.edit("release-plz.toml", 'pr_branch_prefix = "release-plz-"', 'pr_branch_prefix = "cut-"')

    event = {"GITHUB_EVENT_NAME": "pull_request"}
    equal(select(copy.repo, {**event, "GITHUB_HEAD_REF": "cut-1"}), ALL)
    equal(select(copy.repo, {**event, "GITHUB_HEAD_REF": RELEASE_BRANCH}), AFFECTED)


def test_a_release_configuration_naming_no_prefix_is_refused(tree: Callable[[], Tree]) -> None:
    """With no prefix stated there is no telling the release pull request apart, so no guess."""
    copy = tree()
    copy.edit("release-plz.toml", 'pr_branch_prefix = "release-plz-"\n', "")

    with pytest.raises(TierError, match="names no `pr_branch_prefix`"):
        select(copy.repo, {"GITHUB_EVENT_NAME": "pull_request", "GITHUB_HEAD_REF": RELEASE_BRANCH})


#: Each `ci.yml` job running a half of the gate, and the recipe it runs it with.
GATE_HALVES = {"lint": "ci-lint", "test": "ci-test", "test-macos": "ci-test"}


@pytest.mark.parametrize(("key", "recipe"), sorted(GATE_HALVES.items()))
def test_each_half_of_the_gate_runs_at_the_affected_tier_against_a_derived_base(
    key: str, recipe: str
) -> None:
    """The half of the selection only a forge runs, read off the committed workflow.

    The job names no tier, so the recipe runs at its own default, which the real
    `just` answers is `affected` — the release pull request's included: the
    sweep is `just check all`, run by hand rather than by any job.
    """
    job = jobs_of(load_workflow(REPO_ROOT / ".github/workflows/ci.yml"))[key]
    commands = run_commands(job)
    uses = [str(step.get("uses", "")) for step in steps_of(job)]

    equal(
        [command for command in commands if command.split()[:2] == ["just", recipe]],
        [f"just {recipe}"],
        describing=f"`{key}`'s run of `just {recipe}`, naming no tier",
    )
    truth(
        not any("gate-tier" in command for command in commands),
        describing=f"`{key}` asking the selector for no tier, in {commands}",
    )
    dry = run(["just", "--dry-run", recipe], cwd=REPO_ROOT, timeout=300)
    equal(dry.returncode, 0, describing=f"`just --dry-run {recipe}`: {dry.stderr}")
    contains(dry.stderr, "'affected'", describing=f"what `just {recipe}` runs at its default")
    truth("'all'" not in dry.stderr, describing=f"`just {recipe}` sweeping nothing: {dry.stderr}")
    truth(
        any(action.startswith("nrwl/nx-set-shas@") for action in uses),
        describing=f"`{key}` deriving NX_BASE explicitly; it uses {uses}",
    )
    checkout = next(
        step for step in steps_of(job) if str(step.get("uses")).startswith("actions/checkout@")
    )
    equal(checkout.get("with", {}).get("fetch-depth"), 0, describing=f"`{key}`'s checkout depth")
    equal(job.get("permissions", {}).get("actions"), "read", describing="what nx-set-shas reads")


def test_an_empty_tier_from_a_selector_that_failed_is_refused_by_the_gate() -> None:
    """A tier recipe handed the empty tier a failed `$(just gate-tier)` answers fails closed."""
    ran = run(["just", "format-check", ""], cwd=REPO_ROOT, timeout=600)

    failing(ran, naming="unknown tier ''")


#: The one condition every job of `ci.yml` but `test-macos` and `budgets` runs under.
PULL_REQUEST_ONLY = "github.event_name == 'pull_request'"


def macos_owed(prefix: str) -> str:
    """The condition `test-macos` runs under: every pull request but release automation's own."""
    return (
        f"{PULL_REQUEST_ONLY} && !(github.event.pull_request.head.repo.full_name == "
        f"github.repository && startsWith(github.head_ref, '{prefix}'))"
    )


#: The status contexts a pull request to the base branch must always report,
#: whatever the affected set reaches — the fixed contract this repository's
#: protection names.
FIXED_CONTEXTS = (
    "lint",
    "test (linux-x86_64)",
    "test (linux-aarch64)",
    "test (macos-aarch64)",
    "test (windows-x86_64)",
    "test (windows-aarch64)",
    "budgets",
    "skilltest",
)


def condition_of(job: dict[str, object]) -> str:
    """A job's `if`, with the whitespace a folded YAML scalar leaves collapsed."""
    return " ".join(str(job.get("if")).split())


def test_every_fixed_context_is_reported_on_every_pull_request() -> None:
    """No condition, path filter or `needs` edge can leave a required context unreported.

    Each context is traced to the job reporting it, and that job's workflow and
    its own declaration are read for anything that could keep it from running
    on a pull request to the base branch whatever the affected set is. The one
    job that may be skipped is `test-macos`, on release automation's own pull
    request, and a job its condition skips reports its context as a success.
    """
    repo = Repo(REPO_ROOT)
    reported = {context.name: context for context in status_contexts(repo)}
    for name in FIXED_CONTEXTS:
        contains(set(reported), name, describing="the contexts the committed workflows report")
        context = reported[name]
        workflow = load_workflow(REPO_ROOT / ".github" / "workflows" / context.file)
        triggers = workflow.get("on", workflow.get(True))
        truth(
            isinstance(triggers, dict) and "pull_request" in triggers,
            describing=f"{context.file} running on a pull request, for `{name}`",
        )
        filters = triggers["pull_request"] or {}
        equal(set(filters) - {"types"}, set(), describing=f"{context.file}'s pull_request filters")
        for kind in ("opened", "synchronize", "reopened"):
            contains(
                filters.get("types", ["opened", "synchronize", "reopened"]),
                kind,
                describing=f"{context.file}'s pull_request activity types",
            )
        jobs = jobs_of(workflow)
        job = jobs[context.job]
        needs = job.get("needs")
        if needs is not None:
            # The one job that waits on others reports however they ended:
            # `always()` is what no result of theirs can skip, and each job it
            # waits on is held to its own condition here as a context is.
            equal(condition_of(job), "always()", describing=f"`{context.job}`'s condition")
            for key in [needs] if isinstance(needs, str) else needs:
                equal(jobs[key].get("needs"), None, describing=f"what `{key}` waits on")
                truth(
                    condition_of(jobs[key])
                    in (PULL_REQUEST_ONLY, macos_owed(release_branch_prefix(repo))),
                    describing=f"`{key}`'s condition, {jobs[key].get('if')!r}",
                )
            continue
        if context.job == "test-macos":
            # Off release automation's version/changelog pull request alone,
            # told apart by the prefix `release-plz.toml` names.
            equal(
                condition_of(job),
                macos_owed(release_branch_prefix(repo)),
                describing="`test-macos`'s condition",
            )
            continue
        equal(
            condition_of(job),
            PULL_REQUEST_ONLY,
            describing=f"`{context.job}`'s condition, holding on a pull request",
        )


def test_the_budgets_job_waits_on_every_other_job_of_the_run() -> None:
    """It measures the run once every other job has ended, so it waits on all of them."""
    jobs = jobs_of(load_workflow(REPO_ROOT / ".github/workflows/ci.yml"))

    equal(
        sorted(jobs["budgets"].get("needs", [])),
        sorted(key for key in jobs if key != "budgets"),
        describing="the jobs `budgets` waits on",
    )


def test_the_required_checks_record_is_the_fixed_contract_in_full() -> None:
    """AGENTS.md names every fixed context and nothing else; `merge-model` holds it to CI."""
    block = marker_block((REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"), "required-checks")
    record = [line[2:].strip("`") for line in block if line.startswith("- ")]

    equal(sorted(record), sorted(FIXED_CONTEXTS))


def a_tree_with_no_history(root: Path) -> Path:
    """The two projects of `repository`, written at `root` with no repository of their own."""
    for name in ("alpha", "beta"):
        (root / name).mkdir(parents=True)
        (root / name / "project.json").write_text(
            PROJECT.format(name=name, root=name), encoding="utf-8"
        )
        (root / name / "source.txt").write_text(f"{name}\n", encoding="utf-8")
    (root / "repo-policy.toml").write_text(POLICY, encoding="utf-8")
    return root


def test_a_tree_with_no_history_inside_a_checkout_sweeps_as_it_does_outside_one(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A copy meant to be no repository is not scoped by the checkout it happens to sit in.

    Under a scratch directory inside some other checkout, git finds that one,
    and the tier diffs the copy against a base commit of a repository it is not
    part of. Bounded at its own root it is no repository again, and fails
    closed to the sweep exactly as a copy outside every checkout does — while a
    repository initialised at that root is still the one git finds.
    """
    outside = a_tree_with_no_history(tmp_path / "outside")
    swept = resolve(Repo(outside), AFFECTED, {}).nx("lint")
    equal(swept, ["bunx", "nx", "run-many", "-t", "lint"], describing="the tier outside a checkout")

    enclosing = tmp_path / "enclosing"
    copy = a_tree_with_no_history(enclosing / "scratch" / "copy")
    init_repository(enclosing)
    git(enclosing, "add", "-A")
    git(enclosing, "commit", "-q", "-m", "chore: a checkout holding a scratch directory")
    equal(
        resolve(Repo(copy), AFFECTED, {}).nx("lint")[:3],
        ["bunx", "nx", "affected"],
        describing="the tier of an unbounded copy, which git places in the enclosing checkout",
    )

    monkeypatch.setenv(
        DISCOVERY_CEILING, discovered_no_higher_than(copy, os.environ)[DISCOVERY_CEILING]
    )

    scope = resolve(Repo(copy), AFFECTED, {})
    equal(scope.nx("lint"), swept, describing="the tier of the bounded copy")
    contains(scope.reason, "no merge base of HEAD with origin/main or main")

    init_repository(copy)
    git(copy, "add", "-A")
    git(copy, "commit", "-q", "-m", "chore: the copy's own history")
    equal(
        Path(git(copy / "alpha", "rev-parse", "--show-toplevel")).resolve(),
        copy.resolve(),
        describing="the repository git finds from inside the bounded copy",
    )
    equal(
        resolve(Repo(copy), AFFECTED, {}).base,
        git(copy, "rev-parse", "HEAD"),
        describing="the base the tier derives from the copy's own history",
    )
