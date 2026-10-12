"""Fixtures that run the spike's harness the way a cell of its workflow does.

Every test drives `measure.py` as a program over real tasks: tasks of a project
this fixture declares for the length of one test, so a task that fails, and a
task whose pytest spawns, sleeps, copies, asks a server and fails, are real ones
rather than stand-ins.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any, cast

import pytest
from repo_checks.expect import truth
from repo_checks.shell import run

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
PYTHONPATH = os.pathsep.join(
    [
        str(HERE),
        str(ROOT / "tools/repo-checks/src"),
        str(ROOT / "tools/octoprint-env"),
        os.environ.get("PYTHONPATH", ""),
    ]
)

#: A suite whose tests each spend their time on one thing the plugin attributes,
#: one child process spawned at import, outside any test, and one failure.
CASES = """
import http.server
import shutil
import subprocess
import sys
import threading
import time
import urllib.request

import pytest

subprocess.run(["git", "--version"], check=True, capture_output=True)


@pytest.fixture
def breaks_in_setup():
    raise RuntimeError("this fixture fails on purpose")


@pytest.fixture
def breaks_in_teardown():
    yield
    raise RuntimeError("this fixture fails on purpose")


def test_spawns_git():
    subprocess.run(["git", "--version"], check=True, capture_output=True)
    subprocess.run([sys.executable, "-c", "import time; time.sleep(0.1)"], check=True)


def test_waits_on_a_child_by_polling_alone():
    child = subprocess.Popen(["git", "--version"], stdout=subprocess.DEVNULL)
    while child.poll() is None:
        pass


def test_a_child_that_never_starts_is_charged_to_nobody():
    with pytest.raises(OSError):
        subprocess.run(["spike-no-such-program-anywhere"], check=True)


def test_sleeps():
    time.sleep(0.05)


def test_writes_a_large_temporary(tmp_path):
    (tmp_path / "large").write_bytes(bytes(40_000_000))


def test_a_sleep_that_raises_is_still_timed():
    with pytest.raises(TypeError):
        time.sleep("not a duration")


def test_copies_and_removes_a_tree(tmp_path):
    (tmp_path / "from").mkdir()
    (tmp_path / "from" / "nested").mkdir()
    (tmp_path / "from" / "nested" / "file").write_text("x")
    shutil.copytree(tmp_path / "from", tmp_path / "to")
    shutil.rmtree(tmp_path / "to")


def test_copies_one_file(tmp_path):
    (tmp_path / "file").write_text("x")
    shutil.copy2(tmp_path / "file", tmp_path / "copy")


def test_asks_a_local_server():
    handler = http.server.SimpleHTTPRequestHandler
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        urllib.request.urlopen(f"http://127.0.0.1:{server.server_port}/").read()
    finally:
        server.shutdown()


@pytest.mark.parametrize("value", [time.time_ns(), time.time_ns() + 1])
def test_a_parameter_rendered_differently_by_each_process(value):
    pass


def test_skipped():
    pytest.skip("skipped on purpose")


def test_fails():
    raise AssertionError("this case fails on purpose")


def test_fails_in_setup(breaks_in_setup):
    pass


def test_fails_in_teardown(breaks_in_teardown):
    pass
