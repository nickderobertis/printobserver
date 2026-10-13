"""Temporary adapter for 00c5e84's five-call current-method lookup measurement."""

from __future__ import annotations

import json
import os
import platform
import statistics
import sys
import time

from octoprint_env import command_of
from repo_checks.shell import start

ROUNDS = 5
MARKER = "spike-lookup-bench-marker"


def main() -> None:
    """Time the real API, checking every answer against the real child's argv."""
    argv = [sys.executable, "-c", "import sys; sys.stdin.read()", MARKER]
    child = start(argv)
    seconds: list[float] = []
    try:
        for _ in range(ROUNDS):
            started = time.perf_counter()
            answer = command_of(child.pid)
            seconds.append(time.perf_counter() - started)
            if answer != " ".join(argv):
                raise RuntimeError(f"lookup did not return the full child command line: {answer!r}")
    finally:
        child.communicate(timeout=10)
    print(
        "HOST_LOOKUP_MEASUREMENT "
        + json.dumps(
            {
                "method": "current: octoprint_env.command_of",
                "platform": sys.platform,
                "machine": platform.machine(),
                "processors": os.cpu_count(),
                "rounds": ROUNDS,
                "seconds": seconds,
                "median_s": statistics.median(seconds),
                "command_line": " ".join(argv),
            }
        )
    )


if __name__ == "__main__":
    main()
