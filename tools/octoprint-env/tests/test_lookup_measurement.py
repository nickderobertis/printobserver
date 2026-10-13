"""Run the temporary measurement entry point against a real native child."""

from __future__ import annotations

import json
import os
import statistics
import subprocess
import sys
from pathlib import Path

import octoprint_env
import pytest
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.shell import run

SCRIPT = Path(__file__).resolve().parents[1] / "measure_host_lookup.py"


@pytest.mark.parametrize("query_available", [True, False])
def test_measurement_checks_five_native_queries_and_reaps_its_child(query_available: bool) -> None:
    """Validate measured output and EOF cleanup even when the native query is absent."""
    environment = dict(os.environ)
    environment["PYTHONPATH"] = os.pathsep.join(sys.path)
    if not query_available:
        environment["PATH"] = ""
    result = run([sys.executable, str(SCRIPT)], env=environment, timeout=180)
    pid = int(result.stdout.splitlines()[0].removeprefix("HOST_LOOKUP_CHILD "))
    if query_available:
        passing(result)
        measurement = json.loads(
            result.stdout.splitlines()[1].removeprefix("HOST_LOOKUP_MEASUREMENT ")
        )
        equal(measurement["rounds"], 5)
        equal(len(measurement["seconds"]) + len(measurement["censored_samples"]), 5)
        completed = measurement["seconds"]
        equal(measurement["median_s"], statistics.median(completed) if completed else None)
        equal(measurement["median_scope"], "completed_queries")
        for sample in measurement["censored_samples"]:
            equal(sample["bound_s"], 30)
            truth(sample["observed_elapsed_s"] > 0, describing="a timed native query")
        contains(
            measurement["command_line"], "import sys; sys.stdin.read() spike-lookup-bench-marker"
        )
    else:
        failing(result, naming="Verify the native process query tool is on PATH and accessible")
        contains(result.stderr, "retry just host-lookup-measure")
    # The real native query can find no process after EOF cleanup.
    try:
        answer = octoprint_env.command_of(pid)
    except subprocess.CalledProcessError:
        pass
    else:
        equal(answer, "", describing="the measurement child to have exited")
