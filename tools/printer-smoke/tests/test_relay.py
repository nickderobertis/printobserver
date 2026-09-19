"""The relay's own guarantee: its count survives the kill that stops it.

`relay.py` stands in front of the program to stop answering on demand, and
the smoke stops a command that has not answered by killing it — wherever the
relay happens to be at that instant, which on a loaded host is still starting
up. What the tests in `test_cleanup.py` read afterwards is the count that
relay left, so the count has to be there whatever the kill interrupted.

A kill cannot be landed inside a write on purpose, so what is driven here is
what one leaves behind: the state file as the last completed write left it,
and beside it the staging file an interrupted write got as far as. The relay
is run as the committed script, the way the smoke runs it, in front of a
program that answers with the arguments it was given.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

from relay import AFTER, ARMED_BY, HANG_ON, PROGRAM, STAGING_SUFFIX, STATE, RelayState
from repo_checks.expect import absent, contains, equal, failing, passing
from repo_checks.shell import run
from world import RELAY, REPO_ROOT

#: The command the relay is told to count. It is never given reason to hang.
COUNTED = "set-feedrate-factor"

#: What the program answers: the arguments the relay ran it with.
ECHO = "import json, sys; print(json.dumps(sys.argv[1:]))"


def relay(state_file: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    """Run the committed relay once, as the smoke runs it, counting in `state_file`.

    Args:
        state_file: The file it counts in.
        *arguments: What the smoke would have run the program with.

    Returns:
        The completed run: the relay's exit, and what the program printed.
    """
    return run(
        [sys.executable, str(RELAY), "-c", ECHO, *arguments],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "PYTHONPATH": str(REPO_ROOT / "tools" / "repo-checks" / "src"),
            PROGRAM: sys.executable,
            HANG_ON: COUNTED,
            AFTER: "99",
            ARMED_BY: "",
            STATE: str(state_file),
        },
        timeout=60,
    )


def test_a_write_interrupted_before_its_move_costs_the_count_nothing(tmp_path: Path) -> None:
    """The state file holds the last completed count, whatever a kill left beside it.

    A kill inside a write lands after the staging file was opened — empty, or
    part-written — and before it was moved over the state file. The state file
    still holds the previous count, so the next invocation counts on from it
    and its own write replaces the leftover.
    """
    state_file = tmp_path / "relay-state.json"
    RelayState(armed=True, seen=3).write(state_file)
    leftover = state_file.with_name(state_file.name + STAGING_SUFFIX)
    leftover.write_text("", encoding="utf-8")

    answered = relay(state_file, COUNTED, "--json")

    passing(answered, describing="the relay, over a leftover of an interrupted write")
    equal(
        RelayState.read(state_file, armed=True),
        RelayState(armed=True, seen=4),
        describing="the count, carried on from the last completed write",
    )
    equal(leftover.exists(), False, describing="whether the leftover survived the next write")
    # Compared as the text it printed rather than decoded: an answer that is
    # not JSON is then named by the comparison instead of raised by the decoder.
    equal(
        answered.stdout.strip(),
        json.dumps([COUNTED, "--json"]),
        describing="what the program was run with, which the relay passes through whole",
    )


def test_a_command_the_relay_does_not_count_writes_nothing(tmp_path: Path) -> None:
    """An uncounted command is passed through and leaves the count as it was."""
    state_file = tmp_path / "relay-state.json"
    RelayState(armed=True, seen=3).write(state_file)
    written_at = state_file.stat().st_mtime_ns

    answered = relay(state_file, "status", "--json")

    passing(answered, describing="the relay, over a command it does not count")
    equal(state_file.stat().st_mtime_ns, written_at, describing="when the state was last written")
    equal(
        RelayState.read(state_file, armed=True),
        RelayState(armed=True, seen=3),
        describing="the count",
    )


def test_a_state_the_relay_did_not_write_stops_it_before_the_program(tmp_path: Path) -> None:
    """A file holding anything but a relay state is a defect to stop on, not to count from.

    Nothing but the relay writes its state file, so what is not a state there
    is not a count to carry on from; the relay refuses naming the file and what
    it held, and the program is never run.
    """
    state_file = tmp_path / "relay-state.json"
    state_file.write_text(json.dumps({"armed": "yes", "seen": 3}), encoding="utf-8")

    answered = relay(state_file, COUNTED, "--json")

    failing(answered, naming=f"{state_file} holds")
    contains(answered.stderr, "rather than a relay state", describing="what it said")
    absent(answered.stdout, COUNTED, describing="what the program printed, which never ran")
