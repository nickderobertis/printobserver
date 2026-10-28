"""Which of the gate's work each shard of a sharded gate runs, and the verdict over them.

A platform whose `gate (<platform>)` does not fit one runner in the time a
platform is held to is split into shard jobs, `gate-shard (<platform>, k/n)`,
and an aggregating job that keeps the required name. This module is both
halves of that split, so the workflow carries no selection of its own:

* `plan COUNT` prints the gate tiers each of `COUNT` shards runs, one shard a
  line. The tiers are `repo-policy.toml`'s `gate.tiers` — exactly the ones
  `just check` invokes — and a plan hands each of them to exactly one shard, in
  the order `gate.tiers` declares, so the shards together run what the
  unsharded gate runs and nothing twice. Split four ways, the `test` tier
  and its `coverage` run on each side of the `lang:rust` tag (`{lang:rust}`
  and `{!lang:rust}`), which share no Nx task, so the Rust crates' tests and
  every other project's run on two runners and each side's coverage is judged
  at its own ecosystem's floor over exactly the code its tests measured; every
  other tier but the end-to-end one runs beside the non-Rust side; and the
  end-to-end tier's test files run in two parts, `test-e2e[1/2]` and
  `test-e2e[2/2]`, the part a run collects named in `PRINTOBSERVER_E2E_PART`.
  The partition is `END_TO_END_SECOND_PART`: a fixed set of the tier's test
  files, every other file in the first part, so each test is collected by
  exactly one part. Split two ways, for a runner of three processors, the whole
  end-to-end tier runs on one shard and every other tier, each over every
  project, on the other. Split five ways, the non-measured tiers move beside the Rust tests,
  leaving the longer non-Rust measurement on its own runner, and the
  end-to-end tier runs in three parts instead, `test-e2e[1/3]` to
  `test-e2e[3/3]`: `END_TO_END_THIRD_PART` takes its files from both halves,
  and each half keeps the rest of its own. That tier measures no coverage, so
  its split moves no coverage figure.
* `commands TIER SHARD` prints the recipe invocations shard `SHARD` (`k/n`) of
  the gate at `TIER` runs, one a line, and runs none of them.
* `run TIER SHARD` runs shard `SHARD` (`k/n`) of the gate at `TIER`: each of its
  tiers' recipes, handed the tier where the recipe takes one, in order and
  stopping at the first that fails, as `just check` does. Each tier recipe still
  selects its own projects through `repo_checks.gate_tier`, so a shard's Nx
  tasks for a tier are the unsharded gate's, and `coverage` holds the floors
  over the code that shard's own `test` run reached — which, the `test` tier
  being in the same shard, is the code the unsharded gate's coverage reads.
* `verdict KIND PLATFORM` is the aggregating job's one step. It reads the
  workflow's `needs` context as `${{ toJSON(needs) }}` hands it over, from the
  `NEEDS` variable, and fails unless every job it needs concluded `success`: a
  shard that failed, was cancelled or was skipped fails the required context.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

from repo_checks.gate_tier import (
    PROJECTS_VARIABLE,
    TARGET_NAME,
    TIERS,
    ProjectFilter,
    TierError,
    tier_named,
)
from repo_checks.model import Repo
from repo_checks.parsing import recipes
from repo_checks.shell import run

#: The recipe a shard job runs, and the one its aggregating job runs.
SHARD_RECIPE = "check-shard"
VERDICT_RECIPE = "shard-verdict"
#: The variable the aggregating job hands the workflow's `needs` context in.
NEEDS_VARIABLE = "NEEDS"
#: The one result a needed job may conclude with for the verdict to pass.
SUCCESS = "success"
#: The tier that is a shard of its own, or two.
END_TO_END = "test-e2e"
#: The variable naming the part (`k/n`) of the end-to-end tier's test files a
#: run collects; absent, a run collects all of them.
END_TO_END_PART_VARIABLE = "PRINTOBSERVER_E2E_PART"
#: The end-to-end tier's test files its second part collects when it is split in
#: two; the first collects every other file, so a file nobody placed is run.
#: Chosen to even out the two parts' time on a cold Windows runner. The
#: `repo-root-release-build` group stays within this part, since its journeys
#: share a release build of the repository root that the other part's runner
#: would pay for again; the debug build is each part's anyway, because the
#: tier's own target depends on it, so that group's files sit in both.
END_TO_END_SECOND_PART = frozenset(
    {
        # repo-root-release-build
        "test_install_powershell_script_journey.py",
        "test_install_script_journey.py",
        "test_registry_proof_journey.py",
        "test_shipped_artifacts_journey.py",
        "test_world_journey.py",
        # repo-root-debug-build, whose other files are the first part's
        "test_windows_lint_journey.py",
        "test_windows_service_installer_journey.py",
        # no group
        "test_bootstrap_journey.py",
        "test_cargo_build_contract_journey.py",
        "test_commit_msg_journey.py",
        "test_fresh_clone_journey.py",
        "test_platform_matrix_journey.py",
        "test_release_gating_journey.py",
        "test_release_workflow_journey.py",
        "test_workflow_validation.py",
    }
)
#: The end-to-end tier's test files its third part collects when it is split in
#: three, taken from both halves; the first and second parts keep the rest of
#: their own. Chosen from a saturated Windows ARM run, 40295485407, whose halves
#: took 1,174 s and 1,227 s: the first half's serial `release-plz-drafting`
#: group is split across two parts, its `test_release_tag_journey.py` here, and
#: the heaviest files without a group join it — each part's runner pays a
#: group's own setup once, so a group split across parts costs only that.
END_TO_END_THIRD_PART = frozenset(
    {
        # release-plz-drafting, whose other file is the first part's
        "test_release_tag_journey.py",
        # no group
        "test_gate_tier_journey.py",
        "test_release_gating_journey.py",
        "test_sdk_project_targets.py",
    }
)

#: How many parts the end-to-end tier's test files split into in a four-way
#: plan, and in a five-way one.
END_TO_END_PARTS = 2
END_TO_END_THIRDS = 3
#: The variables a step names its part in. Each step is handed only those it
#: names, so a tier the plan runs whole is never narrowed by one a caller left set.
PARTS = (END_TO_END_PART_VARIABLE, PROJECTS_VARIABLE)
#: The tiers that share a shard because `coverage` reads what `test` measured,
#: and that a four-way split runs on each side of `SPLIT_ON`: the Rust crates on
#: one runner, every other project on another, each side's coverage judged at
#: its own ecosystem's floor over the code that side's tests measured.
MEASURED = ("test", "coverage")
SPLIT_ON = ProjectFilter("lang:rust", negated=False)
#: The parameter a tier recipe takes its tier through.
TIER_PARAMETER = "tier"
#: The kinds of required job a verdict is reached for.
KINDS = ("gate", "integration")

#: The exit a refused argument answers with, apart from a recipe's own exit.
REFUSED = 2

SHARD = re.compile(r"^(?P<index>[1-9][0-9]*)/(?P<count>[1-9][0-9]*)$")


class ShardError(ValueError):
    """A shard, a plan or a `needs` document this module refuses to act on."""


@dataclass(frozen=True, slots=True)
class Shard:
    """One shard of `count`, `index` from 1."""

    index: int
    count: int

    @classmethod
    def named(cls, named: str) -> Shard:
        """The shard a caller named as `k/n`.

        Raises:
            ShardError: If it is not `k/n` with `k` from 1 to `n`.
        """
        found = SHARD.match(named)
        if found is None or int(found["index"]) > int(found["count"]):
            msg = f"shard {named!r} is not `k/n` with k from 1 to n"
            raise ShardError(msg)
        return cls(int(found["index"]), int(found["count"]))


@dataclass(frozen=True, slots=True)
class Step:
    """One gate tier a shard runs, or one part of it.

    A part is either one of the end-to-end tier's two halves of test files, or
    one side of a project tag (`gate_tier.ProjectFilter`) for a tier run over
    only that side's projects.
    """

    tier: str
    part: Shard | None = None
    projects: ProjectFilter | None = None

    def __str__(self) -> str:
        """The tier, with the part or the side it is where it is one."""
        part = "" if self.part is None else f"[{self.part.index}/{self.part.count}]"
        side = "" if self.projects is None else f"{{{self.projects}}}"
        return f"{self.tier}{part}{side}"

    @property
    def environment(self) -> dict[str, str]:
        """What its recipe is handed beyond the caller's environment to run only this part."""
        handed: dict[str, str] = {}
        if self.part is not None:
            handed[END_TO_END_PART_VARIABLE] = f"{self.part.index}/{self.part.count}"
        if self.projects is not None:
            handed[PROJECTS_VARIABLE] = str(self.projects)
        return handed


