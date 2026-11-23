"""A draft held to `bounded`'s bounds is stopped, whole, when it hangs or runs away.

The release-path journeys hold their drafts against a release to CPU time and
idle time rather than to a deadline, and a bound that never fires would pass
every one of them as surely as a correct one. So each bound is made to fire
here, over stand-in drafting tools, each of which starts a child of its own: one
that says a line and then waits with its child, both doing nothing, and one
that keeps talking while its child spins. Each child beats into a file, and a
tree that was stopped is one whose child never beats again.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

import pytest
from bounded import Bounds, DraftStoppedError, bounded
from journey import clean_environment
from repo_checks.expect import contains, equal, truth

#: A child that waits, doing nothing, for `WAKES_AFTER_SECONDS`, and then beats
#: into the file it is handed every tenth of a second.
WAKING_CHILD = """
import pathlib, sys, time
time.sleep(float(sys.argv[2]))
beats = pathlib.Path(sys.argv[1])
while True:
    with beats.open("a") as handle:
        handle.write(".")
    time.sleep(0.1)
"""

#: A child that spends CPU without pause, beating into the file it is handed.
SPINNING_CHILD = """
import pathlib, sys, time
beats = pathlib.Path(sys.argv[1])
last = 0.0
while True:
    if time.monotonic() - last > 0.1:
        with beats.open("a") as handle:
            handle.write(".")
        last = time.monotonic()
"""

#: A drafting tool that starts `child`, says one line, and then does what
#: `behaviour` says: `exit` ends at once, `wait` waits forever saying nothing,
#: and `talk` waits forever saying a line every fifth of a second.
STAND_IN = """
import subprocess, sys, time
child, beats, wakes, behaviour = sys.argv[1:5]
subprocess.Popen([sys.executable, child, beats, wakes])
print("determining next version for printobserver-core", flush=True)
if behaviour == "exit":
    print("the draft is done", flush=True)
    sys.exit(3)
if behaviour == "wait":
    time.sleep(3600)
while True:
    time.sleep(0.2)
    print("still comparing printobserver-core", flush=True)
"""

#: How long the waiting child does nothing before it beats: past the idle
#: bound below, so a tree stopped at that bound never beats at all.
WAKES_AFTER_SECONDS = 8.0

#: Bounds small enough to fire within seconds, each far from the other's.
QUICK_IDLE = Bounds(cpu_seconds=600, idle_seconds=3)
QUICK_CPU = Bounds(cpu_seconds=2, idle_seconds=600)


def stand_in(tmp_path: Path, child: str, behaviour: str) -> tuple[list[str], Path]:
    """The command running the stand-in, and the file its child beats into."""
    (tmp_path / "child.py").write_text(child, encoding="utf-8")
    (tmp_path / "stand_in.py").write_text(STAND_IN, encoding="utf-8")
    beats = tmp_path / "beats"
    command = [
        sys.executable,
        str(tmp_path / "stand_in.py"),
        str(tmp_path / "child.py"),
        str(beats),
        str(WAKES_AFTER_SECONDS),
        behaviour,
    ]
    return command, beats


def beat_count(beats: Path) -> int:
    """How many times the child has beaten."""
    return beats.stat().st_size if beats.exists() else 0


def test_a_draft_that_ends_is_answered_with_its_exit_and_what_it_said(tmp_path: Path) -> None:
    """A stand-in that ends inside its bounds comes back as it ended, and alone.

    Its child is still running when it exits, holding its output streams: the
    draft is answered all the same, and the child is stopped with it rather
    than left to beat once it wakes.
    """
    command, beats = stand_in(tmp_path, WAKING_CHILD, "exit")
    started = time.monotonic()

    drafted = bounded(
        command, tmp_path, bounds=QUICK_IDLE, env=clean_environment(), describing="a draft"
    )

    equal(drafted.returncode, 3, describing="the stand-in's own exit status")
    contains(drafted.stdout, "the draft is done", describing="what the stand-in said")
    truth(
        time.monotonic() - started < WAKES_AFTER_SECONDS,
        describing="the draft to have been answered before its child woke",
    )
    time.sleep(WAKES_AFTER_SECONDS + 2 - (time.monotonic() - started))
    equal(beat_count(beats), 0, describing="beats from a child the ended draft left")


def test_a_draft_whose_tree_does_nothing_is_stopped_whole_and_reported(tmp_path: Path) -> None:
    """A stand-in that says a line and then waits, its child too, hits the idle bound."""
    command, beats = stand_in(tmp_path, WAKING_CHILD, "wait")
    started = time.monotonic()

    with pytest.raises(DraftStoppedError) as stopped:
        bounded(
            command, tmp_path, bounds=QUICK_IDLE, env=clean_environment(), describing="a hung draft"
        )

    report = str(stopped.value)
    contains(report, "a hung draft was stopped", describing="the report")
    contains(report, "its process tree did nothing for", describing="the report naming the bound")
    contains(report, "over its bound of 3 s", describing="the report naming the bound")
    contains(report, "determining next version for", describing="what it had said")
    truth(
        time.monotonic() - started < WAKES_AFTER_SECONDS,
        describing="the idle bound to have stopped it before its child woke",
    )
    time.sleep(WAKES_AFTER_SECONDS + 2 - (time.monotonic() - started))
    equal(beat_count(beats), 0, describing="beats from a child of a stopped draft")


def test_a_draft_whose_tree_runs_away_is_stopped_whole_and_reported(tmp_path: Path) -> None:
    """A stand-in that keeps talking while its child spins hits the CPU bound."""
    command, beats = stand_in(tmp_path, SPINNING_CHILD, "talk")

    with pytest.raises(DraftStoppedError) as stopped:
        bounded(
            command,
            tmp_path,
            bounds=QUICK_CPU,
            env=clean_environment(),
            describing="a runaway draft",
        )

    report = str(stopped.value)
    contains(report, "a runaway draft was stopped", describing="the report")
    contains(report, "s of CPU, over its bound of 2 s", describing="the report naming the bound")
    contains(report, "still comparing printobserver-core", describing="what it had said")
    after_stop = beat_count(beats)
    time.sleep(1.5)
    equal(beat_count(beats), after_stop, describing="beats from a child of a stopped draft")
