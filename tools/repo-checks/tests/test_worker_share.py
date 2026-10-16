"""The xdist suites that can run at once never ask for more than the host holds.

`repo_checks.workers` is the one answer the end-to-end tier, `repo-checks` and
`release-artifacts` take for `-n auto`. Here it is asked for every suite on the
hosts the gate runs on, at the parallelism this workspace's own `nx.json`
declares, and every set of suites one Nx invocation can run together is summed
against the host. The end-to-end tier is an invocation of its own and keeps the
whole host; where the justfile starts it beside the test tier, the test tier's
suites take one worker each.
"""

from __future__ import annotations

import itertools
import json
import platform
import sys
from pathlib import Path

import pytest
from repo_checks import workers
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo
from treecopy import REPO_ROOT

#: The hosts the gate runs on, and two either side of them.
HOSTS = {
    "the hosted Linux runners": workers.Host(cpus=4, memory_gib=16.0, free_disk_gib=14.0),
    "the hosted Windows runners": workers.Host(
        cpus=4,
        memory_gib=16.0,
        free_disk_gib=14.0,
        reserved=workers.RESERVED_ON_WINDOWS,
        traced=True,
    ),
    "the hosted macOS runner": workers.Host(cpus=3, memory_gib=7.0, free_disk_gib=14.0),
    "a host short of memory": workers.Host(cpus=16, memory_gib=6.0, free_disk_gib=14.0),
    "a workstation": workers.Host(cpus=20, memory_gib=94.0, free_disk_gib=400.0),
}


def _running_together(parallelism: int) -> list[tuple[workers.Suite, ...]]:
    """Every set of xdist suites one Nx invocation can have running at the same moment."""
    sets: list[tuple[workers.Suite, ...]] = []
    for target in sorted({suite.target for suite in workers.SUITES}):
        suites = [suite for suite in workers.SUITES if suite.target == target]
        for size in range(1, min(parallelism, len(suites)) + 1):
            sets.extend(itertools.combinations(suites, size))
    return sets


