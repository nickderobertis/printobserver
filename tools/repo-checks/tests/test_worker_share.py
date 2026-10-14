"""The xdist suites that can run at once never ask for more than the host holds.

`repo_checks.workers` is the one answer the end-to-end tier, `repo-checks` and
`release-artifacts` take for `-n auto`. Here it is asked for every suite on the
hosts the gate runs on, at the parallelism this workspace's own `nx.json`
declares and with the overlap this tree's own `justfile` answers, and every
set of suites that can run together is summed against the host.
"""

from __future__ import annotations

import itertools
import json
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
        cpus=4, memory_gib=16.0, free_disk_gib=14.0, reserved=workers.RESERVED_ON_WINDOWS
    ),
    "the hosted macOS runner": workers.Host(cpus=3, memory_gib=7.0, free_disk_gib=14.0),
    "a host short of memory": workers.Host(cpus=16, memory_gib=6.0, free_disk_gib=14.0),
    "a workstation": workers.Host(cpus=20, memory_gib=94.0, free_disk_gib=400.0),
}


def _running_together(parallelism: int, beside: bool) -> list[tuple[workers.Suite, ...]]:
    """Every set of xdist suites Nx and the justfile can have running at the same moment."""
    tests = [suite for suite in workers.SUITES if suite.target == "test"]
    ends = [suite for suite in workers.SUITES if suite.target == workers.END_TO_END]
    sets: list[tuple[workers.Suite, ...]] = []
    for size in range(1, min(parallelism, len(tests)) + 1):
        for chosen in itertools.combinations(tests, size):
            sets.append(chosen)
            if beside:
                sets.extend((*chosen, end) for end in ends)
    sets.extend((end,) for end in ends)
    return sets


@pytest.mark.parametrize("beside", [True, False], ids=["tiers-overlapping", "tiers-in-turn"])
@pytest.mark.parametrize("described", sorted(HOSTS))
def test_the_suites_running_together_fit_the_host_at_this_workspaces_parallelism(
    described: str, beside: bool
) -> None:
    """Summed over every set that can run at once, the workers fit the processors and memory."""
    host = HOSTS[described]
    parallelism = workers.nx_parallelism(REPO_ROOT)
    memory_workers = int(host.memory_gib // workers.MEMORY_PER_WORKER_GIB)

    for running in _running_together(parallelism, beside):
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
        for described in ("the hosted Linux runners", "the hosted Windows runners")
        for beside in (True, False)
    }

    equal(parallelism, 3, describing="the parallelism `nx.json` declares")
    equal(shares["the hosted Linux runners", True], (2, 1, 1), describing="e2e beside the tests")
    equal(shares["the hosted Windows runners", False], (3, 2, 1), describing="the tiers in turn")


def test_the_overlap_is_the_justfiles_own_answer() -> None:
    """The end-to-end tier is beside the test tier exactly where the justfile starts it there."""
    equal(
        workers.beside_tests(REPO_ROOT),
        sys.platform != "win32",
        describing="whether the justfile overlaps the two tiers on this host",
    )


def test_a_tree_whose_justfile_cannot_answer_is_taken_to_overlap(tmp_path: Path) -> None:
    """Where `just` says nothing, each suite gets the smaller share rather than the larger."""
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
    host = HOSTS["the hosted Windows runners"]
    checks = workers.Suite("repo-checks", "test")
    artifacts = workers.Suite("release-artifacts", "test")

    equal(workers.share(host, checks, parallelism=1, beside=False), 3, describing="repo-checks")
    equal(workers.share(host, artifacts, parallelism=1, beside=False), 3, describing="artifacts")
    equal(
        workers.together(artifacts, parallelism=1, beside=True),
        (workers.Suite("repo-e2e", "test-e2e"), artifacts),
        describing="what one slot leaves beside release-artifacts with the tiers overlapping",
    )


def test_a_suite_the_table_does_not_name_is_refused() -> None:
    """A fourth xdist suite has to be declared before it is given a share."""
    with pytest.raises(ValueError, match="not one of the xdist suites"):
        workers.together(workers.Suite("octoprint-env", "test"), parallelism=3, beside=True)


def test_the_choice_is_said_with_what_it_was_chosen_from(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The count `-n auto` takes is the one the line on standard error states."""
    suite = workers.Suite("release-artifacts", "test")

    count = workers.chosen(suite, REPO_ROOT)

    said = capsys.readouterr().err
    contains(said, f"release-artifacts: {count} xdist workers: ")
    contains(said, f"at Nx parallelism {workers.nx_parallelism(REPO_ROOT)}")
    contains(said, "repo-checks:test", describing="the suite it shares the host with")
