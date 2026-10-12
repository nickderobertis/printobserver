"""Each producer the harness reads, run for real, and read with the harness's parsers.

`spike_names.py` is the one statement of every record's shape; these hold the
two compiled producers and the lookup benchmark to it, so a producer that
renamed a field fails here rather than as a report silently missing it.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

import pytest
import spike_pytest
from conftest import HERE, PYTHONPATH, ROOT
from figures import NextestStatus, journey_figures, link_figures, pytest_figures, rust_figures
from levers import read_defender_state
from lookup_bench import CURRENT, LookupReport, MethodError
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run, start
from spike_names import (
    JOURNEY_TIMINGS,
    LINK_TIMINGS,
    REAL_LINKER,
    Cost,
    JourneyTiming,
    LinkOutput,
    LinkTiming,
    PytestSession,
    PytestTest,
    RecordError,
)


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
    (cost,) = read.by_kind.values()
    truth(0.05 <= cost.s < 5, describing=f"a 50 ms sleep's seconds as recorded: {cost.s}")


def test_a_sink_nothing_can_be_written_to_changes_nothing_about_the_test(tmp_path: Path) -> None:
    """A sink naming something unwritable leaves the timed test passing."""
    equal(_sink_test(tmp_path), 0, describing="the sink's test with the sink naming a directory")


@pytest.fixture(scope="module")
def noop_linker(tmp_path_factory: pytest.TempPathFactory) -> str:
    """A real linker's stand-in that takes any arguments and succeeds: `true`, or a built one."""
    if sys.platform != "win32":
        return "true"
    source = tmp_path_factory.mktemp("noop") / "noop.rs"
    source.write_text("fn main() {}\n", encoding="utf-8")
    built = source.with_suffix(".exe")
    run(["rustc", str(source), "-o", str(built)], check=True)
    return str(built)


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
    for method in read.methods:
        if method.present:
            ordered = 0 < (method.min_s or 0) <= (method.median_s or 0) <= (method.max_s or 0)
            truth(ordered, describing=f"min, median and max of {method}")
    truth(read.methods[0].is_the_command_line, describing="the current read answering the line")


def test_the_lookup_benchmark_refuses_an_invocation_without_one_report_path() -> None:
    """The benchmark refuses to run without exactly one report path."""
    ran = run(
        [sys.executable, str(HERE / "lookup_bench.py")],
        env={**os.environ, "PYTHONPATH": PYTHONPATH},
    )
    equal(ran.returncode, 2, describing="the exit with no report path")


def test_a_line_that_is_not_its_producers_record_is_counted_not_read(tmp_path: Path) -> None:
    """A malformed line, a non-finite number or a fractional count is counted as unreadable."""
    timings = tmp_path / "timings.jsonl"
    timings.write_text(
        "\n".join(
            [
                '{"kind":"k","ms":1.0,"pid":1,"thread":"t"}',
                '{"kind":"k","ms":NaN,"pid":1,"thread":"t"}',
                '{"kind":"k","ms":1.0,"pid":1.5,"thread":"t"}',
                "[1, 2]",
                "not json",
            ]
        ),
        encoding="utf-8",
    )
    read = journey_figures(timings)
    truth(read is not None, describing="the one readable record")
    if read is not None:
        equal((read.operations, read.unreadable), (1, 4), describing="read and unreadable lines")
    (tmp_path / "plugin").mkdir()
    (tmp_path / "plugin" / "pytest-main-1.jsonl").write_text("garbage\n", encoding="utf-8")
    plugin = pytest_figures(tmp_path / "plugin")
    truth(plugin is not None, describing="figures for a file of only unreadable lines")
    if plugin is not None:
        equal((plugin.tests, plugin.unreadable), (0, 1), describing="what such a file holds")