@dataclass(frozen=True, slots=True)
class Invocation:
    """One recipe a shard runs, and what it hands it beyond the caller's environment."""

    argv: tuple[str, ...]
    environment: Mapping[str, str]

    def __str__(self) -> str:
        """As a shell would read it: the variables, then the command."""
        handed = [f"{name}={value}" for name, value in sorted(self.environment.items())]
        return " ".join([*handed, *self.argv])


def gate_tiers(repo: Repo) -> tuple[str, ...]:
    """The tiers `just check` invokes, as `repo-policy.toml`'s `gate.tiers` declares them.

    Raises:
        ShardError: If that is not a non-empty list of distinct recipe names, so
            no split of it could be said to run each tier exactly once.
    """
    gate = repo.policy.get("gate")
    tiers = gate.get("tiers") if isinstance(gate, dict) else None
    if (
        not isinstance(tiers, list)
        or not tiers
        or not all(isinstance(tier, str) and TARGET_NAME.match(tier) for tier in tiers)
        or len(set(tiers)) != len(tiers)
    ):
        msg = f"repo-policy.toml's `gate.tiers` is {tiers!r}, not a list of distinct recipe names"
        raise ShardError(msg)
    return tuple(tiers)


def plan(repo: Repo, count: int) -> tuple[tuple[Step, ...], ...]:
    """The gate tiers each of `count` shards runs, first shard first.

    Raises:
        ShardError: If no plan splits the gate into `count` shards.
    """
    tiers = gate_tiers(repo)
    rust, others = SPLIT_ON, ProjectFilter(SPLIT_ON.tag, negated=True)
    plans = {
        1: (tuple(Step(tier) for tier in tiers),),
        4: (
            tuple(Step(tier, projects=rust) for tier in tiers if tier in MEASURED),
            tuple(
                Step(tier, projects=others if tier in MEASURED else None)
                for tier in tiers
                if tier != END_TO_END
            ),
            *((Step(END_TO_END, Shard(part, END_TO_END_PARTS)),) for part in (1, 2)),
        ),
        2: (
            (Step(END_TO_END),),
            tuple(Step(tier) for tier in tiers if tier != END_TO_END),
        ),
        5: (
            tuple(
                Step(tier, projects=rust if tier in MEASURED else None)
                for tier in tiers
                if tier != END_TO_END
            ),
            tuple(Step(tier, projects=others) for tier in tiers if tier in MEASURED),
            *((Step(END_TO_END, Shard(part, END_TO_END_THIRDS)),) for part in (1, 2, 3)),
        ),
    }
    if count not in plans:
        msg = (
            f"no plan splits the gate into {count} shards: "
            f"{', '.join(str(one) for one in plans)} (the end-to-end tier apart)"
        )
        raise ShardError(msg)
    return plans[count]


