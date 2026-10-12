"""The harness, driven as a workflow cell drives it, over real tasks."""

from __future__ import annotations

import os
import re
import shutil
import statistics
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

import pytest
from conftest import CASES_OUTCOMES, CASES_TESTS, PYTHONPATH, ROOT, mapping, measure, tasks
from levers import LEVERS, SPIKE_CHANGED, defender_state, pytest_temporaries
from measure import PSEUDO_TARGETS
from repo_checks.expect import absent, contains, equal, truth
from repo_checks.shell import run
from spike_names import PYTEST_OUT, REAL_LINKER

VISION = "printobserver-vision-api:test"

#: The Rust sources that time the client-command tier's operations.
INSTRUMENTED = [
    ROOT / "crates/printobserver/tests" / path
    for path in (
        "journeys/tier.rs",
        "journeys/confirming.rs",
        "journeys/durations.rs",
        "support/world.rs",
        "support/traced.rs",
    )
]
#: Which platform's observation each `traced:` kind belongs to, by what it names.
OBSERVED_BY = {
    "etw-": "win32",
    "invocation-run": "win32",
    "strace": "linux",
    "interposed": "darwin",
}


def instrumented_kinds() -> set[str]:
    """Every kind the instrumented sources time that this platform's run reaches."""
    named = {
        found
        for source in INSTRUMENTED
        for found in re.findall(
            r'"((?:tier|world|confirming|durations|traced):[^"]+)"',
            source.read_text(encoding="utf-8"),
        )
    }
    return {
        kind
        for kind in named
        if not kind.startswith("traced:")
        or any(part in kind and host == sys.platform for part, host in OBSERVED_BY.items())
    }


def test_every_lever_and_pseudo_target_is_listed(tmp_path: Path) -> None:
    """`--list` names every lever and pseudo-target the harness declares."""
    listed = measure("--list", out=tmp_path)
    equal(listed.code, 0, describing="`--list`'s exit")
    for name in [*LEVERS, *PSEUDO_TARGETS]:
        contains(listed.out, name, describing="what `--list` names")


def test_an_invocation_naming_nothing_real_is_refused_before_anything_runs(
    tmp_path: Path,
) -> None:
    """A lever, target, project or output directory it cannot use exits 2, writing nothing."""
    for arguments, naming in (
        (["--experiment", "no-such-lever", "--targets", "octoprint-env:test"], "no-such-lever"),
        (["--targets", "not a task"], "not a task"),
        (["--targets", "no-such-project:test"], "no-such-project"),
        (["--targets", "*:no-such-target"], "no-such-target"),
        (["--targets", "octoprint-env:no-such-target"], "no-such-target"),
        (["--targets", ""], "no target"),
        (["--targets", "octoprint-env:test", "--out", str(ROOT / "spike-out")], "inside the"),
        (["--experiment", "base-tree+base-tree", "--targets", "octoprint-env:test"], "more than"),
    ):
        refused = measure(*arguments, out=tmp_path / "refused")
        equal(refused.code, 2, describing=f"the exit for {arguments}")
        contains(refused.err, naming, describing=f"the refusal of {arguments}")
        truth(refused.report is None, describing=f"no report written for {arguments}")


