"""A fixture repository starts no maintenance of git's own.

`GateCopy` commits the tracked tree into a fresh repository, and every `git
commit` spawns `git maintenance run --auto` — which on a git past 2.54 repacks
the copy detached, under whatever a journey then walks `.git` with. The copy's
own configuration turns that off, for the reason stated beside the setting in
`journey.py`, and this journey drives the real fixture under git's trace to
hold it there: the spawn is visible on every git version, whether or not the
repack it would start ever fires.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import GateCopy, run
from repo_checks.expect import contains, equal, passing, truth
from test_release_path_journey import OCCUPIED, tagged

#: The two git commands that run maintenance on their own initiative: the
#: `maintenance` command every commit spawns, and the `gc --auto` an older git
#: spawned in its place. Neither may appear among the fixture's processes.
MAINTENANCE_COMMANDS = frozenset({"maintenance", "gc"})

#: The two settings a copy carries, as `git config --local --list` spells them:
#: `maintenance.auto` is what stops the spawn on every git since 2.29, and
#: `gc.auto=0` is what stops the `gc` task on one that ran it directly.
QUIESCENT = ("maintenance.auto=false", "gc.auto=0")


#: The two Trace2 events that record a process starting, each carrying its
#: `argv`: a git process's own, and one for each child a git process spawned.
STARTS = frozenset({"start", "child_start"})


def _processes(trace: Path) -> list[list[str]]:
    """Every process git's trace saw start: each git process, and each child one spawned.

    One JSON object per line, whose `event` names the record's kind; a line of
    another shape, or a start carrying no `argv` list, is refused naming the
    line — a trace whose format moved must not read as a fixture that started
    nothing.
    """
    processes: list[list[str]] = []
    for number, line in enumerate(trace.read_text(encoding="utf-8").splitlines(), start=1):
        event = json.loads(line)
        truth(
            isinstance(event, dict) and isinstance(event.get("event"), str),
            describing=f"{trace.name}:{number} to be a Trace2 event record: {line!r}",
        )
        if event["event"] not in STARTS:
            continue
        argv = event.get("argv")
        truth(
            isinstance(argv, list) and all(isinstance(word, str) for word in argv),
            describing=f"{trace.name}:{number} to carry the started process's argv: {line!r}",
        )
        processes.append(argv)
    return processes


def _git_subcommand(argv: list[str]) -> str:
    """The subcommand a git process ran, past the `-c key=value` pairs before it."""
    words = argv[1:]
    while words[:1] == ["-c"]:
        words = words[2:]
    return words[0] if words else ""


def test_a_gate_copy_starts_no_maintenance_of_gits_own(
    gate_copy: Callable[..., GateCopy], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Under git's trace, the fixture's commit and tag spawn nothing that repacks.

    `GIT_TRACE2_EVENT` names a file every git process appends its events to,
    including the `child_start` a `commit` records as it spawns maintenance,
    so what the fixture's own commands started is read off git rather than
    inferred from what was left loose afterwards.
    """
    trace = tmp_path / "git-trace2.jsonl"
    monkeypatch.setenv("GIT_TRACE2_EVENT", str(trace))

    copy = tagged(gate_copy, OCCUPIED)

    processes = _processes(trace)
    subcommands = [_git_subcommand(argv) for argv in processes]
    # The trace was on for the whole of the fixture: an empty one would prove
    # nothing, so the commit and the tag it makes are required to be in it.
    contains(subcommands, "commit", describing="the git processes the fixture started")
    contains(subcommands, "tag", describing="the git processes the fixture started")
    spawned = [argv for argv in processes if _git_subcommand(argv) in MAINTENANCE_COMMANDS]
    equal(spawned, [], describing="the maintenance processes the fixture's git commands spawned")

    settings = run(["git", "config", "--local", "--list"], cwd=copy.root)
    passing(settings, describing="reading the copy's own repository configuration")
    for setting in QUIESCENT:
        contains(settings.stdout.splitlines(), setting, describing="the copy's configuration")
    # And the objects the commit wrote are still where it wrote them: nothing
    # packed them behind the journey's back.
    counted = run(["git", "count-objects", "-v"], cwd=copy.root)
    passing(counted, describing="counting the copy's objects")
    truth(
        "packs: 0" in counted.stdout.splitlines(),
        describing=f"the copy to carry no pack:\n{counted.stdout}",
    )