@pytest.mark.skipif(sys.platform == "win32", reason="a signal ends a process only on Unix")
def test_the_timed_linker_reports_a_signal_as_a_shell_does(
    tmp_path: Path, timed_linker: Path
) -> None:
    """A real linker ended by a signal is reported as 128 plus that signal."""
    killed = tmp_path / "killed-linker"
    killed.write_text("#!/bin/sh\nkill -TERM $$\n", encoding="utf-8")
    killed.chmod(0o755)
    ran = run([str(timed_linker)], env={**os.environ, REAL_LINKER: str(killed)})
    equal(ran.returncode, 128 + 15, describing="the wrapper's exit over a terminated linker")


def test_the_timed_linker_names_what_each_link_wrote(
    tmp_path: Path, timed_linker: Path, noop_linker: str
) -> None:
    """Each record says whether a path, a response file or nothing named the output."""
    links = tmp_path / "links.jsonl"
    real = noop_linker
    for arguments in (["-o", 'out"put'], ["/OUT:C:\\out.exe"], ["@response"], []):
        run(
            [str(timed_linker), *arguments],
            env={**os.environ, REAL_LINKER: real, LINK_TIMINGS: str(links)},
            check=True,
        )
    read = link_figures(links)
    truth(read is not None and read.unreadable == 0, describing=f"every record read in {read}")
    if read is not None:
        equal(
            [(link.output_kind, link.output) for link in read.per_output],
            [
                (LinkOutput.PATH, 'out"put'),
                (LinkOutput.PATH, "C:\\out.exe"),
                (LinkOutput.RESPONSE_FILE, "@response"),
                (LinkOutput.UNKNOWN, ""),
            ],
            describing="what each link named",
        )


def test_a_link_sink_nothing_can_be_written_to_still_exits_as_the_linker_did(
    tmp_path: Path, timed_linker: Path, noop_linker: str
) -> None:
    """With the sink naming a directory, the wrapper still exits as the real linker did."""
    real = noop_linker
    ran = run(
        [str(timed_linker)], env={**os.environ, REAL_LINKER: real, LINK_TIMINGS: str(tmp_path)}
    )
    equal(ran.returncode, 0, describing="the wrapper's exit with an unwritable sink")


def test_a_lookup_method_that_cannot_answer_fails_the_benchmark_naming_it(tmp_path: Path) -> None:
    """With nothing on the path to answer, the benchmark exits 1 and its report says why."""
    report = tmp_path / "lookup.json"
    ran = run(
        [sys.executable, str(HERE / "lookup_bench.py"), str(report)],
        env={**os.environ, "PYTHONPATH": PYTHONPATH, "PATH": str(tmp_path)},
    )
    equal(ran.returncode, 1, describing="the exit when a method could not answer")
    read = LookupReport.read(report)
    truth(read.failed, describing="the method that failed, named in the report")
    contains(ran.stderr, read.failed[0], describing="the failure named on standard error")


@pytest.mark.parametrize(
    ("parse", "record"),
    [
        (JourneyTiming.parse, {"kind": "k", "ms": -1.0, "pid": 1, "thread": "t"}),
        (JourneyTiming.parse, {"kind": 3, "ms": 1.0, "pid": 1, "thread": "t"}),
        (LinkTiming.parse, {"linker": "cc", "output_kind": "elsewhere", "output": "", "ms": 1.0}),
        (LinkTiming.parse, {"linker": "cc", "output_kind": "path", "output": "o", "ms": True}),
        (Cost.parse, {"count": -1, "s": 1.0}),
        (Cost.parse, [1, 2]),
        (
            PytestTest.parse,
            {"nodeid": "n", "worker": "w", "outcome": "won", "s": 1.0, "phases": {}, "costs": {}},
        ),
        (
            PytestTest.parse,
            {
                "nodeid": "n",
                "worker": "w",
                "outcome": "passed",
                "s": 1.0,
                "phases": {"lunch": 1.0},
                "costs": {},
            },
        ),
        (
            PytestSession.parse,
            {"worker": "w", "wall_s": 1, "collect_s": 1, "exitstatus": 0.5, "tests": 1},
        ),
        (
            PytestSession.parse,
            {
                "worker": "w",
                "wall_s": 1,
                "collect_s": 1,
                "exitstatus": 0,
                "tests": 1,
                "outside_tests": [],
            },
        ),
    ],
)
def test_a_record_its_producer_could_not_have_written_is_refused(
    parse: Callable[[object], object], record: object
) -> None:
    """Each parser refuses a record outside the shape its producer writes."""
    with pytest.raises(RecordError):
        parse(record)