def test_a_pytest_task_is_measured_test_by_test_and_its_failure_named(
    tmp_path: Path, fixture_project: str
) -> None:
    """A pytest task is recorded test by test with what each spent, and a failed task named."""
    standing = Path(tempfile.gettempdir()) / f"pytest-of-spike-{os.getpid()}" / "pytest-standing"
    standing.mkdir(parents=True)
    before = pytest_temporaries()
    measured = measure(
        "--targets", f"{fixture_project}:suite,{fixture_project}:fails", out=tmp_path
    )
    truth(standing.is_dir(), describing="a pytest temporary that stood before the run")
    shutil.rmtree(standing.parent)
    equal(measured.code, 1, describing="the exit when a task failed")
    contains(measured.err, f"{fixture_project}:fails exited 3", describing="the failure's naming")
    suite, fails = tasks(measured)
    equal(fails["exit"], 3, describing="the failing task's own exit")
    pytest_run = mapping(suite["pytest"], describing="the suite's figures")
    equal(pytest_run["tests"], CASES_TESTS, describing="the tests the plugin recorded")
    equal(pytest_run["outcomes"], CASES_OUTCOMES, describing="their outcomes")
    by_cost = pytest_run["by_cost"]
    for cost, count in (
        ("spawn:git", 2),
        ("copytree", 1),
        ("copy2", 1),
        ("rmtree", 1),
        ("http", 1),
    ):
        equal(by_cost[cost]["count"], count, describing=f"`{cost}` in {by_cost}")
    truth(by_cost["sleep"]["s"] >= 0.05, describing=f"the sleep's seconds in {by_cost}")
    absent(by_cost, "spawn:spike-no-such-program-anywhere", describing="a child that never ran")
    sleeping = next(
        test for test in pytest_run["per_test"] if test["nodeid"].endswith("test_sleeps")
    )
    truth(sleeping["phases"]["call"] >= 0.05, describing=f"the sleeping call's phase in {sleeping}")
    truth(
        abs(sleeping["s"] - sum(sleeping["phases"].values())) < 0.01,
        describing=f"a test's seconds as the sum of its phases in {sleeping}",
    )
    (session,) = pytest_run["sessions"]
    equal((session["exitstatus"], session["tests"]), (1, CASES_TESTS), describing=str(session))
    truth(
        session["collect_s"] > 0 and session["wall_s"] >= pytest_run["test_s"],
        describing=f"collection and wall time against the tests' own in {session}",
    )
    disk = suite["disk"]
    truth(
        disk["freed_after_gb"] >= 0.03 and all(space["free_gb"] > 0 for space in disk["before"]),
        describing=f"the large temporary freed after the task, and free space read, in {disk}",
    )
    equal(by_cost["sleep"]["count"], 2, describing="both sleeps, the one that raised included")
    equal(session["outside_tests"]["spawn:git"]["count"], 1, describing="the spawn at import")
    own = [line for line in measured.out.splitlines() if line.startswith("measure: ")]
    equal(len(own), 1, describing="the lines the harness itself prints on standard output")
    left = {path for path in pytest_temporaries() - before if not tmp_path.is_relative_to(path)}
    equal(left, set(), describing="the pytest temporaries the suite's own run left")


def test_xdist_workers_collect_one_list_under_stable_ids_each_under_coverage(
    tmp_path: Path, fixture_project: str, coverage_data: Callable[[], set[Path]]
) -> None:
    """Under xdist, workers rendering a parameter differently still run each test once."""
    measured = measure(
        "--experiment", "xdist-2", "--targets", f"{fixture_project}:covered", out=tmp_path
    )
    (suite,) = tasks(measured)
    pytest_run = mapping(suite["pytest"], describing="the suite's figures")
    ids = [test["nodeid"] for test in pytest_run["per_test"]]
    equal(len(ids), len(set(ids)), describing="each test recorded once")
    equal(len(ids), CASES_TESTS, describing="every test run")
    equal(
        sorted({session["worker"] for session in pytest_run["sessions"]}),
        ["gw0", "gw1", "main"],
        describing="the processes recorded",
    )
    contains(str(suite["command"]), "-n 2 --dist load", describing="the command")
    equal(len(coverage_data()), 2, describing="one coverage data file per measured worker")


def test_python_coverage_levers_change_the_command_and_the_environment(
    tmp_path: Path, fixture_project: str, coverage_data: Callable[[], set[Path]]
) -> None:
    """`no-py-coverage` drops `coverage run`, and `sysmon` names its core, for the task."""
    plain = measure(
        "--experiment",
        "no-py-coverage",
        "--targets",
        f"{fixture_project}:covered",
        out=tmp_path / "a",
    )
    (task,) = tasks(plain)
    contains(str(task["command"]), "uv run -q python -m pytest", describing="the command run")
    equal(coverage_data(), set(), describing="the coverage data an uncovered run wrote")
    sysmon = measure(
        "--experiment", "sysmon", "--targets", f"{fixture_project}:covered", out=tmp_path / "b"
    )
    env = mapping(sysmon.report, describing="the report")["env"]
    equal(env["COVERAGE_CORE"], "sysmon", describing=f"the core named in {env}")
    truth(coverage_data(), describing="the data a covered run wrote")


