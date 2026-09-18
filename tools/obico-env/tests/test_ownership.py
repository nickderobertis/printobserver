"""Handing the state directory back, answered the way each host family owns files.

On Linux the bring-down hands everything Obico's root containers wrote back to
the user who ran it, from inside the running `web` container; the live journey
in `test_environment_journey.py` proves that over a real stack. On Windows a file
carries no numeric owner and there is nobody to hand anything back to, so the
answer there is to run nothing and say so.

These need no stack. What they record is the one command the hand-back would
run, in place of running it, so the platform branch and the command line are the
script's own and nothing reaches Docker.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import obico_env
import pytest
from obico_env import Stack, hand_back_ownership
from repo_checks.expect import contains, equal, truth


@pytest.fixture
def commands(monkeypatch: pytest.MonkeyPatch) -> list[list[str]]:
    """Every command the hand-back asked to run, each answered as a success.

    Returns:
        The commands, in order.
    """
    asked: list[list[str]] = []

    def record(argv: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        asked.append(argv)
        return subprocess.CompletedProcess(argv, 0, "", "")

    monkeypatch.setattr(obico_env, "run_program", record)
    return asked


def test_on_windows_nothing_is_handed_back_and_it_says_why(
    tmp_path: Path,
    commands: list[list[str]],
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """No `chown` reaches a container, and the bring-down is not held up for it."""
    monkeypatch.setattr(sys, "platform", "win32")

    handed = hand_back_ownership(Stack(tmp_path / "state"))

    truth(handed, describing="the hand-back on Windows to be reported as done")
    equal(commands, [], describing="the commands the hand-back ran on Windows")
    contains(
        capsys.readouterr().err,
        "nothing a container wrote needs handing back",
        describing="what the bring-down said on Windows",
    )


def test_on_linux_what_the_containers_wrote_is_handed_to_this_user(
    tmp_path: Path, commands: list[list[str]]
) -> None:
    """One `chown`, from inside `web`, to the owner of a file this user just wrote.

    Like the tier it belongs to, this runs on Linux: Obico's containers are
    Linux containers, and the scheduled job that runs this suite is a Linux one.
    """
    written = tmp_path / "written-by-this-user"
    written.write_text("", encoding="utf-8")
    owner = written.stat()

    handed = hand_back_ownership(Stack(tmp_path / "state"))

    truth(handed, describing="the hand-back to be reported as done")
    equal(len(commands), 1, describing="the commands the hand-back ran")
    equal(
        commands[0][-6:],
        ["web", "chown", "-R", f"{owner.st_uid}:{owner.st_gid}", "/app", "/frontend"],
        describing="the command the hand-back ran",
    )
