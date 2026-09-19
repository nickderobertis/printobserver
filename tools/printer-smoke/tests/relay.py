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
from dataclasses import dataclass
from pathlib import Path

from repo_checks.shell import run

PROGRAM = "SMOKE_RELAY_PROGRAM"
HANG_ON = "SMOKE_RELAY_HANG_ON"
AFTER = "SMOKE_RELAY_AFTER"
ARMED_BY = "SMOKE_RELAY_ARMED_BY"
STATE = "SMOKE_RELAY_STATE"

#: Long enough that the bound the caller set is always what ends the wait.
FOREVER_S = 3600.0

#: Where a state is written before it is moved over the file: beside it, so
#: that the move is within one directory and the platform does it as one step.
STAGING_SUFFIX = ".next"


@dataclass(frozen=True)
class RelayState:
    """What the relay has counted: whether the hang is armed, and what it has seen."""

    armed: bool
    seen: int

    @classmethod
    def of(cls, document: object) -> RelayState | None:
        """The state a document is, or `None` where it is not one this wrote.

        Args:
            document: What a state file decoded to.
        """
        if not isinstance(document, dict):
            return None
        armed, seen = document.get("armed"), document.get("seen")
        if not isinstance(armed, bool) or isinstance(seen, bool) or not isinstance(seen, int):
            return None
        return cls(armed=armed, seen=seen)

    @classmethod
    def read(cls, state_file: Path, *, armed: bool) -> RelayState:
        """The state the file holds, or the starting one where there is none yet.

        Args:
            state_file: The file this relay counts in.
            armed: Whether the hang starts armed, for a run that has not
                written yet.

        Raises:
            ValueError: If the file holds anything but a state this wrote. It is
                this relay's own record and nothing else writes it, so what is
                not a state is a defect to stop on rather than to count from.
        """
        if not state_file.is_file():
            return cls(armed=armed, seen=0)
        document = json.loads(state_file.read_text(encoding="utf-8"))
        state = cls.of(document)
        if state is None:
            message = f"{state_file} holds {document!r} rather than a relay state"
            raise ValueError(message)
        return state

    def write(self, state_file: Path) -> None:
        """Write the state so that the file holds the previous one or this one, never neither.

        The smoke stops a command that has not answered inside its bound by
        killing it, and a kill lands wherever the relay happens to be — on a
        loaded host, still starting up. A write that truncated the file in
        place and was killed before it filled it again would leave an empty
        file that no later invocation, and no test, could read. So the state
        is written beside the file and moved over it, which every platform
        does as one step: a kill before the move leaves the previous state
        and a leftover beside it, and the next write replaces both.

        Args:
            state_file: The file this relay counts in.
        """
        staging = state_file.with_name(state_file.name + STAGING_SUFFIX)
        staging.write_text(json.dumps({"armed": self.armed, "seen": self.seen}), encoding="utf-8")
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
    state = RelayState.read(state_file, armed=not armed_by)

    if command == hang_on and state.armed:
        state = RelayState(armed=True, seen=state.seen + 1)
        state.write(state_file)
        if state.seen > int(os.environ.get(AFTER, "0")):
            time.sleep(FOREVER_S)
    elif command and command == armed_by:
        RelayState(armed=True, seen=state.seen).write(state_file)

    # Not captured: what the program writes is what the smoke reads, so it goes
    # to this process's own streams rather than through a buffer of its own.
    return run([os.environ[PROGRAM], *argv], capture=False).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
