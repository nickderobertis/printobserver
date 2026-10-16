"""A sharded gate runs exactly the unsharded gate's work, and its verdict is over every shard.

Three halves, each driven through the interface the workflow itself uses:

* the selection, through the `just gate-shards` and `commands` verbs, against
  what `just --dry-run check` says the unsharded gate invokes;
* a shard's run, through the real `just`, over a copy whose tier recipes write
  what they were handed rather than running the gate;
* the aggregator's verdict, through the real `just shard-verdict` recipe, handed
  `needs` documents in the shape `${{ toJSON(needs) }}` renders them;

and the workflow's shape, through the `sharded-jobs` check over the committed
`ci.yml` and over copies breaking it one way each.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from collections import Counter
from collections.abc import Callable

import pytest
from closures import closure
from repo_checks.checks_shards import sharded_jobs
from repo_checks.expect import accepted, contains, equal, failing, passing, refused
from repo_checks.gate_shards import main as gate_shards_main
from repo_checks.gate_shards import plan
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

#: The files the checks this module drives read; see `closures`.
CLOSURE = closure("sharded-jobs")

CI = ".github/workflows/ci.yml"
#: The committed sharded platform and how many shards its gate is split into.
SHARDED = "windows-aarch64"
SHARDS = 2
#: One `just <recipe> ['<tier>']` invocation, as `just --dry-run` prints one.
INVOCATION = re.compile(r"just (?P<recipe>[a-z][a-z0-9-]*)(?: '(?P<tier>[a-z]+)')?")


def dry_run_check(tier: str) -> set[tuple[str, ...]]:
    """Every recipe invocation the unsharded `just check` makes at `tier`, as `just` reads it."""
    shown = run(["just", "--dry-run", "check", tier], cwd=REPO_ROOT, timeout=120)
    passing(shown, describing="`just --dry-run check`")
    return {
        (found["recipe"], *([found["tier"]] if found["tier"] else []))
        for found in INVOCATION.finditer(shown.stdout + shown.stderr)
    }


def shard_commands(
    capsys: pytest.CaptureFixture[str], tier: str, shard: str
) -> list[tuple[str, ...]]:
    """What the real selector's `commands` verb says shard `shard` of the gate at `tier` runs."""
    capsys.readouterr()
    equal(gate_shards_main(["--root", str(REPO_ROOT), "commands", tier, shard]), 0)
    return [tuple(line.split()[1:]) for line in capsys.readouterr().out.splitlines()]


@pytest.mark.parametrize("tier", ["affected", "all"])
def test_the_shards_together_run_the_unsharded_gate_and_nothing_twice(
    capsys: pytest.CaptureFixture[str], tier: str
) -> None:
    """Every invocation `just check` makes is made by exactly one shard, at the same tier."""
    selected = [
        invocation
        for index in range(1, SHARDS + 1)
        for invocation in shard_commands(capsys, tier, f"{index}/{SHARDS}")
    ]

    equal(
        [one for one, count in Counter(selected).items() if count > 1],
        [],
        describing="invocations two shards both run",
    )
    equal(set(selected), dry_run_check(tier), describing="the shards' union against `check`")
    equal(shard_commands(capsys, tier, f"{SHARDS}/{SHARDS}"), [("test-e2e", tier)])


def test_the_printed_split_is_the_plan_each_shard_runs() -> None:
    """`just gate-shards` prints one shard a line, and one shard is the whole gate."""
    shown = run(["just", "gate-shards", str(SHARDS)], cwd=REPO_ROOT, timeout=120)

    passing(shown)
    equal(
        [tuple(line.split()) for line in shown.stdout.splitlines()],
        list(plan(Repo(REPO_ROOT), SHARDS)),
    )
    equal(
        set(plan(Repo(REPO_ROOT), 1)[0]),
        {invocation[0] for invocation in dry_run_check("all")},
        describing="the one-shard plan against `check`",
    )


@pytest.mark.parametrize("asked", ["3", "0"])
def test_a_split_no_plan_makes_is_refused(asked: str) -> None:
    """A count no plan splits the gate into is refused rather than guessed at."""
    shown = run(["just", "gate-shards", asked], cwd=REPO_ROOT, timeout=120)

    failing(shown, naming=f"no plan splits the gate into {asked} shards")