def test_a_session_marker_that_is_neither_true_nor_absent_is_unreadable(tmp_path: Path) -> None:
    """Only `"session": true` is a session line; `"session": "false"` is counted unreadable."""
    (tmp_path / "pytest-main-1.jsonl").write_text('{"session": "false"}\n', encoding="utf-8")
    read = pytest_figures(tmp_path)
    truth(read is not None and read.unreadable == 1, describing=f"the line counted in {read}")


@pytest.mark.parametrize(
    "document",
    [
        "[]",
        '{"rounds": 5, "pid": 1}',
        '{"rounds": -1, "pid": 1, "methods": []}',
        '{"rounds": 5, "pid": 1, "methods": [{"name": "x"}]}',
        '{"rounds": 5, "pid": 1, "methods": ['
        + json.dumps(
            {
                "name": "x",
                "present": True,
                "median_s": float("nan"),
                "min_s": 0.1,
                "max_s": 0.1,
                "answered": "",
                "failure": "",
                "is_the_command_line": False,
                "same_as_current": False,
            }
        )
        + "]}",
    ],
)
def test_a_report_the_benchmark_did_not_write_is_refused(tmp_path: Path, document: str) -> None:
    """`LookupReport.read` refuses a report of any shape but the benchmark's own."""
    report = tmp_path / "lookup.json"
    report.write_text(document, encoding="utf-8")
    with pytest.raises(MethodError):
        LookupReport.read(report)


@pytest.mark.skipif(sys.platform == "win32", reason="the stand-in is a shell script")
def test_the_timed_linker_names_rust_llds_flavor(tmp_path: Path, timed_linker: Path) -> None:
    """In front of `rust-lld`, the wrapper first tells that driver which linker to be."""
    seen = tmp_path / "arguments"
    rust_lld = tmp_path / "rust-lld"
    rust_lld.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" > {seen}\n', encoding="utf-8")
    rust_lld.chmod(0o755)
    run(
        [str(timed_linker), "-o", "out"], env={**os.environ, REAL_LINKER: str(rust_lld)}, check=True
    )
    equal(
        seen.read_text(encoding="utf-8").split(), ["-flavor", "gnu", "-o", "out"], describing="it"
    )


#: What `DEFENDER_QUERY` answered on windows-11-arm in run 38200888322.
RECORDED_DEFENDER = (
    '{"mode":"Normal","excluded":["C:\\\\","D:\\\\"],"antivirus":true,"realtime":true}'
)


@pytest.mark.parametrize(
    ("answer", "readable"),
    [
        (RECORDED_DEFENDER, True),
        ("not json", False),
        ("[1, 2]", False),
        ('{"realtime": "yes", "excluded": [3, "C:\\\\"]}', True),
    ],
)
def test_defenders_answer_is_read_or_kept_as_unreadable(answer: str, readable: bool) -> None:
    """A recorded answer is read; any other shape is kept as unreadable, fields typed or absent."""
    state = read_defender_state(answer)
    equal(state.unreadable is None, readable, describing=f"the answer {answer!r}")
    if answer == RECORDED_DEFENDER:
        equal((state.realtime, state.excluded), (True, ("C:\\", "D:\\")), describing="it")
    if readable and answer != RECORDED_DEFENDER:
        equal((state.realtime, state.excluded), (None, ("C:\\",)), describing="its typed fields")


@pytest.mark.parametrize(
    ("args", "program"),
    [
        (["git", "--version"], "git"),
        ('"C:\\Program Files\\Git\\bin\\git.exe" --version', "git"),
        (b"/usr/bin/git --version", "git"),
        (Path("/usr/bin/uv"), "uv"),
        ([], "<unnamed>"),
        (object(), "<unnamed>"),
    ],
)
def test_a_child_is_charged_to_the_program_it_runs(args: object, program: str) -> None:
    """Every shape `Popen` accepts names its program, a quoted path with spaces included."""
    equal(spike_pytest._program(args), program, describing=f"the program of {args!r}")


