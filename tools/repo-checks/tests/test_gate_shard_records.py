"""The committed shard records describe the split `ci.yml` dispatches, and fit `gate-time`.

`repo_checks/gate_shard_records.toml` records, for each aarch64 gate split into
shards, what each shard runs and how long that took on the platform's runner.
These tests hold that record against three sources. The split comes from the
planner `just gate-shards` runs, the shard count from the committed workflow's
own cells, and the budget from the root `budgets.yaml`. The tests read the
committed record, and copies of it broken one way each.
"""

from __future__ import annotations

import copy
import tomllib
from collections import Counter
from collections.abc import Callable
from pathlib import Path
from typing import Any, TypeIs

import pytest
import yaml
from repo_checks.checks_graph import projects
from repo_checks.expect import contains, equal
from repo_checks.gate_shards import END_TO_END, plan
from repo_checks.model import Repo
from repo_checks.parsing import load_workflow
from test_gate_shards import CI, dispatched_shards
from treecopy import REPO_ROOT

RECORDS = REPO_ROOT / "tools/repo-checks/src/repo_checks/gate_shard_records.toml"
#: The platforms whose splits the record must carry.
RECORDED = ("linux-aarch64", "windows-aarch64")
#: The project the end-to-end tier is the target of.
END_TO_END_PROJECT = "repo-e2e"


def budget() -> int:
    """The `gate-time` budget's threshold, in seconds, from the root `budgets.yaml`."""
    declared = yaml.safe_load((REPO_ROOT / "budgets.yaml").read_text(encoding="utf-8"))
    return next(one["threshold"] for one in declared["budgets"] if one["id"] == "gate-time")


def whole(value: object) -> TypeIs[int]:
    """Whether `value` is a measured figure: a positive whole number of seconds."""
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def findings(record: dict[str, Any], workflow: dict[str, Any], repo: Repo) -> list[str]:
    """Each way `record` fails to describe the split `workflow` dispatches, timed within budget."""
    found: list[str] = []
    tested = {project.name: project for project in projects(repo) if "test" in project.targets}
    for platform in RECORDED:
        entry = record.get(platform)
        if not isinstance(entry, dict):
            found.append(f"{platform}: no record")
            continue
        found.extend(_platform_findings(platform, entry, workflow, repo, tested))
    return found


def _platform_findings(
    platform: str,
    entry: dict[str, Any],
    workflow: dict[str, Any],
    repo: Repo,
    tested: dict[str, Any],
) -> list[str]:
    found: list[str] = []
    count = entry.get("shards")
    dispatched = dispatched_shards(workflow, platform)
    if count != len(dispatched):
        found.append(f"{platform}: records {count} shards, and ci.yml dispatches {len(dispatched)}")
        return found
    shards = entry.get("shard", [])
    if [one.get("shard") for one in shards] != dispatched:
        found.append(
            f"{platform}: records shards {[one.get('shard') for one in shards]}, "
            f"and ci.yml dispatches {dispatched}"
        )
        return found
    overhead = entry.get("overhead_seconds")
    if not whole(overhead):
        found.append(f"{platform}: the gate's fixed overhead {overhead!r} is no measured figure")
    placed: Counter[str] = Counter()
    for shard, steps in zip(shards, plan(repo, count), strict=True):
        where = f"{platform} {shard['shard']}"
        tiers = shard.get("tiers", {})
        if list(tiers) != [step.tier for step in steps]:
            found.append(
                f"{where}: records tiers {list(tiers)}, and the planner hands it "
                f"{[step.tier for step in steps]}"
            )
        found.extend(
            f"{where}: tier `{tier}` has no measured time, {seconds!r}"
            for tier, seconds in tiers.items()
            if not whole(seconds)
        )
        runs = shard.get("projects", {})
        expected = _projects_run(steps, tested)
        if set(runs) != expected:
            found.append(
                f"{where}: records projects {sorted(runs)}, and it runs {sorted(expected)}"
            )
        measured = next((tier for tier in ("test", END_TO_END) if tier in tiers), None)
        for name, seconds in runs.items():
            if not whole(seconds):
                found.append(f"{where}: `{name}` has no measured time, {seconds!r}")
            elif measured and whole(tiers[measured]) and seconds > tiers[measured]:
                found.append(f"{where}: `{name}` took longer than its tier, {measured}")
        if END_TO_END not in tiers:
            placed.update(list(runs))
        found.extend(_total_findings(where, shard, overhead, tiers))
    found.extend(
        f"{platform}: `{name}`'s tests are recorded in {seen} shards"
        for name, seen in placed.items()
        if seen != 1
    )
    found.extend(
        f"{platform}: `{name}`'s tests are recorded in no shard"
        for name in tested.keys() - placed.keys() - {END_TO_END_PROJECT}
    )
    shard_overheads = [one.get("overhead_seconds") for one in shards]
    if (
        whole(overhead)
        and all(whole(one) for one in shard_overheads)
        and max(shard_overheads) != overhead
    ):
        found.append(
            f"{platform}: the gate's fixed overhead {overhead} is not its shards' largest, "
            f"{max(shard_overheads)}"
        )
    return found


def _projects_run(steps: tuple[Any, ...], tested: dict[str, Any]) -> set[str]:
    """The projects whose tests a shard's steps run: a side of the test tier, or the e2e tier's."""
    runs: set[str] = set()
    for step in steps:
        if step.tier == "test":
            runs |= {
                name
                for name, project in tested.items()
                if step.projects is None or step.projects.admits(project.tags)
            }
        elif step.tier == END_TO_END:
            runs.add(END_TO_END_PROJECT)
    return runs


