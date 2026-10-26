"""A gate copy starts no git maintenance while a journey reads its `.git`.

`GateCopy` commits the whole tracked tree, several hundred loose objects, and a
commit ends by asking whether the repository wants maintenance. Since git 2.54
that answer is a detached repack, which deletes the loose objects and their
emptied `objects/XX` directories while `release-plz update` is still copying the
same `.git` aside. The copy is made through `init_repository`, so it carries
maintenance switched off before its first commit; this journey traces every
process git starts while the real fixture is made and committed in again.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import COMMIT_IDENTITY, GateCopy
from repo_checks.expect import equal
from repo_checks.fixture_repository import QUIESCENT
from repo_checks.shell import run

#: The commands that maintain a repository, any of which a commit could start.
MAINTAINING = frozenset({"maintenance", "gc", "repack"})


def test_a_gate_copy_is_made_and_committed_in_without_starting_maintenance(
    gate_copy: Callable[..., GateCopy], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """No git the fixture or a journey's later commit runs starts a maintenance process."""
    trace = tmp_path / "trace.json"
    monkeypatch.setenv("GIT_TRACE2_EVENT", str(trace))

    copy = gate_copy()
    copy.write("journey.txt", "a journey's own change\n")
    run(["git", "add", "-A"], cwd=copy.root, check=True)
    run(
        ["git", *COMMIT_IDENTITY, "commit", "-q", "-m", "test: a change"], cwd=copy.root, check=True
    )

    events = [json.loads(line) for line in trace.read_text(encoding="utf-8").splitlines()]
    commits = [e for e in events if e.get("event") == "start" and "commit" in e["argv"]]
    equal(len(commits), 2, describing="the commits the trace recorded: the fixture's and ours")
    started = [event["argv"] for event in events if event.get("event") == "child_start"]
    equal(
        [argv for argv in started if MAINTAINING.intersection(argv)],
        [],
        describing="the maintenance git started while the copy was made and committed in",
    )
    for name, value in QUIESCENT:
        read = run(["git", "config", "--local", "--get", name], cwd=copy.root, check=True)
        equal(read.stdout.strip(), value, describing=f"the copy's own `{name}`")
