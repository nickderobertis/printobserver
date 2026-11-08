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
from pathlib import Path
from typing import Any

import pytest
from closures import closure
from repo_checks.checks_graph import projects
from repo_checks.checks_shards import sharded_jobs
from repo_checks.commands import measured_for, measured_sources
from repo_checks.expect import accepted, contains, equal, failing, passing, refused, truth
from repo_checks.gate_shards import (
    END_TO_END_FOURTH_PART,
    END_TO_END_PART_VARIABLE,
    END_TO_END_SECOND_PART,
    END_TO_END_THIRD_PART,
    plan,
)
from repo_checks.gate_shards import REFUSED as REFUSED_SHARD
from repo_checks.gate_shards import main as gate_shards_main
from repo_checks.gate_tier import PROJECTS_VARIABLE, REFUSED, ProjectFilter, command
from repo_checks.gate_tier import main as gate_tier_main
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow, run_commands, steps_of
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

#: The files the checks this module drives read; see `closures`.
CLOSURE = closure("sharded-jobs")

CI = ".github/workflows/ci.yml"
#: The committed sharded platform.
SHARDED = "windows-aarch64"
#: The committed split.
SHARDS = 7
#: Each split a plan makes, and how many parts its end-to-end tier runs in.
SPLITS = ((4, 2), (5, 3), (7, 4))
#: One `just <recipe> ['<tier>']` invocation, as `just --dry-run` prints one.
INVOCATION = re.compile(r"just (?P<recipe>[a-z][a-z0-9-]*)(?: '(?P<tier>[a-z]+)')?")
RUST = f"{PROJECTS_VARIABLE}=lang:rust"
OTHERS = f"{PROJECTS_VARIABLE}=!lang:rust"
PYTHON = f"{PROJECTS_VARIABLE}=!lang:rust,lang:python"
REST = f"{PROJECTS_VARIABLE}=!lang:rust,!lang:python"
#: The sides of the test tier each split measures on, one shard each.
MEASURED_SIDES = {4: (RUST, OTHERS), 5: (RUST, OTHERS), 7: (RUST, PYTHON, REST)}


def dry_run_check(tier: str) -> set[tuple[str, ...]]:
    """Every recipe invocation the unsharded `just check` makes at `tier`, as `just` reads it."""
    shown = run(["just", "--dry-run", "check", tier], cwd=REPO_ROOT, timeout=120)
    passing(shown, describing="`just --dry-run check`")
    return {
        (found["recipe"], *([found["tier"]] if found["tier"] else []))
        for found in INVOCATION.finditer(shown.stdout + shown.stderr)
    }


def shard_commands(capsys: pytest.CaptureFixture[str], tier: str, shard: str) -> list[str]:
    """What the real selector's `commands` verb says shard `shard` of the gate at `tier` runs."""
    capsys.readouterr()
    equal(gate_shards_main(["--root", str(REPO_ROOT), "commands", tier, shard]), 0)
    return capsys.readouterr().out.splitlines()


def invoked(command: str) -> tuple[str, ...]:
    """A printed command's recipe and arguments, without the variables it hands it."""
    words = command.split()
    return tuple(words[words.index("just") + 1 :])


@pytest.mark.parametrize(("count", "parts"), SPLITS)
@pytest.mark.parametrize("tier", ["affected", "all"])
def test_the_shards_together_run_the_unsharded_gate_and_nothing_twice(
    capsys: pytest.CaptureFixture[str], tier: str, count: int, parts: int
) -> None:
    """Every invocation `just check` makes is made by the shards, each part of it once.

    The `test` tier and its `coverage` run on each side of `lang:rust` — the
    seven-way split's other side again on each side of `lang:python` — one
    side a shard, and the end-to-end tier's parts are the remaining shards'.
    """
    shards = [shard_commands(capsys, tier, f"{index}/{count}") for index in range(1, count + 1)]
    selected = [command for shard in shards for command in shard]

    equal(
        [one for one, seen in Counter(selected).items() if seen > 1],
        [],
        describing="commands two shards both run",
    )
    equal(
        {invoked(command) for command in selected},
        dry_run_check(tier),
        describing="the shards' union against `check`",
    )
    measured = {"test", "coverage", "test-e2e"}
    equal(
        [[one for one in shard if invoked(one)[0] in measured] for shard in shards],
        [
            *(
                [f"{side} just test {tier}", f"{side} just coverage {tier}"]
                for side in MEASURED_SIDES[count]
            ),
            *(
                [f"{END_TO_END_PART_VARIABLE}={part}/{parts} just test-e2e {tier}"]
                for part in range(1, parts + 1)
            ),
        ],
        describing="the parts of the tests and their coverage each shard runs",
    )


