"""The harness, driven as a workflow cell drives it, over real tasks."""

from __future__ import annotations

import os
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from conftest import CASES_FAILING, CASES_TESTS, ROOT, mapping, measure, tasks
from levers import LEVERS, SPIKE_CHANGED, pytest_temporaries
from measure import PSEUDO_TARGETS
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run

VISION = "printobserver-vision-api:test"


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
    ):
        refused = measure(*arguments, out=tmp_path / "refused")
        equal(refused.code, 2, describing=f"the exit for {arguments}")
        contains(refused.err, naming, describing=f"the refusal of {arguments}")
        truth(refused.report is None, describing=f"no report written for {arguments}")


def test_a_pytest_task_is_measured_test_by_test_and_its_failure_named(
    tmp_path: Path, fixture_project: str
) -> None:
    """A pytest task is recorded test by test with what each spent, and a failed task named."""
    before = pytest_temporaries()
    measured = measure(
        "--targets", f"{fixture_project}:suite,{fixture_project}:fails", out=tmp_path
    )
    equal(measured.code, 1, describing="the exit when a task failed")
    contains(measured.err, f"{fixture_project}:fails exited 3", describing="the failure's naming")
    suite, fails = tasks(measured)
    equal(fails["exit"], 3, describing="the failing task's own exit")
    pytest_run = mapping(suite["pytest"], describing="the suite's figures")
    equal(pytest_run["tests"], CASES_TESTS, describing="the tests the plugin recorded")
    equal(
        pytest_run["outcomes"],
        {"failed": CASES_FAILING, "passed": CASES_TESTS - CASES_FAILING},
        describing="their outcomes",
    )
    by_cost = pytest_run["by_cost"]
    for cost, count in (("spawn:git", 1), ("copytree", 1), ("rmtree", 1), ("http", 1)):
        equal(by_cost[cost]["count"], count, describing=f"`{cost}` in {by_cost}")
    truth(by_cost["sleep"]["s"] >= 0.05, describing=f"the sleep's seconds in {by_cost}")
    (session,) = pytest_run["sessions"]
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
    for kind in (
        "tier:confirming::the_cross_product_walk",
        "world:supervisor-start-to-announcement",
    ):
        truth(kind in kinds, describing=f"`{kind}` among {sorted(kinds)}")
    observed = "traced:etw-tracerpt" if sys.platform == "win32" else "traced:strace-run"
    if sys.platform != "darwin":
        truth(kinds[observed]["count"] > 0, describing=f"`{observed}` in {sorted(kinds)}")


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