@pytest.mark.parametrize("shard", ["3/2", "0/2", "1", "one/two"])
def test_a_shard_that_is_not_k_of_n_is_refused(shard: str) -> None:
    """A shard outside its count, or not `k/n` at all, is refused before anything runs."""
    shown = run(["just", "check-shard", "all", shard], cwd=REPO_ROOT, timeout=120)

    failing(shown, naming=f"shard {shard!r} is not `k/n`")


def recording_justfile(repo: Repo, failing_recipe: str = "") -> str:
    """A justfile whose gate tiers write what they were handed to `reached`.

    Each tier keeps the parameters the committed recipe declares, so the
    selector hands each the tier exactly as it would the real one.
    """
    from repo_checks.parsing import recipes

    declared = recipes(repo.justfile)
    lines = ['set shell := ["bash", "-euo", "pipefail", "-c"]', ""]
    for tier in plan(repo, 1)[0]:
        parameters = declared[tier].parameters
        lines.append(" ".join([tier, *parameters]) + ":")
        handed = " ".join([tier, *(f"{{{{{one}}}}}" for one in parameters)])
        lines.append(f"    echo '{handed}' >> reached")
        if tier == failing_recipe:
            lines.append("    exit 3")
        lines.append("")
    return "\n".join(lines)


def test_a_shard_runs_its_tiers_in_order_handing_each_its_tier(tree: Callable[[], Tree]) -> None:
    """The first shard runs every tier but the end-to-end one, in order; the second runs that."""
    copy = tree()
    copy.write("justfile", recording_justfile(copy.repo))

    for index in (1, 2):
        equal(gate_shards_main(["--root", str(copy.root), "run", "all", f"{index}/2"]), 0)

    equal(
        [line.strip() for line in copy.read("reached").splitlines()],
        [
            "format-check all",
            "lint all",
            "typecheck all",
            "test all",
            "coverage all",
            "build all",
            "lint-workflows",
            "check-repo",
            "test-e2e all",
        ],
    )


def test_a_shard_stops_at_its_first_failing_tier_with_its_exit(tree: Callable[[], Tree]) -> None:
    """As `just check` does: a failed tier fails the shard, and nothing after it runs."""
    copy = tree()
    copy.write("justfile", recording_justfile(copy.repo, failing_recipe="typecheck"))

    exited = gate_shards_main(["--root", str(copy.root), "run", "affected", "1/2"])

    equal(exited, 3, describing="the shard's exit")
    equal(
        [line.strip() for line in copy.read("reached").splitlines()],
        ["format-check affected", "lint affected", "typecheck affected"],
    )


def needs(**results: str) -> str:
    """A `needs` context as `${{ toJSON(needs) }}` renders it: each job's result and outputs."""
    return json.dumps(
        {
            job.replace("_", "-"): {"result": result, "outputs": {}}
            for job, result in results.items()
        },
        indent=2,
    )


def verdict(environment: dict[str, str]) -> subprocess.CompletedProcess[str]:
    """The aggregating job's own step, the real recipe, under `environment`."""
    return run(
        ["just", "shard-verdict", "gate", SHARDED],
        cwd=REPO_ROOT,
        env={**os.environ, **environment},
        timeout=120,
    )


def test_the_verdict_passes_when_every_shard_succeeded() -> None:
    """Every needed job concluded `success`: the required context passes, naming them."""
    ran = verdict({"NEEDS": needs(gate_shard_windows_aarch64="success")})

    passing(ran)
    contains(ran.stdout, f"gate ({SHARDED}): every shard succeeded")


@pytest.mark.parametrize("result", ["failure", "cancelled", "skipped"])
def test_the_verdict_fails_when_a_shard_did_not_succeed(result: str) -> None:
    """A shard that failed, was cancelled or was skipped fails the required context."""
    ran = verdict({"NEEDS": needs(gate_shard_windows_aarch64=result, other="success")})

    failing(ran, naming=f'`gate-shard-windows-aarch64` concluded "{result}"')


@pytest.mark.parametrize(
    ("handed", "naming"),
    [
        ("{}", "names no shard"),
        ("[]", "names no shard"),
        ("not json", "is not JSON"),
        (json.dumps({"gate-shard-windows-aarch64": "success"}), "concluded null"),
    ],
)
def test_a_needs_document_no_verdict_can_be_reached_over_fails(handed: str, naming: str) -> None:
    """Nothing to judge, or something no result can be read from, is no pass."""
    failing(verdict({"NEEDS": handed}), naming=naming)


