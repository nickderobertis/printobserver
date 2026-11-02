"""The two pointer-read budgets' commands, over the full set their workloads name.

`fixtures/full-set.tsv` is the whole real-print set as pointers — the existing
cases' 77 and the exported cases' 450 — and this commits exactly those pointers
to a fixture repository carrying the tree's `.gitattributes`, then runs each
budget's own command from `budgets.yaml` over it, pointed at that repository,
the way `onebudgetspec check` runs one: with `ONEBUDGETSPEC_RESULT` naming the
file the figure is written to. Nothing is downloaded, because nothing there is
anything but a pointer. Each figure is shown to be the budgets' arithmetic over
the set; no bound is asserted, because that is the budgets' check.
"""

from __future__ import annotations

import json
import os
import shutil
from pathlib import Path
from subprocess import CompletedProcess

import pytest
import yaml
from lfs_set import (
    CI_MISSES,
    GIB,
    HOST_FETCHES,
    MB,
    TYPICAL_CI_MISSES,
    TYPICAL_HOST_FETCHES,
    Oid,
    Tracked,
)
from lfs_world import HERE, REPO_ROOT
from repo_checks.expect import contains, equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.shell import run

FULL_SET = HERE / "fixtures" / "full-set.tsv"
BUDGETS = HERE / "budgets.yaml"


def _rows() -> list[Tracked]:
    """The full set's files, each with the object its pointer names."""
    rows: list[Tracked] = []
    for line in FULL_SET.read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            path, oid, size = line.split("\t")
            rows.append(Tracked(path, Oid(oid), int(size)))
    return rows


def _distinct(rows: list[Tracked]) -> dict[Oid, int]:
    """Each object of `rows` once, with its size."""
    return {row.oid: row.size for row in rows}


