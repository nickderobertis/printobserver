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

import pytest
import yaml
from lfs_set import CI_MISSES, GIB, HOST_FETCHES, MB
from lfs_world import HERE, REPO_ROOT
from repo_checks.expect import equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.shell import run

FULL_SET = HERE / "fixtures" / "full-set.tsv"
BUDGETS = HERE / "budgets.yaml"


def _rows() -> list[tuple[str, str, int]]:
    """The full set's files, as `(path, oid, size)`."""
    rows: list[tuple[str, str, int]] = []
    for line in FULL_SET.read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            path, oid, size = line.split("\t")
            rows.append((path, oid, int(size)))
    return rows


@pytest.fixture(scope="module")
def full_set(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A repository whose `HEAD` holds every pointer of the full set, and nothing else."""
    root = init_repository(tmp_path_factory.mktemp("full-set") / "repository")
    shutil.copy2(REPO_ROOT / ".gitattributes", root / ".gitattributes")
    for path, oid, size in _rows():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(
            f"version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n".encode()
        )
    identity = ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"]
    run(["git", "add", "-A"], cwd=root, check=True)
    run(["git", *identity, "commit", "-q", "-m", "the full set"], cwd=root, check=True)
    return root


def _measure(budget: str, repository: Path, result: Path) -> tuple[float, dict[str, object]]:
    """Run one budget's registered command over `repository`, answering its figure and detail.

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
    run(argv, cwd=repository, env=environment, check=True)
    written = json.loads(result.read_text(encoding="utf-8"))
    return written["value"], json.loads(written["detail"])


def test_the_full_set_is_the_one_the_workloads_name() -> None:
    """527 files: the existing cases' 77 and the exported cases' 450."""
    rows = _rows()
    equal(len(rows), 527, describing="files in the full set")
    exported = [row for row in rows if "/exported-case-" in row[0]]
    equal(len(exported), 450, describing="files of the exported cases")
    equal(
        sum({oid: size for _, oid, size in exported}.values()),
        132_117_714,
        describing="the exported cases' distinct bytes, as the export measured them",
    )


def test_the_cache_miss_command_reports_the_sets_distinct_bytes(
    full_set: Path, tmp_path: Path
) -> None:
    """Every distinct object once, in megabytes: identical files are one download."""
    distinct = {oid: size for _, oid, size in _rows()}
    value, detail = _measure("lfs-bytes-per-ci-run-cache-miss", full_set, tmp_path / "result")
    equal(detail["bytes"], sum(distinct.values()), describing="the set's distinct bytes")
    equal(detail["objects"], len(distinct), describing="the set's distinct objects")
    equal(detail["files"], 527, describing="the files the pointers name")
    truth(
        abs(value - sum(distinct.values()) / MB) < 1e-9, describing=f"the figure in MB, not {value}"
    )


def test_the_monthly_command_reports_twenty_full_fetches_in_gib(
    full_set: Path, tmp_path: Path
) -> None:
    """The set's size times the landing month's 20 full fetches, in GiB."""
    total = sum({oid: size for _, oid, size in _rows()}.values())
    value, detail = _measure("lfs-bandwidth-gib-per-month", full_set, tmp_path / "result")
    equal(CI_MISSES + HOST_FETCHES, 20, describing="full fetches in the landing month")
    equal(detail["set_bytes"], total, describing="the set's size")
    truth(abs(value - total * 20 / GIB) < 1e-9, describing=f"the figure in GiB, not {value}")
