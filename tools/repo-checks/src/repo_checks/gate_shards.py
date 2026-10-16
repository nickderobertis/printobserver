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
  on its own, is a shard of its own; every other tier is the first shard's.
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
#: The tier that is a shard of its own.
END_TO_END = "test-e2e"
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


def gate_tiers(repo: Repo) -> tuple[str, ...]:
    """The tiers `just check` invokes, as `repo-policy.toml`'s `gate.tiers` declares them."""
    return tuple(repo.policy["gate"]["tiers"])


def plan(repo: Repo, count: int) -> tuple[tuple[str, ...], ...]:
    """The gate tiers each of `count` shards runs, first shard first.

    Raises:
        ShardError: If no plan splits the gate into `count` shards.
    """
    tiers = gate_tiers(repo)
    plans = {
        1: (tiers,),
        2: (tuple(tier for tier in tiers if tier != END_TO_END), (END_TO_END,)),
    }
    if count not in plans:
        msg = (
            f"no plan splits the gate into {count} shards: "
            f"{' or '.join(str(one) for one in plans)} (the end-to-end tier apart)"
        )
        raise ShardError(msg)
    return plans[count]


def commands(repo: Repo, tier: str, shard: Shard) -> list[list[str]]:
    """The recipe invocations shard `shard` of the gate at `tier` runs, in order.

    Raises:
        ShardError: If no plan has `shard.count` shards, or a planned tier names no recipe.
        TierError: If `tier` is not one of the gate tiers' tiers.
    """
    tier = tier_named(tier).value
    parsed = recipes(repo.justfile)
    invocations: list[list[str]] = []
    for recipe in plan(repo, shard.count)[shard.index - 1]:
        declared = parsed.get(recipe)
        if declared is None:
            msg = f"the gate tier `{recipe}` names no recipe of the justfile"
            raise ShardError(msg)
        handed = [tier] if TIER_PARAMETER in declared.parameters else []
        invocations.append(["just", recipe, *handed])
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
        print(f"gate-shard {shard.index}/{shard.count}: {' '.join(invocation)}", file=sys.stderr)
        exited = run(invocation, cwd=repo.root, capture=False).returncode
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
            for tiers in plan(repo, parsed.count):
                print(" ".join(tiers))
            return 0
        if parsed.verb == "commands":
            for invocation in commands(repo, parsed.tier, Shard.named(parsed.shard)):
                print(" ".join(invocation))
            return 0
        if parsed.verb == "run":
            return _run(repo, parsed.tier, Shard.named(parsed.shard))
    except (ShardError, TierError) as error:
        print(f"gate-shards: refused: {error}", file=sys.stderr)
        return REFUSED
    return _verdict(parsed.kind, parsed.platform, os.environ)


if __name__ == "__main__":
    sys.exit(main())
