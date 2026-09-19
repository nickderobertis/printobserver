"""The stand-in program, in both forms a host runs one.

The shell form is what every route proof on a POSIX host carries and is
exercised by every one of them. The compiled form is what a Windows host
carries, and a Windows host is not this one — so it is asked for here by
name, built with this tree's own toolchain, and run: a stand-in that compiles
on the host that runs the suite most often is one a Windows runner meets
already proven to answer.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from release_artifacts.stand_in import StandInError, stand_in_program
from repo_checks.expect import contains, equal, failing, passing
from repo_checks.model import Repo
from repo_checks.shell import run


def _said(program: Path, *arguments: str) -> tuple[int, str]:
    result = run([str(program), *arguments], cwd=program.parent, timeout=60)
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def test_the_compiled_form_answers_its_version_and_refuses_anything_else(
    repo: Repo, tmp_path: Path
) -> None:
    """What a Windows host installs is a real program, and it says what it was told to."""
    program = stand_in_program(
        repo, tmp_path / "printobserver.exe", "printobserver 1.2.3 compiled", compiled=True
    )

    code, said = _said(program, "--version")
    passing((code, said), describing="the compiled stand-in asked its version")
    equal(said.strip(), "printobserver 1.2.3 compiled")
    failing(_said(program, "status"), naming="does nothing else")


def test_the_compiled_broken_form_installs_and_does_not_run(repo: Repo, tmp_path: Path) -> None:
    """The artifact a registry serves broken looks the same from outside on every host."""
    program = stand_in_program(
        repo, tmp_path / "printobserver.exe", "unused", broken=True, compiled=True
    )

    failing(_said(program, "--version"), naming="cannot run on this host")


def test_one_answer_is_compiled_once_and_copied_after(repo: Repo, tmp_path: Path) -> None:
    """A suite asking for the same program in many places pays for one build."""
    first = stand_in_program(repo, tmp_path / "one" / "p.exe", "printobserver 7.7.7", compiled=True)
    second = stand_in_program(
        repo, tmp_path / "two" / "p.exe", "printobserver 7.7.7", compiled=True
    )

    equal(first.read_bytes(), second.read_bytes(), describing="the two copies")
    contains(_said(second, "--version")[1], "7.7.7", describing="the copy's own answer")


def test_an_answer_that_closes_its_own_literal_is_refused(repo: Repo, tmp_path: Path) -> None:
    """A program that compiled to something else would answer something else."""
    with pytest.raises(StandInError, match="closes the literal"):
        stand_in_program(repo, tmp_path / "p", 'printobserver "#', compiled=True)