def test_a_task_runs_where_its_project_says_and_a_wildcard_names_every_declarer(
    tmp_path: Path, fixture_project: str
) -> None:
    """A task runs in its declared `cwd`, and `*:<target>` is every project declaring it."""
    measured = measure("--targets", "*:here", out=tmp_path)
    equal(measured.code, 0, describing="the run's exit")
    equal(
        [task["task"] for task in tasks(measured)],
        [f"{fixture_project}:here"],
        describing="the tasks",
    )
    truth(
        (ROOT / "tools" / fixture_project / "cases" / "ran-here").is_file(),
        describing="the file the task wrote in its own directory",
    )


def test_the_base_tree_is_put_back_after_the_run(tmp_path: Path, fixture_project: str) -> None:
    """`base-tree` puts the spike's own files back byte for byte, uncommitted work included."""
    before = {path: (ROOT / path).read_bytes() for path in SPIKE_CHANGED}
    measured = measure(
        "--experiment", "base-tree", "--targets", f"{fixture_project}:fails", out=tmp_path
    )
    equal(measured.code, 1, describing="the exit of a run whose one task fails")
    for path in SPIKE_CHANGED:
        equal((ROOT / path).read_bytes(), before[path], describing=f"{path} after the run")


@pytest.mark.skipif(sys.platform == "win32", reason="these levers change a Windows host for real")
def test_a_host_lever_this_host_cannot_take_says_it_did_not_apply(
    tmp_path: Path, fixture_project: str
) -> None:
    """Off Windows the Defender and D: levers apply nothing, and the report says so."""
    measured = measure(
        "--experiment",
        "defender-excluded+defender-off+temp-on-d",
        "--targets",
        f"{fixture_project}:fails",
        out=tmp_path,
    )
    prepared = mapping(measured.report, describing="the report")["prepared"]
    equal([step["applied"] for step in prepared], [False, False, False], describing=str(prepared))


def test_the_pseudo_targets_carry_their_own_figures(tmp_path: Path) -> None:
    """`host:defender` and `bench:octoprint-lookup` put their answers in the report."""
    measured = measure("--targets", "host:defender,bench:octoprint-lookup", out=tmp_path)
    equal(measured.code, 0, describing=f"the run's exit: {measured.err}")
    defender, lookup = tasks(measured)
    equal(defender["defender"]["applies"], sys.platform == "win32", describing="Defender's state")
    methods = mapping(lookup["lookup"], describing="the benchmark")["methods"]
    truth(methods[0]["is_the_command_line"], describing=f"the current read in {methods}")


def test_a_nextest_task_is_read_into_compile_run_per_test_and_link_figures(tmp_path: Path) -> None:
    """A crate's covered run is read into compile, run, per-test and link figures."""
    measured = measure("--targets", VISION, out=tmp_path)
    equal(measured.code, 0, describing="a passing crate's run")
    (crate,) = tasks(measured)
    rust = mapping(crate["rust"], describing="the crate's figures")
    truth(rust["tests"] > 0 and rust["binaries"] > 0, describing=f"the counts in {rust}")
    equal(len(rust["per_test"]), rust["tests"], describing="a duration for every test")
    truth(rust["run_s"] is not None, describing="nextest's own run time")
    report = mapping(measured.report, describing="the report")
    truth(report["linker"]["used"], describing=f"the timed linker in {report['linker']}")


def test_the_rust_levers_change_what_runs(tmp_path: Path) -> None:
    """`compile-only` builds and runs nothing; `no-rust-coverage` runs every test uncovered."""
    built = measure("--experiment", "compile-only", "--targets", VISION, out=tmp_path / "a")
    equal(built.code, 0, describing="a build-only run's exit")
    equal(tasks(built)[0]["rust"]["tests"], 0, describing="the tests a build-only run ran")
    plain = measure("--experiment", "no-rust-coverage", "--targets", VISION, out=tmp_path / "b")
    (crate,) = tasks(plain)
    contains(str(crate["command"]), "cargo nextest run", describing="the uncovered command")
    truth(crate["rust"]["tests"] > 0, describing="the tests an uncovered run ran")