@pytest.mark.parametrize("tier", ["affected", "all"])
def test_five_shards_leave_non_rust_measurement_on_its_own_runner(
    capsys: pytest.CaptureFixture[str], tier: str
) -> None:
    """The longest Windows measurement gets a runner without other gate tiers."""
    second = shard_commands(capsys, tier, "2/5")
    equal(second, [f"{OTHERS} just test {tier}", f"{OTHERS} just coverage {tier}"])
    first = shard_commands(capsys, tier, "1/5")
    for recipe in ("format-check", "lint", "typecheck", "build", "lint-workflows", "check-repo"):
        contains(
            [invoked(command)[0] for command in first],
            recipe,
            describing="tiers moved beside Rust measurement",
        )


@pytest.mark.parametrize("tier", ["affected", "all"])
def test_seven_shards_split_the_non_rust_measurement_on_python(
    capsys: pytest.CaptureFixture[str], tier: str
) -> None:
    """The Python projects' tests and every other non-Rust project's run on runners of their own.

    The first shard is the five-way split's first, unchanged.
    """
    equal(
        shard_commands(capsys, tier, "2/7"),
        [f"{PYTHON} just test {tier}", f"{PYTHON} just coverage {tier}"],
    )
    equal(
        shard_commands(capsys, tier, "3/7"),
        [f"{REST} just test {tier}", f"{REST} just coverage {tier}"],
    )
    equal(shard_commands(capsys, tier, "1/7"), shard_commands(capsys, tier, "1/5"))


@pytest.mark.parametrize("tier", ["affected", "all"])
def test_two_shards_run_the_end_to_end_tier_beside_every_other_tier(
    capsys: pytest.CaptureFixture[str], tier: str
) -> None:
    """The macOS split: the whole gate, nothing twice, the end-to-end tier on a runner alone."""
    shards = [shard_commands(capsys, tier, f"{index}/2") for index in (1, 2)]
    selected = [command for shard in shards for command in shard]

    equal(
        [one for one, seen in Counter(selected).items() if seen > 1],
        [],
        describing="commands both shards run",
    )
    equal(
        {invoked(command) for command in selected},
        dry_run_check(tier),
        describing="the shards' union against `check`",
    )
    equal(shards[0], [f"just test-e2e {tier}"], describing="what the first shard runs")
    contains(shards[1], f"just test {tier}", describing="the second shard, every project's tests")
    contains(shards[1], f"just coverage {tier}", describing="and their coverage beside them")


@pytest.mark.parametrize("count", [1, 2, *(count for count, _ in SPLITS)])
def test_the_printed_split_is_the_plan_each_shard_runs(count: int) -> None:
    """`just gate-shards` prints one shard a line; together they are the tiers `check` runs."""
    shown = run(["just", "gate-shards", str(count)], cwd=REPO_ROOT, timeout=120)

    passing(shown)
    printed = [line.split() for line in shown.stdout.splitlines()]
    equal(printed, [[str(step) for step in shard] for shard in plan(Repo(REPO_ROOT), count)])
    equal(len(printed), count, describing="the shards printed")
    equal(
        {re.split(r"[\[{]", word)[0] for line in printed for word in line},
        {invocation[0] for invocation in dry_run_check("all")},
        describing=f"the {count}-shard plan against `check`",
    )


@pytest.mark.parametrize("asked", ["3", "0"])
def test_a_split_no_plan_makes_is_refused(asked: str) -> None:
    """A count no plan splits the gate into is refused rather than guessed at."""
    shown = run(["just", "gate-shards", asked], cwd=REPO_ROOT, timeout=120)

    failing(shown, naming=f"no plan splits the gate into {asked} shards")


@pytest.mark.parametrize("shard", ["5/4", "0/4", "1", "one/two"])
def test_a_shard_that_is_not_k_of_n_is_refused(shard: str) -> None:
    """A shard outside its count, or not `k/n` at all, is refused before anything runs."""
    shown = run(["just", "check-shard", "all", shard], cwd=REPO_ROOT, timeout=120)

    failing(shown, naming=f"shard {shard!r} is not `k/n`")


