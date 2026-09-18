#!/usr/bin/env python3
"""A stand-in for the installed program that stops answering, on demand.

Everything the smoke does it does by running one command and reading what came
back, and the failure this exists to produce is the one where nothing comes
back at all: a command that never answers inside its bound. That is not
something a controlled machine can be scripted into — it answers or it does
not — so it is produced here, in front of the real program, by not answering.

Every invocation is passed straight through to the program `SMOKE_RELAY_PROGRAM`
names, except the ones the environment below selects, which sleep until the
caller's own bound stops them:

* `SMOKE_RELAY_HANG_ON` — the client command to stop answering on.
* `SMOKE_RELAY_AFTER` — how many of that command to answer normally first.
* `SMOKE_RELAY_ARMED_BY` — a command that arms the hang, so that a run can be
  driven all the way through and only then meet a machine it cannot read.
* `SMOKE_RELAY_STATE` — the file this counts in, one run's own.
"""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

from repo_checks.shell import run

PROGRAM = "SMOKE_RELAY_PROGRAM"
HANG_ON = "SMOKE_RELAY_HANG_ON"
AFTER = "SMOKE_RELAY_AFTER"
ARMED_BY = "SMOKE_RELAY_ARMED_BY"
STATE = "SMOKE_RELAY_STATE"

#: Long enough that the bound the caller set is always what ends the wait.
FOREVER_S = 3600.0


def remember(state_file: Path, *, armed: bool, seen: int) -> None:
    """Write the state so that it is either the previous one or this one, never neither.

    The smoke stops a command that has not answered inside its bound by killing
    it, and a kill lands wherever the relay happens to be — on a loaded host,
    still starting up. A write that truncated the file in place and was killed
    before it filled it again would leave an empty file that no later
    invocation, and no test, could read. So the new state is written beside the
    file and moved over it, which every platform does as one step.

    Args:
        state_file: The file this relay counts in.
        armed: Whether the hang is armed.
        seen: How many of the hung command have been seen.
    """
    staging = state_file.with_name(state_file.name + ".next")
    staging.write_text(json.dumps({"armed": armed, "seen": seen}), encoding="utf-8")
    staging.replace(state_file)


def main(argv: list[str]) -> int:
    """Answer as the program does, or not at all.

    Args:
        argv: The arguments the smoke ran the program with.

    Returns:
        Whatever the program itself exited with, once it has been let run.
    """
    hang_on = os.environ.get(HANG_ON, "")
    armed_by = os.environ.get(ARMED_BY, "")
    selected = {name for name in (hang_on, armed_by) if name}
    command = next((argument for argument in argv if argument in selected), "")
    state_file = Path(os.environ[STATE])
    state = json.loads(state_file.read_text(encoding="utf-8")) if state_file.is_file() else {}
    armed = bool(state.get("armed", not armed_by))
    seen = int(state.get("seen", 0))

    if command == hang_on and armed:
        seen += 1
        remember(state_file, armed=armed, seen=seen)
        if seen > int(os.environ.get(AFTER, "0")):
            time.sleep(FOREVER_S)
    elif command and command == armed_by:
        remember(state_file, armed=True, seen=seen)

    # Not captured: what the program writes is what the smoke reads, so it goes
    # to this process's own streams rather than through a buffer of its own.
    return run([os.environ[PROGRAM], *argv], capture=False).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