def test_the_journeys_report_the_operations_of_the_client_command_tier(tmp_path: Path) -> None:
    """The printobserver crate's run reports the tier's own operations by kind."""
    measured = measure("--targets", "printobserver:test", out=tmp_path)
    (crate,) = tasks(measured)
    kinds = mapping(crate["journey"], describing="the journeys' figures")["by_kind"]
    expected = instrumented_kinds()
    truth(
        {kind.split(":")[0] for kind in expected}
        == {"tier", "world", "confirming", "durations", "traced"},
        describing=f"every family of kind read out of the sources: {sorted(expected)}",
    )
    for kind in sorted(expected):
        truth(kinds.get(kind, {}).get("count", 0) > 0, describing=f"`{kind}` among {sorted(kinds)}")


def test_a_cell_runs_every_run_and_fails_if_any_did(tmp_path: Path, fixture_project: str) -> None:
    """`just spike-cell` measures each run whatever an earlier one answered."""
    ran = run(
        [
            "just",
            "spike-cell",
            f"baseline|{fixture_project}:fails;baseline|{fixture_project}:here",
            str(tmp_path),
        ],
        cwd=ROOT,
        env={**os.environ},
    )
    truth(ran.returncode != 0, describing="the cell's exit after a failed run")
    for run_dir in ("1-baseline", "2-baseline"):
        truth((tmp_path / run_dir / "measure.json").is_file(), describing=f"{run_dir}'s report")


@pytest.mark.parametrize(
    ("lever", "variable", "value"),
    [
        ("line-tables-only", "CARGO_PROFILE_DEV_DEBUG", "line-tables-only"),
        ("debug-0", "CARGO_PROFILE_DEV_DEBUG", "0"),
        ("incremental-on", "CARGO_INCREMENTAL", "1"),
        ("split-unpacked", "CARGO_PROFILE_DEV_SPLIT_DEBUGINFO", "unpacked"),
    ],
)
def test_a_build_lever_reaches_the_build_it_measures(
    tmp_path: Path, lever: str, variable: str, value: str
) -> None:
    """Each build-profile lever is in the environment the crate's build ran under."""
    # No compiler cache in front of rustc, as on the runners the levers are measured on:
    # sccache refuses an incremental build outright.
    measured = measure(
        "--experiment",
        f"compile-only+{lever}",
        "--targets",
        VISION,
        out=tmp_path,
        environment={"RUSTC_WRAPPER": ""},
    )
    equal(measured.code, 0, describing=f"a build under `{lever}`")
    env = mapping(measured.report, describing="the report")["env"]
    equal(env[variable], value, describing=f"`{variable}` in {env}")
    log = (tmp_path / "printobserver-vision-api_test.log").read_text(encoding="utf-8")
    contains(log, "Compiling printobserver-vision-api", describing="the crate rebuilt under it")


def test_a_linker_the_smoke_link_refuses_leaves_the_build_on_its_own_linker(
    tmp_path: Path,
) -> None:
    """A real linker that fails the smoke link is reported, and the task builds without it."""
    measured = measure("--targets", VISION, out=tmp_path, environment={REAL_LINKER: "false"})
    equal(measured.code, 0, describing="the task's run on its own linker")
    linker = mapping(measured.report, describing="the report")["linker"]
    equal(linker["used"], False, describing=str(linker))
    contains(linker["why"], "smoke link failed", describing="why it was not used")