def nx_tasks(narrowed: str) -> set[str]:
    """The Nx tasks the gate's real `test` tier invocation selects, as Nx plans them.

    The invocation is `gate_tier`'s own, for the full sweep and narrowed to
    `narrowed` where one is named; Nx is asked for its task graph instead of
    running it.
    """
    environment = {PROJECTS_VARIABLE: narrowed} if narrowed else {}
    argv, _ = command(Repo(REPO_ROOT), "all", ["test"], environment)
    shown = run(
        [*argv, "--graph=stdout"],
        cwd=REPO_ROOT,
        env={**os.environ, "NX_DAEMON": "false", "NX_NO_CLOUD": "true"},
        timeout=600,
    )
    passing(shown, describing=f"Nx's plan of `{' '.join(argv)}`")
    return set(json.loads(shown.stdout)["tasks"]["tasks"])


def test_the_two_sides_of_the_test_tier_are_its_nx_tasks_none_twice() -> None:
    """Nx plans the Rust side and the other side as two halves of the unsharded tier's tasks.

    The task each side's tests depend on is planned on that side: the schema
    generation beside the Rust crates, the debug build beside the Python suites
    that run it.
    """
    whole = nx_tasks("")
    rust, others = nx_tasks("lang:rust"), nx_tasks("!lang:rust")

    equal(rust & others, set(), describing="tasks both sides run")
    equal(rust | others, whole, describing="the two sides against the unsharded tier")
    python, rest = nx_tasks("!lang:rust,lang:python"), nx_tasks("!lang:rust,!lang:python")
    equal(python & rest, set(), describing="tasks both parts of the non-Rust side run")
    equal(python | rest, others, describing="its two parts against the non-Rust side")
    truth(
        {task for task in python if task.endswith(":test")}
        and {task for task in rest if task.endswith(":test")},
        describing="each part of the non-Rust side running tests of its own",
    )
    contains(rust, "printobserver-types:schemas")
    contains(others, "printobserver:build")
    equal(
        {task for task in whole if task.endswith(":test")},
        {
            f"{project.name}:test"
            for project in projects(Repo(REPO_ROOT))
            if "test" in project.targets
        },
        describing="the unsharded tier against every project with a `test` target",
    )


@pytest.mark.parametrize(
    "named", ["rust", "lang:", "!!lang:rust", "lang:rust,", ",lang:rust", "lang:rust;lang:python"]
)
def test_a_side_that_names_no_tag_is_refused_before_nx_runs(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str], named: str
) -> None:
    """A variable no tag can be read from narrows nothing, so the tier's run is refused.

    Through the runner every graph tier's recipe calls, and in-process, so
    nothing a tier recipe clears first touches the checkout's coverage data.
    """
    monkeypatch.setenv(PROJECTS_VARIABLE, named)

    exited = gate_tier_main(["--root", str(REPO_ROOT), "run", "all", "test"])

    equal(exited, REFUSED, describing="the runner's exit")
    contains(capsys.readouterr().err, f"{PROJECTS_VARIABLE}={named!r} is not a project tag")


@pytest.mark.parametrize(
    ("narrowed", "rust", "python"),
    [
        ("lang:rust", True, False),
        ("!lang:rust", False, True),
        ("!lang:rust,lang:python", False, True),
        ("!lang:rust,!lang:python", False, False),
        ("", None, None),
    ],
)
def test_each_side_s_coverage_is_over_its_own_ecosystem_alone(
    monkeypatch: pytest.MonkeyPatch, narrowed: str, rust: bool | None, python: bool | None
) -> None:
    """Narrowed, the report holds one ecosystem's whole figure and leaves the other unruled.

    Unnarrowed, the full sweep's report is over every project, as before.
    """
    if narrowed:
        monkeypatch.setenv(PROJECTS_VARIABLE, narrowed)
    else:
        monkeypatch.delenv(PROJECTS_VARIABLE, raising=False)
    repo = Repo(REPO_ROOT)
    every = projects(repo)

    scope = measured_for(repo, "all")

    if rust is None:
        equal((scope.rust, scope.python), (None, None), describing="the full sweep's scope")
        return
    crates = tuple(project.root for project in every if "lang:rust" in project.tags)
    sources = measured_sources(repo, every)
    equal(scope.rust, crates if rust else (), describing="the Rust crates the report covers")
    equal(scope.python, sources if python else (), describing="the Python sources it covers")
    contains(scope.reason, f"only {narrowed}'s side")


