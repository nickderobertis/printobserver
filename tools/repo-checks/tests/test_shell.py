"""The one place this repository starts a subprocess.

Every check, command and test helper runs programs through `run`, so its
contract is worth driving directly: it resolves the executable against PATH
before running it, and it reports a program that is not there rather than
raising out of a caller that was going to report the failure itself.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.
# ruff: noqa: S101

from __future__ import annotations

from pathlib import Path

import pytest
from repo_checks.shell import PROGRAM_NOT_FOUND, run

ABSENT = "a-program-this-repository-does-not-install"


def test_a_program_is_run_by_absolute_path(tmp_path: Path) -> None:
    """The executable is resolved against PATH, which is what fixes S607."""
    result = run(["git", "rev-parse", "--is-inside-work-tree"], cwd=tmp_path)

    assert result.args[0].startswith("/"), result.args
    assert result.args[0].endswith("/git"), result.args


def test_output_comes_back_captured() -> None:
    """Callers read what a program said, so the default captures both streams."""
    result = run(["git", "--version"])

    assert result.returncode == 0
    assert "git version" in result.stdout


def test_a_missing_program_reports_rather_than_raising() -> None:
    """A caller that reports its own failure gets a status it can act on."""
    result = run([ABSENT, "--version"])

    assert result.returncode == PROGRAM_NOT_FOUND
    assert ABSENT in result.stderr


def test_a_missing_program_raises_where_the_caller_asked_it_to() -> None:
    """`check` means the caller has no failure path of its own."""
    with pytest.raises(FileNotFoundError, match=ABSENT):
        run([ABSENT], check=True)


def test_output_can_be_left_to_the_terminal() -> None:
    """A long install's own progress is what a reader needs, so it is not captured."""
    result = run(["git", "--version"], capture=False)

    assert result.returncode == 0
    assert result.stdout is None
