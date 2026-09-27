"""Every proof installs a Python distribution on an interpreter that distribution admits.

Which interpreter a host offers by default is the host's business rather than
the proof's: a proof that took it would pass on a host whose default meets the
distribution's floor and fail on one whose default does not, over the same
artifact. So each host here is made to prefer the release just below the floor
— through `UV_PYTHON`, which is how a host names its default to `uv` — and the
environment the proof installed into is read back for the interpreter it holds.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from release_artifacts.build import CLIENT_REQUIRES_PYTHON, REQUIRES_PYTHON, floor
from release_artifacts.installing import install, interpreter_in
from repo_checks.expect import passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run

#: Each Python distribution a proof installs, beside the floor it declares.
DECLARED = {
    "pypi:printobserver-sdk": CLIENT_REQUIRES_PYTHON,
    "pypi:printobserver-cli": REQUIRES_PYTHON,
}

VERSION_PROBE = "import sys; print(sys.version_info.major, sys.version_info.minor)"


def _below(declaration: str) -> str:
    """The interpreter release just below a `>=X.Y` floor, as `X.Y`."""
    major, minor = floor(declaration).split(".")
    return f"{major}.{int(minor) - 1}"


@pytest.mark.parametrize("identifier", DECLARED)
def test_a_proof_installs_on_an_interpreter_the_distribution_admits_whatever_the_host_prefers(
    repo: Repo,
    program: Path,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    identifier: str,
) -> None:
    """A host preferring an interpreter below the floor still gets an environment above it."""
    declaration = DECLARED[identifier]
    monkeypatch.setenv("UV_PYTHON", _below(declaration))

    taken = install(repo, identifier, tmp_path / "taken", program)

    asked = run([str(interpreter_in(taken.environment)), "-c", VERSION_PROBE], cwd=tmp_path)
    passing(asked, describing="asking the proof's environment which interpreter it holds")
    major, minor = (int(part) for part in asked.stdout.split())
    lowest = tuple(int(part) for part in floor(declaration).split("."))
    truth(
        (major, minor) >= lowest,
        describing=(
            f"the environment {identifier} was installed into ({major}.{minor}) "
            f"to satisfy its declared `{declaration}`"
        ),
    )
