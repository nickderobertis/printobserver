"""One lane of a job run in the background, so that a job can run another beside it.

The `macos` job of `ci.yml` runs the gate and the printer integration tier on one
runner, and the integration tier is paced by its virtual printer rather than by
the runner's processors, so it runs beside the gate rather than after it. A step
ends when its command does, so the lane is started detached and answered by a
later step:

* `start LANE COMMAND...` starts COMMAND detached and returns at once;
* `run LANE COMMAND...` is what `start` runs: COMMAND, then its output written
  to `LANE/log` and its exit status to `LANE/exit`, the status last and whole,
  so a waiter that finds the file reads the lane's verdict and never part of it;
* `wait LANE` waits for `LANE/exit`, prints the lane's output, and exits with
  its status.
"""

from __future__ import annotations

import shutil
import sys
import time
from pathlib import Path

from repo_checks.shell import run, start

#: The lane's output, and its exit status.
LOG = "log"
EXIT = "exit"
#: Seconds between two looks for the lane's exit status.
POLL = 10.0
#: How a usage error is answered.
USAGE = 2


def begin(lane: Path, command: list[str]) -> int:
    """Start `command` detached as a lane under `lane`, emptied first."""
    shutil.rmtree(lane, ignore_errors=True)
    lane.mkdir(parents=True)
    start([sys.executable, "-m", "repo_checks.lanes", "run", str(lane), *command], own_group=True)
    print(f"lanes: started `{' '.join(command)}` under {lane}", file=sys.stderr)
    return 0


def execute(lane: Path, command: list[str]) -> int:
    """Run `command`, then keep its output and, last and whole, its exit status."""
    completed = run(command, capture=True)
    (lane / LOG).write_text(completed.stdout + completed.stderr, encoding="utf-8")
    status = lane / f"{EXIT}.partial"
    status.write_text(str(completed.returncode), encoding="utf-8")
    status.replace(lane / EXIT)
    return completed.returncode


def wait(lane: Path, poll: float = POLL) -> int:
    """Wait for the lane's exit status, print its output, and answer that status."""
    if not lane.is_dir():
        print(f"lanes: no lane was started under {lane}; start it first", file=sys.stderr)
        return USAGE
    while not (lane / EXIT).is_file():
        time.sleep(poll)
    sys.stdout.write((lane / LOG).read_text(encoding="utf-8"))
    sys.stdout.flush()
    return int((lane / EXIT).read_text(encoding="utf-8"))


def main(argv: list[str] | None = None) -> int:
    """`start`, `run` or `wait` one lane."""
    arguments = sys.argv[1:] if argv is None else argv
    match arguments:
        case ["start", lane, program, *rest]:
            return begin(Path(lane), [program, *rest])
        case ["run", lane, program, *rest]:
            return execute(Path(lane), [program, *rest])
        case ["wait", lane]:
            return wait(Path(lane))
        case _:
            print(
                "usage: python -m repo_checks.lanes start LANE COMMAND... | run LANE COMMAND... "
                "| wait LANE",
                file=sys.stderr,
            )
            return USAGE


if __name__ == "__main__":
    sys.exit(main())