@pytest.mark.parametrize(
    "tiers",
    ['tiers = "format-check"', "tiers = []", 'tiers = ["lint", "lint"]', 'tiers = ["--all"]'],
)
def test_gate_tiers_no_split_could_run_once_each_are_refused(
    tree: Callable[[], Tree], tiers: str
) -> None:
    """A tier list that is no list, empty, repeated or no recipe name is refused, not split."""
    copy = tree()
    text = copy.read("repo-policy.toml")
    start = text.index("tiers = [", text.index("[gate]"))
    copy.write("repo-policy.toml", text[:start] + tiers + text[text.index("]", start) + 1 :])

    exited = gate_shards_main(["--root", str(copy.root), "plan", str(SHARDS)])

    equal(exited, REFUSED_SHARD, describing="the plan's exit")


def recording_justfile(repo: Repo, failing_recipe: str = "") -> str:
    """A justfile whose gate tiers write what they were handed to `reached`.

    Each tier keeps the parameters the committed recipe declares, so the
    selector hands each the tier exactly as it would the real one, and each
    writes the end-to-end part and the side of the projects it was handed,
    where it was handed one.
    """
    from repo_checks.parsing import recipes

    declared = recipes(repo.justfile)
    lines = ['set shell := ["bash", "-euo", "pipefail", "-c"]', ""]
    for tier in (step.tier for step in plan(repo, 1)[0]):
        parameters = declared[tier].parameters
        lines.append(" ".join([tier, *parameters]) + ":")
        handed = " ".join([tier, *(f"{{{{{one}}}}}" for one in parameters)])
        parts = f"${{{END_TO_END_PART_VARIABLE}:-}}${{{PROJECTS_VARIABLE}:-}}"
        lines.append(f"    echo '{handed}' {parts} >> reached")
        if tier == failing_recipe:
            lines.append("    exit 3")
        lines.append("")
    return "\n".join(lines)


def test_a_shard_runs_its_tiers_in_order_handing_each_its_tier(tree: Callable[[], Tree]) -> None:
    """Shard by shard, each runs its tiers in `gate.tiers` order, handed the tier and part."""
    copy = tree()
    copy.write("justfile", recording_justfile(copy.repo))

    for index in range(1, SHARDS + 1):
        equal(gate_shards_main(["--root", str(copy.root), "run", "all", f"{index}/{SHARDS}"]), 0)

    equal(
        [line.strip() for line in copy.read("reached").splitlines()],
        [
            "format-check all",
            "lint all",
            "typecheck all",
            "test all lang:rust",
            "coverage all lang:rust",
            "build all",
            "lint-workflows",
            "check-repo",
            "test all !lang:rust,lang:python",
            "coverage all !lang:rust,lang:python",
            "test all !lang:rust,!lang:python",
            "coverage all !lang:rust,!lang:python",
            "test-e2e all 1/4",
            "test-e2e all 2/4",
            "test-e2e all 3/4",
            "test-e2e all 4/4",
        ],
    )


def test_a_shard_stops_at_its_first_failing_tier_with_its_exit(tree: Callable[[], Tree]) -> None:
    """As `just check` does: a failed tier fails the shard, and nothing after it runs."""
    copy = tree()
    copy.write("justfile", recording_justfile(copy.repo, failing_recipe="typecheck"))

    exited = gate_shards_main(["--root", str(copy.root), "run", "affected", f"1/{SHARDS}"])

    equal(exited, 3, describing="the shard's exit")
    equal(
        [line.strip() for line in copy.read("reached").splitlines()],
        ["format-check affected", "lint affected", "typecheck affected"],
    )


def collected(part: str, *selection: str) -> tuple[int, list[str]]:
    """The end-to-end tier's collection, as `PRINTOBSERVER_E2E_PART=part` leaves it.

    Through pytest's own collection of the committed suite, with the same
    marker selection its project target runs it under, at the verbosity the
    repository's own `addopts` sets, which lists one test a line; `part` empty
    is the unsharded tier.
    """
    environment = {
        **{key: value for key, value in os.environ.items() if key != END_TO_END_PART_VARIABLE},
        "PYTHONPATH": os.pathsep.join(
            str(REPO_ROOT / one)
            for one in (
                "tools/repo-checks/src",
                "tools/contract-codegen/src",
                "tools/release-artifacts/src",
            )
        ),
        **({END_TO_END_PART_VARIABLE: part} if part else {}),
    }
    shown = run(
        ["uv", "run", "-q", "pytest", "tests/repo-e2e/tests", "--collect-only", *selection],
        cwd=REPO_ROOT,
        env=environment,
        timeout=600,
    )
    return shown.returncode, [line for line in shown.stdout.splitlines() if "::" in line]