def test_a_wrapper_that_does_not_build_is_reported_and_the_run_goes_on(
    tmp_path: Path, fixture_project: str
) -> None:
    """A `rustc` that cannot build the wrapper is reported, and the tasks still run."""
    refusing = tmp_path / "bin"
    refusing.mkdir()
    rustc = refusing / ("rustc.bat" if sys.platform == "win32" else "rustc")
    rustc.write_text(
        "@exit /b 1\n" if sys.platform == "win32" else "#!/bin/sh\nexit 1\n", encoding="utf-8"
    )
    rustc.chmod(0o755)
    measured = measure(
        "--targets",
        f"{fixture_project}:here",
        out=tmp_path / "out",
        environment={"PATH": os.pathsep.join([str(refusing), os.environ["PATH"]])},
    )
    equal(measured.code, 0, describing="the run's exit")
    linker = mapping(measured.report, describing="the report")["linker"]
    contains(linker["why"], "did not build", describing=str(linker))


@pytest.mark.skipif(sys.platform == "win32", reason="on Windows the lever names rust-lld for real")
def test_rust_lld_off_windows_says_it_did_not_apply(tmp_path: Path, fixture_project: str) -> None:
    """Off Windows `rust-lld` names no linker, and the report says so."""
    measured = measure(
        "--experiment", "rust-lld", "--targets", f"{fixture_project}:here", out=tmp_path
    )
    report = mapping(measured.report, describing="the report")
    equal(
        [step["applied"] for step in report["prepared"]],
        [False],
        describing=str(report["prepared"]),
    )
    truth(report["linker"]["used"], describing="the timed linker in front of the usual one")


HOSTED_WINDOWS = sys.platform == "win32" and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"


@pytest.mark.skipif(
    not HOSTED_WINDOWS, reason="changes Defender and TEMP; only on a disposable runner"
)
def test_the_windows_host_levers_change_the_host(tmp_path: Path, fixture_project: str) -> None:
    """On a hosted Windows runner the Defender and D: levers take effect on the host."""
    measured = measure(
        "--experiment",
        "defender-excluded+defender-off+temp-on-d",
        "--targets",
        f"{fixture_project}:suite",
        out=tmp_path,
    )
    report = mapping(measured.report, describing="the report")
    equal(
        [step["applied"] for step in report["prepared"]], [True, True, True], describing=str(report)
    )
    state = defender_state()
    equal(state.realtime, False, describing="real-time scanning after `defender-off`")
    contains(state.excluded, str(ROOT), describing="the workspace among Defender's exclusions")
    truth(
        any(Path("D:/spike-temp").glob("pytest-of-*")),
        describing="the suite's pytest temporaries made on D:",
    )


@pytest.mark.skipif(
    not HOSTED_WINDOWS, reason="rust-lld is this lever's linker on windows-msvc alone"
)
def test_rust_lld_links_the_build_on_windows(tmp_path: Path) -> None:
    """On Windows the build links through rust-lld, timed where the wrapper could front it."""
    measured = measure("--experiment", "compile-only+rust-lld", "--targets", VISION, out=tmp_path)
    equal(measured.code, 0, describing="a build linked by rust-lld")
    report = mapping(measured.report, describing="the report")
    linker = report["linker"]
    if linker["used"]:
        equal(linker["real"], "rust-lld", describing="the linker the wrapper stood in front of")
        truth(report["links"]["count"] > 0, describing="the links it timed")
    else:
        contains(list(report["env"].values()), "rust-lld", describing="the untimed fallback")