def test_a_verdict_handed_no_needs_is_refused() -> None:
    """Without the variable there is no verdict at all, and the run says which is missing."""
    environment = {key: value for key, value in os.environ.items() if key != "NEEDS"}
    ran = run(["just", "shard-verdict", "gate", SHARDED], cwd=REPO_ROOT, env=environment)

    failing(ran, naming="`NEEDS` is not set")


def test_the_committed_sharded_gate_is_accepted(committed: Repo) -> None:
    """The committed `ci.yml` is shards and one aggregator, as the check reads them."""
    accepted(sharded_jobs(committed))


AGGREGATOR = "  gate-windows-aarch64:\n"


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        ("    if: always()\n", "", "does not run `if: always()`"),
        (
            "    needs: [gate-shard-windows-aarch64]\n",
            "    needs: [gate]\n",
            "does not `needs` the shard job `gate-shard-windows-aarch64`",
        ),
        (
            "      - run: just shard-verdict gate windows-aarch64\n",
            "      - run: just check-repo\n",
            "the one thing an aggregating job runs is `just shard-verdict gate windows-aarch64`",
        ),
        (
            "          NEEDS: ${{ toJSON(needs) }}\n",
            "          NEEDS: '{}'\n",
            "reads no shard's result",
        ),
        ("    name: gate (windows-aarch64)\n", "    name: gate-verdict\n", "which 0 jobs"),
    ],
)
def test_an_aggregator_that_is_no_verdict_over_its_shards_is_refused(
    tree: Callable[[], Tree], old: str, new: str, naming: str
) -> None:
    """Each way the aggregating job could report a context its shards did not earn."""
    copy = tree()
    text = copy.read(CI)
    at = text.index(AGGREGATOR)
    copy.write(CI, text[:at] + text[at:].replace(old, new, 1))

    refused(sharded_jobs(copy.repo), naming)


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        (
            "            shard: 2\n            shards: 2\n",
            "            shard: 3\n            shards: 2\n",
            "runs every shard 1 to its count",
        ),
        (
            "            shard: 2\n            shards: 2\n",
            "            shard: 2\n            shards: 3\n",
            "runs every shard 1 to its count",
        ),
        ("            shards: 2\n", "            shards: 1\n", "runs every shard 1 to its count"),
        (
            "            runner: windows-11-arm\n            shard: 2",
            "            runner: windows-2025\n            shard: 2",
            "but AGENTS.md declares `windows-11-arm`",
        ),
        (
            '      - run: just check-shard "$(just gate-tier)" "$GATE_SHARD"\n',
            '      - run: just check "$(just gate-tier)"\n',
            "runs no `just check-shard`",
        ),
    ],
)
def test_a_shard_job_that_is_not_every_shard_of_one_platform_is_refused(
    tree: Callable[[], Tree], old: str, new: str, naming: str
) -> None:
    """Each way the shard cells could leave part of the platform's gate unrun."""
    copy = tree()
    copy.edit(CI, old, new)

    refused(sharded_jobs(copy.repo), naming)


def test_a_sharded_platform_the_gate_job_also_carries_is_refused(tree: Callable[[], Tree]) -> None:
    """Two jobs reporting `gate (windows-aarch64)` are two answers to one required context."""
    copy = tree()
    copy.edit(
        CI,
        "          # `windows-aarch64` is a sharded gate",
        "          - id: windows-aarch64\n            runner: windows-11-arm\n"
        "          # `windows-aarch64` is a sharded gate",
    )

    refused(sharded_jobs(copy.repo), "the unsharded job's matrix leaves a sharded platform out")


def test_an_integration_shard_is_refused_while_no_selector_splits_it(
    tree: Callable[[], Tree],
) -> None:
    """The integration tier has no plan, so a job claiming a shard of it runs nothing known."""
    copy = tree()
    copy.edit(
        CI,
        "    name: gate-shard (${{ matrix.platform.id }}",
        "    name: integration-shard (${{ matrix.platform.id }}",
    )

    refused(sharded_jobs(copy.repo), "shards the integration tier, which no selector splits")