"""

#: How `CASES`' tests come out.
CASES_OUTCOMES = {"failed": 3, "passed": 11, "skipped": 1}
CASES_TESTS = sum(CASES_OUTCOMES.values())


@dataclass(frozen=True)
class Measured:
    """One run of the harness: its exit, what it printed, and the report it wrote."""

    code: int
    out: str
    err: str
    report: dict[str, Any] | None


def measure(*arguments: str, out: Path, environment: dict[str, str] | None = None) -> Measured:
    """Run `measure.py` as the `spike-measure` recipe does, into `out`."""
    ran = run(
        [sys.executable, str(HERE / "measure.py"), "--out", str(out), *arguments],
        cwd=ROOT,
        env={**os.environ, "PYTHONPATH": PYTHONPATH, **(environment or {})},
    )
    written = out / "measure.json"
    report = json.loads(written.read_text(encoding="utf-8")) if written.is_file() else None
    return Measured(ran.returncode, ran.stdout, ran.stderr, report)


@pytest.fixture
def fixture_project() -> Iterator[str]:
    """A project declared in the tree for one test, named by what this returns."""
    name = f"spike-fixture-{os.getpid()}"
    root = ROOT / "tools" / name
    (root / "cases").mkdir(parents=True)
    (root / "cases" / "test_cases.py").write_text(CASES, encoding="utf-8")
    cases = f"tools/{name}/cases -p no:cacheprovider"
    observing = f"\"import shutil; shutil.copy('justfile', 'tools/{name}/seen')\""
    manifest = {
        "name": name,
        "root": f"tools/{name}",
        "targets": {
            "fails": {"command": "exit 3"},
            "suite": {"command": f"uv run -q pytest {cases}"},
            "covered": {"command": f"uv run -q coverage run -m pytest {cases}"},
            "pause": {"command": 'python -c "import time; time.sleep(1)"'},
            "observe": {"command": f"python -c {observing}"},
            "here": {
                "command": "python -c \"import pathlib; pathlib.Path('ran-here').touch()\"",
                "options": {"cwd": f"tools/{name}/cases"},
            },
        },
    }
    (root / "project.json").write_text(json.dumps(manifest), encoding="utf-8")
    try:
        yield name
    finally:
        shutil.rmtree(root)


#: A crate whose four tests end each way the harness reads apart.
FIXTURE_CRATE = """
#[test]
fn passes() {}

#[test]
fn fails() {
    panic!("on purpose");
}

#[test]
fn times_out() {
    std::thread::sleep(std::time::Duration::from_secs(30));
}

#[test]
fn aborts() {
    std::process::abort();
}
"""


@pytest.fixture
def fixture_crate(tmp_path_factory: pytest.TempPathFactory) -> Iterator[str]:
    """A project whose one task runs nextest over a crate whose tests end four ways.

    The crate is outside the repository, so it joins no workspace; the project
    declaring the task is in the tree for the test's length.
    """
    crate = tmp_path_factory.mktemp("crate") / "fixture"
    run(["cargo", "new", "--quiet", "--lib", "--vcs", "none", str(crate)], check=True)
    (crate / "src" / "lib.rs").write_text(
        FIXTURE_CRATE,
        encoding="utf-8",
    )
    # nextest terminates a test still running two one-second periods in.
    (crate / ".config").mkdir()
    (crate / ".config" / "nextest.toml").write_text(
        '[profile.default]\nslow-timeout = { period = "1s", terminate-after = 2 }\n',
        encoding="utf-8",
    )
    name = f"spike-crate-{os.getpid()}"
    root = ROOT / "tools" / name
    root.mkdir()
    manifest = crate.joinpath("Cargo.toml").as_posix()
    command = f'cargo nextest run --manifest-path "{manifest}" --no-fail-fast'
    target = {"name": name, "root": f"tools/{name}", "targets": {"test": {"command": command}}}
    (root / "project.json").write_text(json.dumps(target), encoding="utf-8")
    try:
        yield f"{name}:test"
    finally:
        shutil.rmtree(root)


@pytest.fixture
def coverage_data() -> Iterator[Callable[[], set[Path]]]:
    """The coverage data files a test wrote into the repository root, removed after it."""
    before = set(ROOT.glob(".coverage*"))

    def written() -> set[Path]:
        return set(ROOT.glob(".coverage*")) - before

    yield written
    for path in written():
        path.unlink()


def mapping(value: object, *, describing: str) -> dict[str, Any]:
    """`value` as the JSON object a report holds there, refusing anything else."""
    truth(isinstance(value, dict), describing=f"{describing} as an object, not {value!r}")
    return cast("dict[str, Any]", value)


def tasks(measured: Measured) -> list[dict[str, Any]]:
    """The task results a report carries."""
    report = mapping(measured.report, describing="the report")
    return [mapping(task, describing="a task") for task in report["tasks"]]