@pytest.mark.parametrize("beside", [True, False], ids=["tiers-overlapping", "tiers-in-turn"])
@pytest.mark.parametrize("described", sorted(HOSTS))
def test_the_suites_running_together_fit_the_host_at_this_workspaces_parallelism(
    described: str, beside: bool
) -> None:
    """Summed over every set one invocation can run at once, the workers fit the host."""
    host = HOSTS[described]
    parallelism = workers.nx_parallelism(REPO_ROOT)
    memory_workers = int(host.memory_gib // workers.MEMORY_PER_WORKER_GIB)

    for running in _running_together(parallelism):
        counts = {
            str(suite): workers.share(host, suite, parallelism=parallelism, beside=beside)
            for suite in running
        }
        total = sum(counts.values())
        truth(
            total <= max(len(running), host.cpus - host.reserved),
            describing=f"{counts} on {described} within its processors not left to the host",
        )
        truth(
            total <= max(len(running), memory_workers),
            describing=f"{counts} on {described} within {host.memory_gib:g} GiB",
        )
        truth(all(count >= 1 for count in counts.values()), describing=f"{counts}: none empty")


def test_the_hosted_runners_shares_are_the_ones_the_gate_was_sized_for() -> None:
    """Each processor goes somewhere, the left-over ones to the suites with the most to do."""
    parallelism = workers.nx_parallelism(REPO_ROOT)
    shares = {
        (described, beside): tuple(
            workers.share(HOSTS[described], suite, parallelism=parallelism, beside=beside)
            for suite in workers.SUITES
        )
        for described in HOSTS
        for beside in (True, False)
    }

    equal(parallelism, 3, describing="the parallelism `nx.json` declares")
    equal(shares["the hosted Linux runners", True], (4, 1, 1), describing="Linux")
    equal(shares["the hosted macOS runner", True], (3, 1, 1), describing="macOS")
    equal(shares["the hosted Windows runners", False], (3, 1, 1), describing="Windows")
    equal(shares["a workstation", False], (20, 10, 10), describing="tiers in turn, untraced")


@pytest.mark.parametrize("beside", [True, False], ids=["tiers-overlapping", "tiers-in-turn"])
def test_the_end_to_end_tier_keeps_the_host_whether_or_not_the_test_tier_is_beside_it(
    beside: bool,
) -> None:
    """A separate invocation is not divided with: the tier's share is the whole budget."""
    tier = workers.Suite("repo-e2e", "test-e2e")
    for described, host in HOSTS.items():
        equal(
            workers.together(tier, parallelism=3),
            (tier,),
            describing=f"what the end-to-end tier shares with on {described}",
        )
        equal(
            workers.share(host, tier, parallelism=3, beside=beside),
            host.budget,
            describing=described,
        )


def test_the_test_tier_beside_the_end_to_end_tier_runs_one_worker_a_suite() -> None:
    """However large the host, a test-tier suite overlapping the end-to-end tier takes one."""
    for suite in workers.SUITES[1:]:
        for described, host in HOSTS.items():
            equal(
                workers.share(host, suite, parallelism=3, beside=True),
                1,
                describing=f"{suite} on {described}",
            )
            contains(
                workers.said(host, suite, parallelism=3, beside=True),
                f"{suite.project}: 1 xdist workers: ",
            )


def test_a_traced_test_tier_runs_one_worker_a_suite_even_with_the_tiers_in_turn() -> None:
    """Where the test tier traces the whole system, its suites leave the host to the trace."""
    traced = HOSTS["the hosted Windows runners"]
    untraced = workers.Host(cpus=4, memory_gib=16.0, free_disk_gib=14.0, reserved=1)
    for suite in workers.SUITES[1:]:
        equal(workers.share(traced, suite, parallelism=3, beside=False), 1, describing=f"{suite}")
        contains(
            workers.said(traced, suite, parallelism=3, beside=False),
            "the test tier tracing the whole system on this host",
        )
    equal(
        [workers.share(untraced, suite, parallelism=3, beside=False) for suite in workers.SUITES],
        [3, 2, 1],
        describing="the same host, untraced",
    )


def test_the_overlap_is_the_justfiles_own_answer() -> None:
    """The end-to-end tier is beside the test tier everywhere but on Windows aarch64."""
    equal(
        workers.beside_tests(REPO_ROOT),
        not (sys.platform == "win32" and platform.machine() == "ARM64"),
        describing="whether the justfile overlaps the two tiers on this host",
    )


def test_a_tree_whose_justfile_cannot_answer_is_taken_to_overlap(tmp_path: Path) -> None:
    """Where `just` says nothing, the test tier gets the smaller share rather than the larger."""
    truth(workers.beside_tests(tmp_path), describing="the overlap with no justfile to ask")


@pytest.mark.parametrize(
    ("declared", "environ", "parallelism"),
    [
        ({"parallel": 5}, {}, 5),
        ({"parallel": 5}, {"NX_PARALLEL": "2"}, 2),
        ({"parallel": 4}, {"NX_PARALLEL": "several"}, 4),
    ],
    ids=["declared", "environment-wins", "nonsense-in-the-environment-ignored"],
)
def test_the_parallelism_is_nxs_own(
    tmp_path: Path, declared: object, environ: dict[str, str], parallelism: int
) -> None:
    """`NX_PARALLEL`, then `nx.json`'s `parallel`, as Nx reads them."""
    (tmp_path / "nx.json").write_text(json.dumps(declared), encoding="utf-8")

    equal(workers.nx_parallelism(tmp_path, environ), parallelism, describing=f"{declared}")


@pytest.mark.parametrize(
    "declared",
    [{}, {"parallel": 0}, {"parallel": True}, {"parallel": "3"}, [3], None],
    ids=["absent", "none", "boolean", "string", "not-an-object", "null"],
)
def test_a_workspace_declaring_no_parallelism_is_refused(tmp_path: Path, declared: object) -> None:
    """Nothing to divide the host by is a refusal naming the file, not a guess."""
    (tmp_path / "nx.json").write_text(json.dumps(declared), encoding="utf-8")

    with pytest.raises(ValueError, match="declares no `parallel`"):
        workers.nx_parallelism(tmp_path, {})


def test_the_table_names_exactly_the_targets_that_run_under_xdist() -> None:
    """A target whose pytest takes `-n auto` is one of `SUITES`, and every one of them does."""
    found: set[workers.Suite] = set()
    for path in Repo(REPO_ROOT).project_paths:
        project = json.loads(path.read_text(encoding="utf-8"))
        for target, declared in project.get("targets", {}).items():
            command = declared.get("command", "") if isinstance(declared, dict) else ""
            if "pytest" in command and "-n auto" in command:
                found.add(workers.Suite(project["name"], target))

    equal(
        sorted(map(str, found)),
        sorted(map(str, workers.SUITES)),
        describing="the targets running pytest under `-n auto`",
    )


def test_a_lone_slot_runs_one_test_suite_at_a_time() -> None:
    """At parallelism one the two test-tier suites never share; each takes the whole host."""
    host = HOSTS["the hosted Linux runners"]
    checks = workers.Suite("repo-checks", "test")
    artifacts = workers.Suite("release-artifacts", "test")

    equal(workers.share(host, checks, parallelism=1, beside=False), 4, describing="repo-checks")
    equal(workers.share(host, artifacts, parallelism=1, beside=False), 4, describing="artifacts")
    equal(workers.together(artifacts, parallelism=1), (artifacts,), describing="one slot")


def test_a_suite_the_table_does_not_name_is_refused() -> None:
    """A fourth xdist suite has to be declared before it is given a share."""
    with pytest.raises(ValueError, match="not one of the xdist suites"):
        workers.together(workers.Suite("octoprint-env", "test"), parallelism=3)


def test_the_choice_is_said_with_what_it_was_chosen_from(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The count `-n auto` takes is the one the line on standard error states."""
    suite = workers.Suite("release-artifacts", "test")

    count = workers.chosen(suite, REPO_ROOT)

    said = capsys.readouterr().err
    contains(said, f"release-artifacts: {count} xdist workers: ")
    if workers.beside_tests(REPO_ROOT):
        contains(said, "the end-to-end tier running beside the test tier on this host")
    elif workers.here(REPO_ROOT).traced:
        contains(said, "the test tier tracing the whole system on this host")
    else:
        contains(said, f"at Nx parallelism {workers.nx_parallelism(REPO_ROOT)}")
        contains(said, "repo-checks:test", describing="the suite it shares the host with")