@pytest.fixture(scope="module")
def full_set(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A repository whose `HEAD` holds every pointer of the full set, and nothing else."""
    root = init_repository(tmp_path_factory.mktemp("full-set") / "repository")
    shutil.copy2(REPO_ROOT / ".gitattributes", root / ".gitattributes")
    for row in _rows():
        target = root / row.path
        target.parent.mkdir(parents=True, exist_ok=True)
        pointer = f"oid sha256:{row.oid}\nsize {row.size}\n"
        target.write_bytes(f"version https://git-lfs.github.com/spec/v1\n{pointer}".encode())
    identity = ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"]
    run(["git", "add", "-A"], cwd=root, check=True)
    run(["git", *identity, "commit", "-q", "-m", "the full set"], cwd=root, check=True)
    return root


def _command(budget: str, repository: Path, result: Path) -> CompletedProcess[str]:
    """Run one budget's registered command over `repository`, as a check runs it.

    The argv is the budget's own, from `budgets.yaml`, with its `--directory`
    pointed at the fixture rather than at this tree, the project this tree's
    environment is in named beside it, and its script named where it is.
    """
    document = yaml.safe_load(BUDGETS.read_text(encoding="utf-8"))
    command = next(entry["command"] for entry in document["budgets"] if entry["id"] == budget)
    directory = command.index("--directory")
    argv = [
        *command[:directory],
        "--directory",
        str(repository),
        "--project",
        str(REPO_ROOT),
        *(
            str(REPO_ROOT / word) if word.endswith(".py") else word
            for word in command[directory + 2 :]
        ),
    ]
    environment = {
        **os.environ,
        "ONEBUDGETSPEC_RESULT": str(result),
        "PYTHONPATH": str(REPO_ROOT / "tools" / "repo-checks" / "src"),
    }
    return run(argv, cwd=repository, env=environment)


def _measure(budget: str, repository: Path, result: Path) -> tuple[float, dict[str, object]]:
    """One budget's figure over `repository`, and the detail reported beside it."""
    answer = _command(budget, repository, result)
    equal(answer.returncode, 0, describing=f"{budget}'s command, having said:\n{answer.stderr}")
    written = json.loads(result.read_text(encoding="utf-8"))
    return written["value"], json.loads(written["detail"])


def test_the_full_set_is_the_one_the_workloads_name() -> None:
    """527 files: the existing cases' 77 and the exported cases' 450."""
    rows = _rows()
    equal(len(rows), 527, describing="files in the full set")
    exported = [row for row in rows if row.case.startswith("exported-case-")]
    equal(len(exported), 450, describing="files of the exported cases")
    equal(
        sum(_distinct(exported).values()),
        132_117_714,
        describing="the exported cases' distinct bytes, as the export measured them",
    )


def test_the_cache_miss_command_reports_the_sets_distinct_bytes(
    full_set: Path, tmp_path: Path
) -> None:
    """Every distinct object once, in megabytes: identical files are one download."""
    distinct = _distinct(_rows())
    value, detail = _measure("lfs-bytes-per-ci-run-cache-miss", full_set, tmp_path / "result")
    equal(detail["bytes"], sum(distinct.values()), describing="the set's distinct bytes")
    equal(detail["objects"], len(distinct), describing="the set's distinct objects")
    equal(detail["files"], 527, describing="the files the pointers name")
    per_case: dict[str, dict[Oid, int]] = {}
    for row in _rows():
        per_case.setdefault(row.case, {})[row.oid] = row.size
    equal(
        detail["bytes_per_case"],
        {case: sum(objects.values()) for case, objects in sorted(per_case.items())},
        describing="each case's own distinct bytes",
    )
    shared = {oid for oid in distinct if sum(oid in objects for objects in per_case.values()) > 1}
    truth(shared, describing="an object two cases share, which each case's bytes count")
    truth(
        abs(value - sum(distinct.values()) / MB) < 1e-9, describing=f"the figure in MB, not {value}"
    )


def test_the_monthly_command_reports_twenty_full_fetches_in_gib(
    full_set: Path, tmp_path: Path
) -> None:
    """The set's size times the landing month's 20 full fetches, in GiB."""
    total = sum(_distinct(_rows()).values())
    value, detail = _measure("lfs-bandwidth-gib-per-month", full_set, tmp_path / "result")
    equal(CI_MISSES + HOST_FETCHES, 20, describing="full fetches in the landing month")
    equal(detail["set_bytes"], total, describing="the set's size")
    equal(
        detail["typical_month"],
        {"ci_cache_misses": TYPICAL_CI_MISSES, "host_fetches": TYPICAL_HOST_FETCHES},
        describing="a typical month's fetches",
    )
    equal((TYPICAL_CI_MISSES, TYPICAL_HOST_FETCHES), (4, 2), describing="a typical month")
    typical = detail["typical_month_gib"]
    truth(
        isinstance(typical, float) and abs(typical - total * 6 / GIB) < 1e-9,
        describing=f"a typical month's six fetches in GiB, not {typical}",
    )
    truth(abs(value - total * 20 / GIB) < 1e-9, describing=f"the figure in GiB, not {value}")


def test_a_repository_holding_no_case_pointer_is_refused_rather_than_measured_as_zero(
    tmp_path: Path,
) -> None:
    """With nothing to read, the command fails naming why, and reports no figure."""
    empty = init_repository(tmp_path / "empty")
    (empty / "README.md").write_text("no cases\n", encoding="utf-8")
    identity = ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"]
    run(["git", "add", "-A"], cwd=empty, check=True)
    run(["git", *identity, "commit", "-q", "-m", "nothing"], cwd=empty, check=True)
    result = tmp_path / "result"
    answer = _command("lfs-bytes-per-ci-run-cache-miss", empty, result)
    equal(answer.returncode, 1, describing="the command's exit over no cases")
    equal(result.exists(), False, describing="a figure written for no cases")
    contains(answer.stderr, "git-lfs lists no file under tests/real-prints/ at HEAD")