def end_to_end_part(file_name: str, parts: int) -> int:
    """Which of `parts` parts of the end-to-end tier collects the test file `file_name`.

    Raises:
        ShardError: If the tier is split into a number of parts no partition makes.
    """
    match parts:
        case 1:
            return 1
        case 2:
            return 2 if file_name in END_TO_END_SECOND_PART else 1
        case 3 if file_name in END_TO_END_THIRD_PART:
            return 3
        case 3:
            return 2 if file_name in END_TO_END_SECOND_PART else 1
        case _:
            msg = (
                f"the end-to-end tier's test files are split into one, two or three parts, "
                f"not {parts}"
            )
            raise ShardError(msg)


def commands(repo: Repo, tier: str, shard: Shard) -> list[Invocation]:
    """The recipe invocations shard `shard` of the gate at `tier` runs, in order.

    Raises:
        ShardError: If no plan has `shard.count` shards, or a planned tier names no recipe.
        TierError: If `tier` is not one of the gate tiers' tiers.
    """
    tier = tier_named(tier).value
    parsed = recipes(repo.justfile)
    invocations: list[Invocation] = []
    for step in plan(repo, shard.count)[shard.index - 1]:
        declared = parsed.get(step.tier)
        if declared is None:
            msg = f"the gate tier `{step.tier}` names no recipe of the justfile"
            raise ShardError(msg)
        handed = (tier,) if TIER_PARAMETER in declared.parameters else ()
        invocations.append(Invocation(("just", step.tier, *handed), step.environment))
    return invocations


