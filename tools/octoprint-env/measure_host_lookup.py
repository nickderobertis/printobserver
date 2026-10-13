"""Measure native process lookup latency while checking complete command identity."""

from __future__ import annotations

import json
import os
import platform
import statistics
import subprocess
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
    print(f"HOST_LOOKUP_CHILD {child.pid}", flush=True)
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
    try:
        main()
    except (OSError, subprocess.SubprocessError, RuntimeError) as error:
        print(
            f"Lookup measurement failed: {error}. "
            "Verify the native process query tool is on PATH and accessible, "
            "then retry just host-lookup-measure.",
            file=sys.stderr,
        )
        sys.exit(1)