@pytest.mark.parametrize("selection", [(), ("-m", "not alone"), ("-m", "alone")])
def test_the_end_to_end_parts_collect_the_whole_tier_and_no_test_twice(
    selection: tuple[str, ...],
) -> None:
    """Split in two by test file, the tier's two parts are its whole collection, disjoint.

    Under each selection the tier's target runs with, each part exits as a
    collection that ran what it was asked does, an empty share included.
    """
    whole = collected("", *selection)
    parts = [collected(f"{index}/2", *selection) for index in (1, 2)]

    equal(whole[0], 0, describing="the unsharded collection's exit")
    equal([exited for exited, _ in parts], [0, 0], describing="each part's exit")
    together = [test for _, tests in parts for test in tests]
    equal(
        [test for test, seen in Counter(together).items() if seen > 1],
        [],
        describing="tests both parts collect",
    )
    equal(sorted(together), sorted(whole[1]), describing="the parts' union against the tier")
    if not selection:
        equal([bool(tests) for _, tests in parts], [True, True], describing="each part's share")
        equal(
            {test.split("::")[0].rsplit("/", 1)[-1] for test in parts[1][1]},
            set(END_TO_END_SECOND_PART),
            describing="the files the second part collects",
        )


@pytest.mark.parametrize("selection", [(), ("-m", "not alone"), ("-m", "alone")])
def test_the_three_end_to_end_parts_collect_the_whole_tier_and_no_test_twice(
    selection: tuple[str, ...],
) -> None:
    """Split in three, the parts are the tier's whole collection, disjoint, each part non-empty.

    The third part collects exactly its own files, and each of the other two
    keeps what it collected of the two-way split but for those.
    """
    whole = collected("", *selection)
    parts = [collected(f"{index}/3", *selection) for index in (1, 2, 3)]

    equal(whole[0], 0, describing="the unsharded collection's exit")
    equal([exited for exited, _ in parts], [0, 0, 0], describing="each part's exit")
    together = [test for _, tests in parts for test in tests]
    equal(
        [test for test, seen in Counter(together).items() if seen > 1],
        [],
        describing="tests two parts collect",
    )
    equal(sorted(together), sorted(whole[1]), describing="the parts' union against the tier")
    if not selection:
        files = [{test.split("::")[0].rsplit("/", 1)[-1] for test in tests} for _, tests in parts]
        equal([bool(one) for one in files], [True, True, True], describing="each part's share")
        equal(files[2], set(END_TO_END_THIRD_PART), describing="the files the third part collects")
        equal(
            files[1],
            set(END_TO_END_SECOND_PART) - END_TO_END_THIRD_PART,
            describing="the files the second part keeps",
        )
        truth(
            not files[0] & (END_TO_END_SECOND_PART | END_TO_END_THIRD_PART),
            describing="the first part collecting no file another part names",
        )


