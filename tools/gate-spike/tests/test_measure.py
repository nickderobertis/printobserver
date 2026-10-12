"""The harness, driven as a workflow cell drives it, over real tasks."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from conftest import ROOT, Measured, mapping, measure
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run

Tasks = Callable[[Measured], list[dict[str, object]]]


def test_every_lever_and_pseudo_target_is_listed(tmp_path: Path) -> None:
    """`--list` names every lever and pseudo-target the harness takes."""
    listed = measure("--list", out=tmp_path)
    equal(listed.code, 0, describing="`--list`'s exit")
    for name in ("baseline", "base-tree", "xdist-3", "bench:octoprint-lookup"):
        contains(listed.out, name, describing="what `--list` names")


def test_an_invocation_naming_nothing_real_is_refused_before_anything_runs(
    tmp_path: Path,
) -> None:
    """A lever, target or project that does not exist is refused with exit 2, writing no report."""
    for arguments, naming in (
        (["--experiment", "no-such-lever", "--targets", "octoprint-env:test"], "no-such-lever"),
        (["--targets", "not a task"], "not a task"),
        (["--targets", "no-such-project:test"], "no-such-project"),
        (["--targets", "*:no-such-target"], "no-such-target"),
        (["--targets", "octoprint-env:no-such-target"], "no-such-target"),
        (["--targets", ""], "no target"),
    ):
        refused = measure(*arguments, out=tmp_path / "refused")
        equal(refused.code, 2, describing=f"the exit for {arguments}")
        contains(refused.err, naming, describing=f"the refusal of {arguments}")
        truth(refused.report is None, describing=f"no report written for {arguments}")


def test_a_pytest_task_is_measured_test_by_test_and_its_failure_named(
    tmp_path: Path, fixture_project: str, tasks_of: Tasks
) -> None:
    """A pytest task is recorded test by test with what each spent, and a failed task is named."""
    measured = measure(
        "--targets", f"{fixture_project}:suite,{fixture_project}:fails", out=tmp_path
    )
    equal(measured.code, 1, describing="the exit when a task failed")
    contains(measured.err, f"{fixture_project}:fails exited 3", describing="the failure's naming")
    suite, fails = tasks_of(measured)
    equal(fails["exit"], 3, describing="the failing task's own exit")
    pytest = mapping(suite["pytest"], describing="the pytest figures")
    equal(pytest["tests"], 5, describing="the tests the plugin recorded")
    equal(pytest["outcomes"], {"failed": 1, "passed": 4}, describing="their outcomes")
    by_cost = pytest["by_cost"]
    truth(by_cost["spawn:git"]["count"] == 1, describing=f"one git child in {by_cost}")
    truth(by_cost["sleep"]["s"] >= 0.05, describing=f"the sleep's seconds in {by_cost}")
    truth(
        all(test["phases"].get("call") is not None for test in pytest["per_test"]),
        describing="every test's call phase",
    )
    own = [line for line in measured.out.splitlines() if line.startswith("measure: ")]
    equal(len(own), 1, describing="the lines the harness itself prints on standard output")


def test_xdist_workers_collect_one_list_under_stable_ids(
    tmp_path: Path, fixture_project: str, tasks_of: Tasks
) -> None:
    """Under xdist, workers that render a parameter differently still run each test once."""
    measured = measure(
        "--experiment", "xdist-2-nocov", "--targets", f"{fixture_project}:suite", out=tmp_path
    )
    (suite,) = tasks_of(measured)
    pytest = mapping(suite["pytest"], describing="the pytest figures")
    ids = [test["nodeid"] for test in pytest["per_test"]]
    equal(len(ids), len(set(ids)), describing="each test recorded once")
    equal(len(ids), 5, describing="every test run")
    equal(
        sorted({session["worker"] for session in pytest["sessions"]}),
        ["gw0", "gw1", "main"],
        describing="the processes recorded",
    )
    contains(str(suite["command"]), "-n 2 --dist load", describing="the rewritten command")


def test_the_base_tree_is_put_back_after_the_run(tmp_path: Path, fixture_project: str) -> None:
    """`base-tree` leaves the spike's own files exactly as committed once the run ends."""
    changed = (".github/workflows/ci.yml", "justfile", ".gitignore")
    before = {path: (ROOT / path).read_bytes() for path in changed}
    measured = measure(
        "--experiment", "base-tree", "--targets", f"{fixture_project}:fails", out=tmp_path
    )
    equal(measured.code, 1, describing="the exit of a run whose one task fails")
    for path in changed:
        equal((ROOT / path).read_bytes(), before[path], describing=f"{path} after the run")
    status = run(["git", "status", "--porcelain", "--", *changed], cwd=ROOT, check=True)
    equal(status.stdout, "", describing="the spike's own files, as committed")


def test_a_nextest_task_is_read_into_compile_run_and_per_test_figures(
    tmp_path: Path, tasks_of: Tasks
) -> None:
    """A crate's covered nextest run is read into its compile, run and per-test figures."""
    measured = measure("--targets", "printobserver-vision-api:test", out=tmp_path)
    equal(measured.code, 0, describing="a passing crate's run")
    (crate,) = tasks_of(measured)
    rust = mapping(crate["rust"], describing="the rust figures")
    truth(rust["tests"] > 0 and rust["binaries"] > 0, describing=f"the counts in {rust}")
    equal(len(rust["per_test"]), rust["tests"], describing="a duration for every test")
    truth(rust["run_s"] is not None, describing="nextest's own run time")
