"""A lane started in the background and waited out by a later step, run for real.

`ci.yml`'s `macos` job starts `just test-integration LANE` in one step, runs the
gate, and answers the lane in a later step with `just test-integration-wait
LANE`; both recipes are this module's `start` and `wait`. What each of those
steps sees is driven here over a real detached process: the lane's whole
output, and its own exit status as the step's, whatever it was.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest
from repo_checks import lanes
from repo_checks.expect import contains, equal, truth
from treecopy import REPO_ROOT

#: What `just` exports to every recipe, so the lane's own process finds its package.
PACKAGES = str(REPO_ROOT / "tools/repo-checks/src")


@pytest.fixture(autouse=True)
def packages(monkeypatch: pytest.MonkeyPatch) -> None:
    """The package path the recipes export, for the detached process a lane starts."""
    monkeypatch.setenv("PYTHONPATH", PACKAGES)


def _python(source: str) -> list[str]:
    """A command running `source` under this interpreter: a lane with a known output and exit."""
    return [sys.executable, "-c", source]


@pytest.mark.parametrize("status", [0, 9])
def test_a_started_lane_is_answered_with_its_output_and_its_status(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], status: int
) -> None:
    """The step that waits answers what the lane printed and the status it exited with."""
    lane = tmp_path / "lane"
    lane.mkdir()
    (lane / "exit").write_text("0", encoding="utf-8")
    source = f"import sys, time; time.sleep(1); print('tier ran'); sys.exit({status})"

    equal(lanes.main(["start", str(lane), *_python(source)]), 0)
    truth(not (lane / "exit").exists(), describing="a previous lane's verdict, cleared")

    equal(lanes.wait(lane, poll=0.2), status)
    contains(capsys.readouterr().out, "tier ran")


def test_the_lane_s_status_is_written_after_its_output_and_whole(tmp_path: Path) -> None:
    """`run` keeps the output first, and the status by a rename, so a waiter never reads half."""
    lane = tmp_path / "lane"
    lane.mkdir()

    code = lanes.main(["run", str(lane), *_python("import sys; print('out'); sys.exit(3)")])

    equal(code, 3)
    equal((lane / "exit").read_text(encoding="utf-8"), "3")
    contains((lane / "log").read_text(encoding="utf-8"), "out")
    equal(sorted(path.name for path in lane.iterdir()), ["exit", "log"])


def test_waiting_on_a_lane_nobody_started_is_refused(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """With no lane there, the waiting step fails at once rather than waiting forever."""
    equal(lanes.main(["wait", str(tmp_path / "never")]), lanes.USAGE)

    contains(capsys.readouterr().err, "no lane was started under")


@pytest.mark.parametrize("arguments", [[], ["start", "lane"], ["wait"], ["stop", "lane"]])
def test_an_invocation_naming_no_lane_or_command_is_refused(
    arguments: list[str], capsys: pytest.CaptureFixture[str]
) -> None:
    """Every verb needs its lane, and starting one needs its command."""
    equal(lanes.main(arguments), lanes.USAGE)

    contains(capsys.readouterr().err, "usage:")