@pytest.mark.parametrize("selection", [(), ("-m", "not alone"), ("-m", "alone")])
def test_the_four_end_to_end_parts_collect_the_whole_tier_and_no_test_twice(
    selection: tuple[str, ...],
) -> None:
    """Split in four, the parts are the tier's whole collection, disjoint, each part non-empty.

    The fourth collects exactly its own files, taken from the second, and the
    other three keep what they collected of the three-way split.
    """
    whole = collected("", *selection)
    parts = [collected(f"{index}/4", *selection) for index in (1, 2, 3, 4)]

    equal(whole[0], 0, describing="the unsharded collection's exit")
    equal([exited for exited, _ in parts], [0, 0, 0, 0], describing="each part's exit")
    together = [test for _, tests in parts for test in tests]
    equal(
        [test for test, seen in Counter(together).items() if seen > 1],
        [],
        describing="tests two parts collect",
    )
    equal(sorted(together), sorted(whole[1]), describing="the parts' union against the tier")
    if not selection:
        files = [{test.split("::")[0].rsplit("/", 1)[-1] for test in tests} for _, tests in parts]
        equal([bool(one) for one in files], [True] * 4, describing="each part's share")
        equal(files[3], set(END_TO_END_FOURTH_PART), describing="the files the fourth collects")
        equal(files[2], set(END_TO_END_THIRD_PART), describing="the files the third collects")
        equal(
            files[1],
            set(END_TO_END_SECOND_PART) - END_TO_END_THIRD_PART - END_TO_END_FOURTH_PART,
            describing="the files the second part keeps",
        )
        truth(
            not files[0]
            & (END_TO_END_SECOND_PART | END_TO_END_THIRD_PART | END_TO_END_FOURTH_PART),
            describing="the first part collecting no file another part names",
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


def test_an_aggregator_may_skip_only_where_its_conditioned_shards_were_skipped(
    tree: Callable[[], Tree], committed: Repo
) -> None:
    """The macOS gate's aggregator is skipped with its shards; a Windows one never is."""
    accepted(sharded_jobs(committed))
    copy = tree()
    text = copy.read(CI)
    at = text.index(AGGREGATOR)
    copy.write(
        CI,
        text[:at]
        + text[at:].replace(
            "    if: always()\n",
            "    if: always() && needs.gate-shard-windows-aarch64.result != 'skipped'\n",
            1,
        ),
    )

    refused(sharded_jobs(copy.repo), "does not run `if: always()`")


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        (
            "            shard: 5\n            shards: 5\n",
            "            shard: 6\n            shards: 5\n",
            "runs every shard 1 to its count",
        ),
        (
            "            shard: 5\n            shards: 5\n",
            "            shard: 5\n            shards: 6\n",
            "runs every shard 1 to its count",
        ),
        ("            shards: 5\n", "            shards: 1\n", "runs every shard 1 to its count"),
        (
            "            runner: windows-11-arm\n            shard: 2",
            "            runner: windows-2025\n            shard: 2",
            "but AGENTS.md declares `windows-11-arm`",
        ),
        (
            '      - run: just check-shard "$(just gate-tier)" "$GATE_SHARD"\n',
            '      - run: just check "$(just gate-tier)"\n',
            "runs no one step `just check-shard",
        ),
        (
            "          GATE_SHARD: ${{ matrix.platform.shard }}/${{ matrix.platform.shards }}\n",
            "          GATE_SHARD: 1/4\n",
            "so its cells do not each run the shard their own matrix values name",
        ),
        (
            "    strategy:\n      fail-fast: false\n      matrix:\n        platform:\n"
            "          - id: windows-aarch64\n",
            "    strategy: sharded\n    unused:\n      matrix:\n        platform:\n"
            "          - id: windows-aarch64\n",
            "is a shard job with no `platform` matrix cells",
        ),
        (
            "            shard: 1\n            shards: 5\n",
            "            shard: true\n            shards: 5\n",
            "runs every shard 1 to its count",
        ),
        (
            "            shard: 1\n            shards: 5\n",
            "            shard: 1\n            shards: [5]\n",
            "whose value is not something a status context can be named after",
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


@pytest.mark.parametrize(
    ("platform", "runner"),
    [
        ("windows-aarch64", "windows-11-arm"),
        ("linux-x86_64", "ubuntu-24.04"),
        ("linux-aarch64", "ubuntu-24.04-arm"),
    ],
)
def test_a_sharded_platform_an_unsharded_gate_job_also_carries_is_refused(
    tree: Callable[[], Tree], platform: str, runner: str
) -> None:
    """Two jobs reporting one sharded platform's `gate` are two answers to one required context.

    No committed job runs the gate unsharded, so the copy gains one carrying the
    platform beside its shards.
    """
    copy = tree()
    anchor = "  gate-shard-linux-x86_64:\n"
    copy.edit(CI, anchor, unsharded_gate_job(platform, runner) + anchor)

    refused(sharded_jobs(copy.repo), "the unsharded job's matrix leaves a sharded platform out")


def unsharded_gate_job(platform: str, runner: str) -> str:
    """A job running the whole gate on one platform's runner, as one did before sharding."""
    return (
        "  gate:\n"
        "    name: gate (${{ matrix.platform.id }})\n"
        "    strategy:\n"
        "      fail-fast: false\n"
        "      matrix:\n"
        "        platform:\n"
        f"          - id: {platform}\n"
        f"            runner: {runner}\n"
        "    runs-on: ${{ matrix.platform.runner }}\n"
        "    steps:\n"
        "      - run: just bootstrap\n"
        '      - run: just check "$(just gate-tier)"\n'
        "\n"
    )


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


#: The platform the newest split is of, its shard job and its aggregating job.
AARCH64 = "linux-aarch64"
AARCH64_SHARDS = "gate-shard-linux-aarch64"
AARCH64_GATE = "gate-linux-aarch64"
#: The recipe an unsharded gate runs the whole of the gate by.
WHOLE_GATE = "check"


def platform_cells(workflow: dict[str, Any], platform: str) -> list[tuple[str, dict[str, Any]]]:
    """Every job of `workflow` running a cell on `platform`, with that cell's matrix values."""
    found: list[tuple[str, dict[str, Any]]] = []
    for key, job in jobs_of(workflow).items():
        strategy = job.get("strategy")
        matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
        entries = matrix.get("platform") if isinstance(matrix, dict) else None
        found.extend(
            (key, entry)
            for entry in entries or []
            if isinstance(entry, dict) and entry.get("id") == platform
        )
    return found


def dispatched_shards(workflow: dict[str, Any], platform: str) -> list[str]:
    """The `k/n` each of `platform`'s gate cells hands its shard step, as CI renders it."""
    shards: list[str] = []
    for key, cell in platform_cells(workflow, platform):
        for step in steps_of(jobs_of(workflow)[key]):
            if str(step.get("run", "")).split()[:2] == ["just", "check-shard"]:
                handed = str(step.get("env", {}).get("GATE_SHARD", ""))
                shards.append(
                    re.sub(
                        r"\$\{\{\s*matrix\.platform\.(\w+)\s*\}\}",
                        lambda found, cell=cell: str(cell.get(found[1], "")),
                        handed,
                    )
                )
    return shards


def partitions(sides: set[str]) -> bool:
    """Whether `sides` admit every project exactly once between them."""
    filters = [ProjectFilter.named(side) for side in sides]
    return all(
        sum(one.admits(project.tags) for one in filters) == 1
        for project in projects(Repo(REPO_ROOT))
    )


def coverage_findings(selected: list[str], unsharded: set[tuple[str, ...]]) -> list[str]:
    """How the commands a split runs differ from the unsharded gate's: omitted, or run twice.

    A recipe is run whole by one command, or in parts that together are all of
    it: both sides of one project tag, or every part `k/n` of the end-to-end tier.
    """
    findings = [f"`{one}` is run twice" for one, seen in Counter(selected).items() if seen > 1]
    by_recipe: dict[tuple[str, ...], set[str]] = {}
    for line in selected:
        handed = " ".join(word for word in line.split() if "=" in word)
        by_recipe.setdefault(invoked(line), set()).add(handed)
    findings.extend(f"`just {' '.join(one)}` is omitted" for one in unsharded - by_recipe.keys())
    findings.extend(
        f"`just {' '.join(one)}` is run by no shard of the unsharded gate"
        for one in by_recipe.keys() - unsharded
    )
    for recipe, parts in by_recipe.items():
        sides = {part.split("=", 1)[1] for part in parts if part.startswith(PROJECTS_VARIABLE)}
        thirds = {
            part.split("=", 1)[1] for part in parts if part.startswith(END_TO_END_PART_VARIABLE)
        }
        whole = "" in parts
        counts = {int(part.split("/")[1]) for part in thirds}
        if whole and len(parts) > 1:
            findings.append(f"`just {' '.join(recipe)}` is run whole and in parts")
        elif sides and not partitions(sides):
            findings.append(
                f"`just {' '.join(recipe)}` is omitted for a side: run for {sorted(sides)}"
            )
        elif thirds and (
            len(counts) != 1 or thirds != {f"{k}/{n}" for n in counts for k in range(1, n + 1)}
        ):
            findings.append(
                f"`just {' '.join(recipe)}` is omitted for a part: run for {sorted(thirds)}"
            )
    return findings


def aarch64_work(
    capsys: pytest.CaptureFixture[str], workflow: dict[str, Any], tier: str
) -> list[str]:
    """Every command `linux-aarch64`'s dispatched shards run at `tier`, by the real selector."""
    return [
        command
        for shard in dispatched_shards(workflow, AARCH64)
        for command in shard_commands(capsys, tier, shard)
    ]


def test_linux_aarch64_runs_no_unsharded_gate() -> None:
    """Its every gate cell runs a shard, and its context is its aggregator's alone."""
    workflow = load_workflow(REPO_ROOT / CI)
    cells = platform_cells(workflow, AARCH64)
    gate_cells = [
        key
        for key, _ in cells
        if any(
            command.split()[:2] in (["just", WHOLE_GATE], ["just", "check-shard"])
            for command in run_commands(jobs_of(workflow)[key])
        )
    ]

    equal(sorted(set(gate_cells)), [AARCH64_SHARDS], describing="the jobs running its gate")
    equal(
        [
            key
            for key, job in jobs_of(workflow).items()
            if job.get("name") == f"gate ({AARCH64})"
            or any(command.split()[:2] == ["just", WHOLE_GATE] for command in run_commands(job))
        ],
        [AARCH64_GATE],
        describing="the jobs reporting its context or running the whole gate",
    )


def test_linux_aarch64_dispatches_exactly_the_planners_shards() -> None:
    """Each cell hands its shard step its own `k/n`, and the cells are every shard of one plan."""
    workflow = load_workflow(REPO_ROOT / CI)
    shards = dispatched_shards(workflow, AARCH64)
    count = len(shards)

    equal(shards, [f"{index}/{count}" for index in range(1, count + 1)])
    equal(
        len(plan(Repo(REPO_ROOT), count)),
        count,
        describing=f"the planner's {count}-way split of the gate",
    )


@pytest.mark.parametrize("tier", ["affected", "all"])
def test_linux_aarch64_shards_run_exactly_the_unsharded_gates_work(
    capsys: pytest.CaptureFixture[str], tier: str
) -> None:
    """What its dispatched shards run, through the real selector, is `just check` once over."""
    work = aarch64_work(capsys, load_workflow(REPO_ROOT / CI), tier)

    equal(coverage_findings(work, dry_run_check(tier)), [], describing="omitted or doubled work")


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        pytest.param(
            "            shard: 5\n            shards: 5\n",
            "            shard: 4\n            shards: 5\n",
            "is run twice",
            id="a-shard-run-twice",
        ),
        pytest.param(
            "            shard: 2\n            shards: 5\n",
            "            shard: 1\n            shards: 5\n",
            "is omitted for a side",
            id="the-non-rust-tests-omitted",
        ),
        pytest.param(
            "            shard: 4\n            shards: 5\n",
            "            shard: 3\n            shards: 5\n",
            "is omitted for a part",
            id="an-end-to-end-part-omitted",
        ),
    ],
)
def test_linux_aarch64_shards_omitting_or_doubling_work_are_found(
    capsys: pytest.CaptureFixture[str], tmp_path: Path, old: str, new: str, naming: str
) -> None:
    """A cell handed another cell's shard leaves one part unrun and runs another twice."""
    text = (REPO_ROOT / CI).read_text(encoding="utf-8")
    at = text.index(f"  {AARCH64_SHARDS}:\n")
    broken = tmp_path / "ci.yml"
    broken.write_text(text[:at] + text[at:].replace(old, new, 1), encoding="utf-8")

    work = aarch64_work(capsys, load_workflow(broken), "all")

    contains("\n".join(coverage_findings(work, dry_run_check("all"))), naming)