def test_the_plugin_unloaded_or_unconfigured_leaves_pytest_as_it_was(tmp_path: Path) -> None:
    """Loaded with no output named it writes nothing; configured, it puts back what it wrapped."""
    script = tmp_path / "check.py"
    script.write_text(
        "import shutil, subprocess, sys, time, urllib.request\n"
        "import pytest\n"
        "before = (shutil.copy2, shutil.copytree, shutil.rmtree, time.sleep,\n"
        "          urllib.request.urlopen, subprocess.Popen.__init__, subprocess.Popen.wait)\n"
        "code = pytest.main([sys.argv[1], '-q', '-p', 'spike_pytest', '-p', 'no:cacheprovider'])\n"
        "after = (shutil.copy2, shutil.copytree, shutil.rmtree, time.sleep,\n"
        "         urllib.request.urlopen, subprocess.Popen.__init__, subprocess.Popen.wait)\n"
        "print('restored' if all(a is b for a, b in zip(before, after)) else 'left wrapped')\n"
        "sys.exit(code)\n",
        encoding="utf-8",
    )
    (tmp_path / "test_one.py").write_text("def test_one():\n    pass\n", encoding="utf-8")
    out = tmp_path / "records"
    for environment, written in (({}, False), ({PYTEST_OUT: str(out)}, True)):
        ran = run(
            [sys.executable, str(script), str(tmp_path / "test_one.py")],
            cwd=tmp_path,
            env={**os.environ, "PYTHONPATH": PYTHONPATH, **environment},
        )
        equal(ran.returncode, 0, describing=f"the suite under {environment}: {ran.stdout}")
        contains(ran.stdout, "restored", describing="every wrapped function after the session")
        equal(
            out.is_dir() and any(out.iterdir()), written, describing=f"records under {environment}"
        )


def test_compile_only_keeps_a_tasks_own_filter_and_runs_nothing(tmp_path: Path) -> None:
    """A command with its own `-E` filter builds under `none() & (...)` and runs no test."""
    measured = measure(
        "--experiment", "compile-only", "--targets", "printobserver-octoprint:test", out=tmp_path
    )
    equal(measured.code, 0, describing="a build-only run of a filtered crate")
    (crate,) = tasks(measured)
    contains(str(crate["command"]), '-E "none() & (not binary(integration))"', describing="it")
    equal(crate["rust"]["tests"], 0, describing="the tests it ran")


def test_xdist_without_coverage_runs_every_test_and_measures_none(
    tmp_path: Path, fixture_project: str, coverage_data: Callable[[], set[Path]]
) -> None:
    """`xdist-2-nocov` spreads the suite over two workers and writes no coverage data."""
    measured = measure(
        "--experiment", "xdist-2-nocov", "--targets", f"{fixture_project}:covered", out=tmp_path
    )
    (suite,) = tasks(measured)
    pytest_run = mapping(suite["pytest"], describing="the suite's figures")
    equal(pytest_run["tests"], CASES_TESTS, describing="every test, once")
    equal(coverage_data(), set(), describing="the coverage data an uncovered xdist run wrote")


def test_a_crates_figures_are_its_own_tests_totals(tmp_path: Path) -> None:
    """Compile time, the fastest and median test and every status are read off the run itself."""
    measured = measure("--targets", VISION, out=tmp_path)
    rust = mapping(tasks(measured)[0]["rust"], describing="the crate's figures")
    durations = sorted(test["s"] for test in rust["per_test"])
    truth(rust["compile_s"] > 0, describing=f"cargo's compile time in {rust['compile_s']}")
    equal(rust["fastest_test_s"], durations[0], describing="the fastest test")
    equal(rust["median_test_s"], statistics.median(durations), describing="the median test")
    equal({test["status"] for test in rust["per_test"]}, {"PASS"}, describing="every status")


def test_a_rerun_into_the_same_output_reports_only_its_own_records(
    tmp_path: Path, fixture_project: str
) -> None:
    """Measuring the same task into the same directory twice reports the second run alone."""
    for _ in range(2):
        measured = measure("--targets", f"{fixture_project}:suite", out=tmp_path)
    (suite,) = tasks(measured)
    equal(suite["pytest"]["tests"], CASES_TESTS, describing="the second run's tests")
    links = mapping(measured.report, describing="the report")["links"]
    equal(links, None, describing="links a run that built nothing reports")


def test_a_host_that_fails_mid_preparation_still_gets_its_files_back(
    tmp_path: Path, fixture_project: str
) -> None:
    """A preparation that raises after `base-tree` replaced the files still puts them back."""
    before = {path: (ROOT / path).read_bytes() for path in SPIKE_CHANGED}
    answering = tmp_path / "bin"
    answering.mkdir()
    rustc = answering / ("rustc.bat" if sys.platform == "win32" else "rustc")
    rustc.write_text(
        "@exit /b 0\n" if sys.platform == "win32" else "#!/bin/sh\nexit 0\n", encoding="utf-8"
    )
    rustc.chmod(0o755)
    measured = measure(
        "--experiment",
        "base-tree",
        "--targets",
        f"{fixture_project}:here",
        out=tmp_path / "out",
        environment={"PATH": os.pathsep.join([str(answering), os.environ["PATH"]])},
    )
    equal(measured.code, 1, describing="the exit when the host could not be prepared")
    contains(measured.err, "could not be prepared", describing="what it said")
    for path in SPIKE_CHANGED:
        equal((ROOT / path).read_bytes(), before[path], describing=f"{path} after the failure")


