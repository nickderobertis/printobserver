"""Which of the gate's work each shard of a sharded gate runs, and the verdict over them.

A platform whose `gate (<platform>)` does not fit one runner in the time a
platform is held to is split into shard jobs, `gate-shard (<platform>, k/n)`,
and an aggregating job that keeps the required name. This module is both
halves of that split, so the workflow carries no selection of its own:

* `plan COUNT` prints the gate tiers each of `COUNT` shards runs, one shard a
  line. The tiers are `repo-policy.toml`'s `gate.tiers` — exactly the ones
  `just check` invokes — and a plan hands each of them to exactly one shard, in
  the order `gate.tiers` declares, so the shards together run what the
  unsharded gate runs and nothing twice. The end-to-end tier, the one longest
  on its own, is apart from the rest. In two shards it is the second. In three
  its test files run in two parts, `test-e2e[1/2]` and `test-e2e[2/2]`, the
  part a run collects named in `PRINTOBSERVER_E2E_PART`: the first shard runs
  the `test` and `coverage` tiers, which share a shard because the coverage
  floors are read off what that `test` run measured; the second runs the
  first part; and the third runs every other tier and then the second part,
  whose build of the program the `build` tier has already made. The partition
  is `END_TO_END_SECOND_PART`: a fixed set of the tier's test files, every
  other file in the first part, so each test is collected by exactly one part.
  That tier measures no coverage, so its split moves no coverage figure.
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

from repo_checks.gate_tier import TIERS, TierError, tier_named
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
#: Chosen to halve the tier's summed test time, and so that each of
#: `tests/journey.py`'s xdist groups stays within one part: a group's journeys
#: share a build of the repository root that a second runner would pay for
#: again. The first part keeps the `release-plz-drafting` group; this one takes
#: the `repo-root-release-build` and `repo-root-debug-build` groups.
END_TO_END_SECOND_PART = frozenset(
    {
        # repo-root-release-build
        "test_install_powershell_script_journey.py",
        "test_install_script_journey.py",
        "test_registry_proof_journey.py",
        "test_shipped_artifacts_journey.py",
        "test_world_journey.py",
        # repo-root-debug-build
        "test_server_shutdown_journey.py",
        "test_service_manager_journey.py",
        "test_windows_lint_journey.py",
        "test_windows_service_installer_journey.py",
        # no group
        "test_bootstrap_journey.py",
        "test_cargo_build_contract_journey.py",
        "test_commit_msg_journey.py",
        "test_fresh_clone_journey.py",
        "test_platform_matrix_journey.py",
        "test_release_gating_journey.py",
        "test_workflow_validation.py",
    }
)
#: How many parts the end-to-end tier's test files split into where they split.
END_TO_END_PARTS = 2
#: The tiers that share a shard because `coverage` reads what `test` measured.
MEASURED = ("test", "coverage")
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
    """One gate tier a shard runs, or one part of it."""

    tier: str
    part: Shard | None = None

    def __str__(self) -> str:
        """The tier, with the part it is where it is one."""
        return (
            self.tier if self.part is None else f"{self.tier}[{self.part.index}/{self.part.count}]"
        )


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
    """The tiers `just check` invokes, as `repo-policy.toml`'s `gate.tiers` declares them."""
    return tuple(repo.policy["gate"]["tiers"])


def plan(repo: Repo, count: int) -> tuple[tuple[Step, ...], ...]:
    """The gate tiers each of `count` shards runs, first shard first.

    Raises:
        ShardError: If no plan splits the gate into `count` shards.
    """
    tiers = gate_tiers(repo)
    rest = tuple(Step(tier) for tier in tiers if tier != END_TO_END)
    first, second = (Step(END_TO_END, Shard(part, END_TO_END_PARTS)) for part in (1, 2))
    plans = {
        1: (tuple(Step(tier) for tier in tiers),),
        2: (rest, (Step(END_TO_END),)),
        3: (
            tuple(step for step in rest if step.tier in MEASURED),
            (first,),
            (*(step for step in rest if step.tier not in MEASURED), second),
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
    if parts == 1:
        return 1
    if parts == END_TO_END_PARTS:
        return 2 if file_name in END_TO_END_SECOND_PART else 1
    msg = f"the end-to-end tier's test files are split into one part or two, not {parts}"
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
        environment = (
            {}
            if step.part is None
            else {END_TO_END_PART_VARIABLE: f"{step.part.index}/{step.part.count}"}
        )
        invocations.append(Invocation(("just", step.tier, *handed), environment))
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
            env={**os.environ, **invocation.environment},
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
        if parsed.verb == "plan":
            for steps in plan(repo, parsed.count):
                print(" ".join(str(step) for step in steps))
            return 0
        if parsed.verb == "commands":
            for invocation in commands(repo, parsed.tier, Shard.named(parsed.shard)):
                print(invocation)
            return 0
        if parsed.verb == "run":
            return _run(repo, parsed.tier, Shard.named(parsed.shard))
    except (ShardError, TierError) as error:
        print(f"gate-shards: refused: {error}", file=sys.stderr)
        return REFUSED
    return _verdict(parsed.kind, parsed.platform, os.environ)


if __name__ == "__main__":
    sys.exit(main())
