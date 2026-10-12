"""Each producer the harness reads, run for real, and read with the harness's parsers.

`spike_names.py` is the one statement of every record's shape; these hold the
two compiled producers and the lookup benchmark to it, so a producer that
renamed a field fails here rather than as a report silently missing it.
"""

from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

import pytest
from conftest import HERE, PYTHONPATH, ROOT
from figures import journey_figures, link_figures
from lookup_bench import LookupReport
from repo_checks.expect import equal, truth
from repo_checks.shell import run
from spike_names import JOURNEY_TIMINGS, LINK_TIMINGS, REAL_LINKER


def _sink_test(sink: Path) -> int:
    ran = run(
        [
            "cargo",
            "test",
            "--locked",
            "-p",
            "printobserver",
            "--features",
            "test-fixtures",
            "--test",
            "journeys",
            "the_spike_timing_sink_records_a_timed_operation",
        ],
        cwd=ROOT,
        env={**os.environ, JOURNEY_TIMINGS: str(sink)},
    )
    return ran.returncode


def test_the_journeys_timing_sink_writes_what_the_harness_reads(tmp_path: Path) -> None:
    """The journeys' sink writes a record `JourneyTiming` reads, its kind escaped faithfully."""
    sink = tmp_path / "timings.jsonl"
    equal(_sink_test(sink), 0, describing="the sink's own test")
    read = journey_figures(sink)
    truth(read is not None, describing="a record in the sink")
    if read is None:
        return
    equal(read.unreadable, 0, describing="lines the harness could not read")
    equal(list(read.by_kind), ['spike "sink" \x01'], describing="the kind, escaped and read back")


def test_a_sink_nothing_can_be_written_to_changes_nothing_about_the_test(tmp_path: Path) -> None:
    """A sink naming something unwritable leaves the timed test passing."""
    equal(_sink_test(tmp_path), 0, describing="the sink's test with the sink naming a directory")


@pytest.fixture(scope="module")
def timed_linker() -> Path:
    """The wrapper, built the way `measure.py` builds it."""
    built = Path(tempfile.mkdtemp(prefix="timed-linker-")) / (
        "timed-linker.exe" if sys.platform == "win32" else "timed-linker"
    )
    run(
        ["rustc", "--edition", "2021", "-O", str(HERE / "timed_linker.rs"), "-o", str(built)],
        check=True,
    )
    return built


def test_the_timed_linker_records_every_link_it_makes(tmp_path: Path, timed_linker: Path) -> None:
    """A real link through the wrapper is recorded as one `LinkTiming`."""
    run(["cargo", "new", "--quiet", "--vcs", "none", str(tmp_path / "smoke")], check=True)
    host = run(["rustc", "-vV"], check=True).stdout.split("host: ")[1].split()[0]
    variable = "CARGO_TARGET_" + host.replace("-", "_").upper() + "_LINKER"
    links = tmp_path / "links.jsonl"
    built = run(
        ["cargo", "build", "--quiet", "--manifest-path", str(tmp_path / "smoke/Cargo.toml")],
        env={
            **os.environ,
            variable: str(timed_linker),
            LINK_TIMINGS: str(links),
            "CARGO_TARGET_DIR": str(tmp_path / "target"),
        },
    )
    equal(built.returncode, 0, describing=f"a link through the wrapper: {built.stderr}")
    read = link_figures(links)
    truth(read is not None and read.count == 1, describing=f"one link recorded in {read}")
    if read is not None:
        truth("smoke" in read.per_output[0].output, describing="the link's output named")


def test_the_timed_linker_exits_as_the_real_linker_did(timed_linker: Path) -> None:
    """The wrapper exits with the real linker's code, and 127 where it could not run it."""
    for real, code in (("false", 1), ("no-such-linker-anywhere", 127)):
        ran = run([str(timed_linker)], env={**os.environ, REAL_LINKER: real})
        equal(ran.returncode, code, describing=f"the wrapper's exit over `{real}`")


def test_the_lookup_benchmark_reads_this_hosts_command_line(tmp_path: Path) -> None:
    """The benchmark answers this host's own read of a process's command line."""
    report = tmp_path / "lookup.json"
    ran = run(
        [sys.executable, str(HERE / "lookup_bench.py"), str(report)],
        env={**os.environ, "PYTHONPATH": PYTHONPATH},
    )
    equal(ran.returncode, 0, describing=f"the benchmark's exit: {ran.stderr}")
    read = LookupReport.read(report)
    equal(read.failed, [], describing="methods that did not answer")
    truth(read.methods[0].is_the_command_line, describing="the current read answering the line")


def test_the_lookup_benchmark_refuses_an_invocation_without_one_report_path() -> None:
    """The benchmark refuses to run without exactly one report path."""
    ran = run(
        [sys.executable, str(HERE / "lookup_bench.py")],
        env={**os.environ, "PYTHONPATH": PYTHONPATH},
    )
    equal(ran.returncode, 2, describing="the exit with no report path")
