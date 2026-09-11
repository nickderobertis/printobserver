"""What the artifact journeys are driven against.

Nothing is mocked. The real tool assembles real wheels and real packages from
the committed tree; what stands in for the `printobserver` program is a small
one of this suite's own, because what these journeys are about is the artifact
around it rather than the program inside it.
"""

from __future__ import annotations

import shutil
from collections.abc import Callable
from pathlib import Path

import pytest
from release_artifacts import targets
from release_artifacts.__main__ import main
from repo_checks.expect import equal
from repo_checks.model import Repo

REPO_ROOT = Path(__file__).resolve().parents[3]

#: A program every route's artifact carries, in place of the real one. It runs,
#: and it says which version it is, which is what a route's own assertion is.
#: The version is the workspace's own rather than a number written here: release
#: automation moves that one, and a copy kept by hand is stale the first time it
#: does.
STAND_IN = """#!/bin/sh
if [ "${{1:-}}" = "--version" ]; then
    echo "printobserver {version}"
    exit 0
fi
echo "printobserver: a stand-in program, which does nothing" >&2
exit 1
"""


@pytest.fixture
def repo() -> Repo:
    """The committed tree the tool builds from."""
    return Repo(REPO_ROOT)


@pytest.fixture
def version() -> str:
    """The version the workspace declares, which is the one every artifact carries."""
    return targets.workspace(REPO_ROOT)["version"]


@pytest.fixture
def program(tmp_path: Path, version: str) -> Path:
    """A runnable program a route's artifact can carry, reporting the tree's version."""
    path = tmp_path / "printobserver"
    path.write_text(STAND_IN.format(version=version), encoding="utf-8")
    path.chmod(0o755)
    return path


@pytest.fixture
def into(tmp_path: Path) -> Callable[[str], Path]:
    """A factory for directories the tool writes artifacts into."""

    def make(name: str) -> Path:
        made = tmp_path / name
        made.mkdir(parents=True, exist_ok=True)
        return made

    return make


@pytest.fixture(scope="module")
def built(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Every artifact the tool assembles from the committed tree, built once per module.

    What `just build-artifacts` leaves in `dist`, carrying the stand-in
    program: the two wheels, the four packages, the release tarball and its
    checksum file. Built once because the build compiles the Node client and
    packages the Rust one, and a journey that publishes it takes a copy.
    """
    root = tmp_path_factory.mktemp("built")
    program = root / "printobserver"
    program.write_text(
        STAND_IN.format(version=targets.workspace(REPO_ROOT)["version"]), encoding="utf-8"
    )
    program.chmod(0o755)
    dist = root / "dist"
    equal(
        main(
            ["build-all", "--root", str(REPO_ROOT), "--into", str(dist), "--binary", str(program)]
        ),
        0,
        describing="building every artifact from the committed tree",
    )
    return dist


@pytest.fixture
def dist(built: Path, tmp_path: Path) -> Path:
    """A copy of what the build left, for a journey that writes beside it."""
    return Path(shutil.copytree(built, tmp_path / "dist"))