def aggregator_verdict(result: str) -> subprocess.CompletedProcess[str]:
    """`gate (linux-aarch64)`'s own verdict step, as committed, over its shards' `result`.

    The `needs` document is built from the jobs the aggregator declares it
    needs, as `${{ toJSON(needs) }}` renders it: a matrix job's one result.
    """
    job = jobs_of(load_workflow(REPO_ROOT / CI))[AARCH64_GATE]
    step = next(one for one in steps_of(job) if "NEEDS" in (one.get("env") or {}))
    handed = needs(**{need.replace("-", "_"): result for need in job["needs"]})
    return run(
        str(step["run"]).split(),
        cwd=REPO_ROOT,
        env={**os.environ, "NEEDS": handed},
        timeout=120,
    )


def test_linux_aarch64_s_context_passes_when_every_shard_succeeded() -> None:
    """The aggregator's verdict passes, naming the shard job it needs."""
    ran = aggregator_verdict("success")

    passing(ran)
    contains(ran.stdout, f"gate ({AARCH64}): every shard succeeded ({AARCH64_SHARDS})")


@pytest.mark.parametrize("result", ["failure", "cancelled", "skipped"])
def test_linux_aarch64_s_context_fails_when_a_shard_failed_or_never_ran(result: str) -> None:
    """A shard that failed, or was cancelled or skipped before it ran, fails `gate (linux-aarch64)`.

    The aggregator runs `if: always()`, so it reaches this verdict whatever its
    shards concluded; the `sharded-jobs` check holds it to that.
    """
    job = jobs_of(load_workflow(REPO_ROOT / CI))[AARCH64_GATE]
    equal(job.get("if"), "always()", describing="the aggregator's condition")

    failing(aggregator_verdict(result), naming=f'`{AARCH64_SHARDS}` concluded "{result}"')