def _total_findings(
    where: str, shard: dict[str, Any], overhead: object, tiers: dict[str, Any]
) -> list[str]:
    own, total = shard.get("overhead_seconds"), shard.get("total_seconds")
    if not (whole(own) and whole(total) and all(whole(one) for one in tiers.values())):
        return [f"{where}: its overhead or total has no measured figure"]
    found: list[str] = []
    if whole(overhead) and own > overhead:
        found.append(f"{where}: its overhead {own} exceeds the gate's, {overhead}")
    if total != own + sum(tiers.values()):
        found.append(
            f"{where}: its total {total} is not its overhead and tiers, {own + sum(tiers.values())}"
        )
    if total >= budget():
        found.append(f"{where}: its total {total} does not fit the {budget()} s `gate-time` budget")
    return found


def committed() -> dict[str, Any]:
    """The committed record."""
    return tomllib.loads(RECORDS.read_text(encoding="utf-8"))


def test_the_committed_record_describes_the_dispatched_split_within_budget() -> None:
    """Every aarch64 shard ci.yml dispatches is recorded with the planner's work and its times."""
    equal(findings(committed(), load_workflow(REPO_ROOT / CI), Repo(REPO_ROOT)), [])


def shard_of(record: dict[str, Any], platform: str, index: int) -> dict[str, Any]:
    """Shard `index` (from 1) of `platform`'s record."""
    return record[platform]["shard"][index - 1]


def moved_project(record: dict[str, Any]) -> None:
    """Move `repo-checks` from Linux's non-Rust shard to its Rust one."""
    seconds = shard_of(record, "linux-aarch64", 2)["projects"].pop("repo-checks")
    shard_of(record, "linux-aarch64", 1)["projects"]["repo-checks"] = seconds


def doubled_project(record: dict[str, Any]) -> None:
    """Record the Windows Rust crate `printobserver-sdk` in the rest shard as well."""
    shard_of(record, "windows-aarch64", 3)["projects"]["printobserver-sdk"] = 54


def placeholder(record: dict[str, Any]) -> None:
    """Record one project's time as a placeholder."""
    shard_of(record, "windows-aarch64", 2)["projects"]["octoprint-env"] = 0


def miscounted(record: dict[str, Any]) -> None:
    """Record Linux as the old unsharded gate."""
    record["linux-aarch64"]["shards"] = 1


def overspent(record: dict[str, Any]) -> None:
    """Record a shard whose tiers outgrow the budget, total included."""
    shard = shard_of(record, "windows-aarch64", 2)
    shard["tiers"]["test"] += 100
    shard["total_seconds"] += 100


def mis_summed(record: dict[str, Any]) -> None:
    """Record a total that is not its overhead plus its tiers."""
    shard_of(record, "linux-aarch64", 3)["total_seconds"] += 1


def tier_moved(record: dict[str, Any]) -> None:
    """Record `build` in a shard the planner does not hand it to."""
    shard = shard_of(record, "linux-aarch64", 1)
    shard_of(record, "linux-aarch64", 2)["tiers"]["build"] = shard["tiers"].pop("build")


@pytest.mark.parametrize(
    ("breaking", "naming"),
    [
        pytest.param(moved_project, "records projects", id="a-project-in-the-wrong-shard"),
        pytest.param(doubled_project, "recorded in 2 shards", id="a-project-in-two-shards"),
        pytest.param(placeholder, "`octoprint-env` has no measured time", id="a-placeholder"),
        pytest.param(miscounted, "records 1 shards, and ci.yml dispatches 5", id="miscounted"),
        pytest.param(overspent, "does not fit the 1800 s `gate-time` budget", id="over-budget"),
        pytest.param(mis_summed, "is not its overhead and tiers", id="a-wrong-total"),
        pytest.param(tier_moved, "the planner hands it", id="a-tier-in-the-wrong-shard"),
    ],
)
def test_a_record_that_misdescribes_the_split_is_refused(
    breaking: Callable[[dict[str, Any]], None], naming: str
) -> None:
    """Each way a record can drift from the planner, the workflow or the budget is named."""
    record = copy.deepcopy(committed())
    breaking(record)

    contains(
        "\n".join(findings(record, load_workflow(REPO_ROOT / CI), Repo(REPO_ROOT))),
        naming,
    )


def test_a_record_missing_a_platform_is_refused() -> None:
    """A split nothing records is not described."""
    record = copy.deepcopy(committed())
    del record["windows-aarch64"]

    contains(
        "\n".join(findings(record, load_workflow(REPO_ROOT / CI), Repo(REPO_ROOT))),
        "windows-aarch64: no record",
    )


def test_a_workflow_that_dispatches_another_split_is_refused(tmp_path: Path) -> None:
    """The record follows ci.yml: a workflow that splits Linux six ways no longer matches it."""
    text = (REPO_ROOT / CI).read_text(encoding="utf-8")
    at = text.index("  gate-shard-linux-aarch64:\n")
    end = text.index("    runs-on:", at)
    broken = tmp_path / "ci.yml"
    broken.write_text(
        text[:at] + text[at:end].replace("shards: 5", "shards: 6") + text[end:], encoding="utf-8"
    )

    contains(
        "\n".join(findings(committed(), load_workflow(broken), Repo(REPO_ROOT))),
        "linux-aarch64: records shards",
    )