def test_a_failing_crate_is_read_with_its_failure_out_of_the_passing_figures(
    tmp_path: Path, fixture_crate: str
) -> None:
    """A nextest run with a failing test records it as failed and keeps it out of the medians."""
    measured = measure("--targets", fixture_crate, out=tmp_path)
    truth(measured.code != 0, describing="the exit of a run with a failing test")
    rust = mapping(tasks(measured)[0]["rust"], describing="the crate's figures")
    statuses = {test["test"].split("::")[-1]: test["status"] for test in rust["per_test"]}
    equal(statuses, {"passes": "PASS", "fails": "FAIL"}, describing="each test's status")
    passing = next(test["s"] for test in rust["per_test"] if test["status"] == "PASS")
    equal((rust["fastest_test_s"], rust["median_test_s"]), (passing, passing), describing="it")


def test_compile_only_refuses_a_task_that_runs_no_nextest(
    tmp_path: Path, fixture_project: str
) -> None:
    """`compile-only` over a task that starts no nextest runs nothing and names why."""
    measured = measure(
        "--experiment", "compile-only", "--targets", f"{fixture_project}:here", out=tmp_path
    )
    equal(measured.code, 1, describing="the run's exit")
    (task,) = tasks(measured)
    equal(task["exit"], 2, describing="the task's own")
    contains(measured.err, "starts none", describing="why it was not run")
    truth(
        not (ROOT / "tools" / fixture_project / "cases" / "ran-here").exists(),
        describing="the command left unrun",
    )


def test_compile_only_gives_an_unfiltered_nextest_its_own_filter(
    tmp_path: Path, fixture_crate: str
) -> None:
    """A nextest command with neither a filter nor `--no-tests` still builds and runs no test."""
    measured = measure("--experiment", "compile-only", "--targets", fixture_crate, out=tmp_path)
    equal(measured.code, 0, describing="a build-only run of the fixture crate")
    (crate,) = tasks(measured)
    contains(str(crate["command"]), 'nextest run --no-tests=pass -E "none()"', describing="it")
    equal(crate["rust"]["tests"], 0, describing="the tests it ran")


def test_a_task_and_the_report_are_timed_by_the_wall_clock(
    tmp_path: Path, fixture_project: str
) -> None:
    """A task that sleeps a second is reported at about a second, inside the report's own wall."""
    measured = measure(
        "--targets", f"{fixture_project}:pause,{fixture_project}:pause", out=tmp_path
    )
    report = mapping(measured.report, describing="the report")
    for task in report["tasks"]:
        truth(1.0 <= task["s"] < 30, describing=f"a one-second task's seconds: {task['s']}")
    truth(report["wall_s"] >= sum(task["s"] for task in report["tasks"]), describing="the wall")


def test_a_task_under_base_tree_reads_the_base_branchs_files(
    tmp_path: Path, fixture_project: str
) -> None:
    """What a task reads under `base-tree` is the merge base's copy, not the spike's."""
    measured = measure(
        "--experiment", "base-tree", "--targets", f"{fixture_project}:observe", out=tmp_path
    )
    equal(measured.code, 0, describing="the observing task's run")
    base = run(["git", "merge-base", "HEAD", "origin/main"], cwd=ROOT, check=True).stdout.strip()
    shown = run(["git", "show", f"{base}:justfile"], cwd=ROOT, check=True).stdout
    seen = (ROOT / "tools" / fixture_project / "seen").read_text(encoding="utf-8")
    equal(seen, shown, describing="the justfile the task read")