def test_concurrent_test_processes_write_whole_records_to_one_sink(tmp_path: Path) -> None:
    """Eight test processes timing into one sink at once leave eight readable records."""
    built = run(
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
            "--no-run",
            "--message-format=json",
        ],
        cwd=ROOT,
        check=True,
    )
    executable = next(
        line["executable"]
        for line in map(json.loads, built.stdout.splitlines())
        if line.get("target", {}).get("name") == "journeys" and line.get("executable")
    )
    sink = tmp_path / "timings.jsonl"
    started = [
        start(
            [executable, "the_spike_timing_sink_records_a_timed_operation", "--exact"],
            env={**os.environ, JOURNEY_TIMINGS: str(sink)},
        )
        for _ in range(8)
    ]
    equal([process.wait() for process in started], [0] * 8, describing="each process's exit")
    read = journey_figures(sink)
    truth(read is not None, describing="records in the sink")
    if read is not None:
        equal((read.operations, read.unreadable), (8, 0), describing="whole records")


@pytest.mark.skipif(not Path("/dev/full").exists(), reason="needs a device every write to fails")
def test_a_link_record_that_cannot_be_written_is_said_and_the_exit_kept(
    timed_linker: Path,
) -> None:
    """A sink that opens but refuses the write is named on stderr; the exit stays the linker's."""
    ran = run(
        [str(timed_linker)], env={**os.environ, REAL_LINKER: "false", LINK_TIMINGS: "/dev/full"}
    )
    equal(ran.returncode, 1, describing="the real linker's own exit")
    contains(ran.stderr, "could not record this link", describing="what the wrapper said")


def test_the_lookup_benchmarks_current_read_is_octoprint_envs_own(tmp_path: Path) -> None:
    """The first method is `command_of` itself, and every method's line is compared with it."""
    report = tmp_path / "lookup.json"
    run(
        [sys.executable, str(HERE / "lookup_bench.py"), str(report)],
        env={**os.environ, "PYTHONPATH": PYTHONPATH},
        check=True,
    )
    read = LookupReport.read(report)
    equal(read.methods[0].name, CURRENT, describing="the first method")
    equal(read.methods[0].same_as_current, True, describing="the current read against itself")
    if sys.platform == "win32":
        native = next(m for m in read.methods if m.name == "win32 NtQueryInformationProcess")
        equal((native.is_the_command_line, native.same_as_current), (True, True), describing="it")


def test_a_status_this_does_not_know_is_kept_as_other_rather_than_dropped() -> None:
    """A finished-test line with an unknown status is read, as `OTHER`, with what nextest said."""
    log = "\n".join(
        [
            "    Starting 2 tests across 1 binary",
            "        PASS [   0.010s] (1/2) crate::binary passes",
            "        TRY 2 WEIRD [   0.020s] (2/2) crate::binary weird",
            "        SLOW [> 60.000s] (───) crate::binary still-running",
            "     Summary [   0.100s] 2 tests run",
        ]
    )
    read = rust_figures(log)
    truth(read is not None, describing="figures for a nextest log")
    if read is not None:
        equal(
            [(test.test, test.status, test.reported) for test in read.per_test],
            [
                ("passes", NextestStatus.PASSED, "PASS"),
                ("weird", NextestStatus.OTHER, "TRY 2 WEIRD"),
            ],
            describing="the finished tests, and not the one still running",
        )


@pytest.mark.skipif(not Path("/dev/full").exists(), reason="needs a device every write to fails")
def test_a_journey_sink_that_refuses_the_write_is_said_and_the_test_passes() -> None:
    """A sink that opens but refuses the write is named on stderr, and the timed test passes."""
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
            "--",
            "--nocapture",
        ],
        cwd=ROOT,
        env={**os.environ, JOURNEY_TIMINGS: "/dev/full"},
    )
    equal(ran.returncode, 0, describing="the timed test's own exit")
    contains(ran.stderr + ran.stdout, "was not recorded in /dev/full", describing="what it said")