def verdict(needs: object) -> list[str]:
    """Why the needed jobs fail the required context, or nothing where every one succeeded.

    `needs` is the workflow's `needs` context as `toJSON` renders it: each
    needed job's key mapped to an object carrying its `result`.
    """
    if not isinstance(needs, Mapping) or not needs:
        return [f"`needs` is {json.dumps(needs)}, which names no shard to reach a verdict over"]
    failed: list[str] = []
    for job, entry in sorted(needs.items(), key=lambda item: str(item[0])):
        result = entry.get("result") if isinstance(entry, Mapping) else None
        if result != SUCCESS:
            failed.append(f"`{job}` concluded {json.dumps(result)}, not {SUCCESS!r}")
    return failed


def _verdict(kind: str, platform: str, environ: Mapping[str, str]) -> int:
    """The aggregating job's exit, saying which shard failed it."""
    required = f"{kind} ({platform})"
    named = environ.get(NEEDS_VARIABLE)
    if named is None:
        print(f"{required}: refused: `{NEEDS_VARIABLE}` is not set", file=sys.stderr)
        return REFUSED
    try:
        needs = json.loads(named)
    except json.JSONDecodeError as error:
        print(f"{required}: refused: `{NEEDS_VARIABLE}` is not JSON: {error}", file=sys.stderr)
        return REFUSED
    failed = verdict(needs)
    for reason in failed:
        print(f"{required}: {reason}", file=sys.stderr)
    if failed:
        return 1
    print(f"{required}: every shard succeeded ({', '.join(sorted(needs))})")
    return 0


def _run(repo: Repo, tier: str, shard: Shard) -> int:
    """Run one shard's recipes in order, stopping at the first that fails."""
    for invocation in commands(repo, tier, shard):
        print(f"gate-shard {shard.index}/{shard.count}: {invocation}", file=sys.stderr)
        exited = run(
            list(invocation.argv),
            cwd=repo.root,
            env={
                **{name: value for name, value in os.environ.items() if name not in PARTS},
                **invocation.environment,
            },
            capture=False,
        ).returncode
        if exited != 0:
            return exited
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    """`plan` a split, `run` one shard of it, or reach the `verdict` over the shards."""
    parser = argparse.ArgumentParser(prog="gate-shards", description=__doc__)
    verbs = parser.add_subparsers(dest="verb", required=True)
    planned = verbs.add_parser("plan", help="print each shard's gate tiers, one shard a line")
    planned.add_argument("count", type=int)
    for verb, said in (
        ("commands", "print what one shard of the gate at a tier runs"),
        ("run", "run one shard of the gate at a tier"),
    ):
        sharded = verbs.add_parser(verb, help=said)
        sharded.add_argument("tier", help=f"one of {', '.join(TIERS)}")
        sharded.add_argument("shard", help="`k/n`")
    judged = verbs.add_parser("verdict", help="fail unless every needed shard succeeded")
    judged.add_argument("kind", choices=KINDS)
    judged.add_argument("platform")
    parser.add_argument("--root", default=".", help="the tree to read (default: the cwd)")
    parsed = parser.parse_args(argv)
    repo = Repo(Path(parsed.root))

    try:
        match parsed.verb:
            case "plan":
                for steps in plan(repo, parsed.count):
                    print(" ".join(str(step) for step in steps))
                return 0
            case "commands":
                for invocation in commands(repo, parsed.tier, Shard.named(parsed.shard)):
                    print(invocation)
                return 0
            case "run":
                return _run(repo, parsed.tier, Shard.named(parsed.shard))
            case _:
                return _verdict(parsed.kind, parsed.platform, os.environ)
    except (ShardError, TierError) as error:
        print(f"gate-shards: refused: {error}", file=sys.stderr)
        return REFUSED


if __name__ == "__main__":
    sys.exit(main())
