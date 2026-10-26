"""A fixture repository never starts maintenance under the test reading it.

Git's automatic maintenance is decided by counting loose objects, and it counts
them by sampling one fanout directory, `objects/17`, and multiplying by 256. So
a repository holding forty loose objects whose ids all begin `17` is, to git, a
repository holding over ten thousand — above the threshold of every strategy
git has shipped — and the next commit asks for a repack. These journeys put a
repository in that state and commit in it, once as the helper makes it and once
with its two settings taken back out, which is the control that shows the state
is one git does repack.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
from pathlib import Path

import pytest
from repo_checks.expect import equal, truth
from repo_checks.fixture_repository import QUIESCENT, init_repository
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: The suites whose fixture repositories are made by the helper, and nothing else.
SUITES = ("tools/repo-checks/tests", "tools/release-artifacts/tests", "tests/repo-e2e/tests")

#: What making a repository looks like in a suite's source: `init` as an
#: argument of its own, the command spelled whole, or the long flag naming the
#: first branch. Written so that this file's own source does not match it.
INITIALIZING = re.compile(r"""(["'])init\1|\bgit[ ]init\b|initial[-]branch""")

#: The fanout directory git samples, and how many loose objects to put in it.
SAMPLED = "17"
LOOSE = 40

#: The commands that maintain a repository, any of which a commit could start.
MAINTAINING = frozenset({"maintenance", "gc", "repack"})

#: What keeps whatever maintenance does start in the foreground, so that it is
#: finished — and its pack on disk — by the time the commit returns.
FOREGROUND = ("-c", "gc.autoDetach=false", "-c", "maintenance.autoDetach=false")


def _loose_in_sampled(root: Path, scratch: Path) -> None:
    """Write `LOOSE` loose blobs into `root`, every one under `objects/17`."""
    scratch.mkdir()
    paths: list[str] = []
    number = 0
    while len(paths) < LOOSE:
        body = f"{number}\n".encode()
        number += 1
        # Git's own object id for a blob, which is what names its fanout directory.
        object_id = hashlib.sha1(b"blob %d\0" % len(body) + body, usedforsecurity=False)
        if object_id.hexdigest().startswith(SAMPLED):
            path = scratch / f"{number}.txt"
            path.write_bytes(body)
            paths.append(str(path))
    run(
        ["git", "hash-object", "-w", "--stdin-paths"],
        cwd=root,
        stdin="\n".join(paths) + "\n",
        check=True,
    )
    equal(len(list((root / ".git/objects" / SAMPLED).iterdir())), LOOSE, describing="loose")


def _commit_traced(root: Path, trace: Path) -> list[list[str]]:
    """Commit in `root` as a test would, and answer every child git started."""
    run(
        [
            "git",
            *FOREGROUND,
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "chore: a commit after the loose objects",
        ],
        cwd=root,
        env={**os.environ, "GIT_TRACE2_EVENT": str(trace)},
        check=True,
    )
    events = [json.loads(line) for line in trace.read_text(encoding="utf-8").splitlines()]
    return [event["argv"] for event in events if event.get("event") == "child_start"]


def _packs(root: Path) -> list[Path]:
    """Every pack the repository holds."""
    return sorted((root / ".git/objects/pack").glob("*.pack"))


def _maintaining(children: list[list[str]]) -> list[list[str]]:
    """The children that maintain the repository."""
    return [argv for argv in children if MAINTAINING.intersection(argv)]


def test_a_fixture_repository_carries_maintenance_switched_off(tmp_path: Path) -> None:
    """Both settings are the repository's own, so no host configuration is assumed."""
    root = init_repository(tmp_path / "fixture", branch="base")

    for name, value in QUIESCENT:
        read = run(["git", "config", "--local", "--get", name], cwd=root, check=True)
        equal(read.stdout.strip(), value, describing=f"the fixture's own `{name}`")
    branch = run(["git", "symbolic-ref", "--short", "HEAD"], cwd=root, check=True)
    equal(branch.stdout.strip(), "base", describing="the branch its first commit lands on")


def test_loose_objects_above_the_threshold_start_no_maintenance_in_a_fixture(
    tmp_path: Path,
) -> None:
    """The commit that would repack any other repository starts nothing in this one."""
    root = init_repository(tmp_path / "fixture")
    _loose_in_sampled(root, tmp_path / "blobs")

    children = _commit_traced(root, tmp_path / "trace.json")

    equal(_maintaining(children), [], describing="the maintenance the commit started")
    equal(_packs(root), [], describing="the packs written")
    equal(
        len(list((root / ".git/objects" / SAMPLED).iterdir())),
        LOOSE,
        describing="the loose objects after the commit",
    )


def test_the_same_repository_without_the_settings_is_repacked(tmp_path: Path) -> None:
    """The control: the state above is one git does repack, so the journey proves something."""
    root = init_repository(tmp_path / "control")
    for name, _ in QUIESCENT:
        run(["git", "config", "--local", "--unset", name], cwd=root, check=True)
    _loose_in_sampled(root, tmp_path / "blobs")

    children = _commit_traced(root, tmp_path / "trace.json")

    truth(_maintaining(children), describing=f"maintenance started by the commit: {children}")
    truth(_packs(root), describing="a pack written by the maintenance the commit started")


@pytest.mark.parametrize("suite", SUITES)
def test_every_fixture_repository_in_a_suite_is_made_by_the_helper(suite: str) -> None:
    """A repository made any other way is one maintenance may run in."""
    found = [
        f"{path.relative_to(REPO_ROOT)}:{number}: {line.strip()}"
        for path in sorted((REPO_ROOT / suite).rglob("*.py"))
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1)
        if INITIALIZING.search(line)
    ]
    equal(found, [], describing=f"repositories {suite} makes without `init_repository`")
