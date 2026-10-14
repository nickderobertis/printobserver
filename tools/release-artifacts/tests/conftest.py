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
from release_artifacts.stand_in import stand_in_program
from repo_checks import platforms
from repo_checks.expect import equal
from repo_checks.model import Repo

REPO_ROOT = Path(__file__).resolve().parents[3]

#: What the stand-in program every route's artifact carries, in place of the
#: real one, answers `--version` with. It runs, and it says which version it
#: is, which is what a route's own assertion is. The version is the workspace's
#: own rather than a number written here: release automation moves that one,
#: and a copy kept by hand is stale the first time it does.
ANSWER = "printobserver {version}"


def compiled_stand_in(into: Path) -> Path:
    """The stand-in program, compiled for this host into `into`.

    The compiled form from `release_artifacts.stand_in`, on every host — a
    route's artifact states what the program inside it was built against, and
    on macOS that floor is read out of the program's own load commands, which
    a shell script does not have — under the name this host calls the program
    by, `printobserver.exe` on Windows.
    """
    repo = Repo(REPO_ROOT)
    version = targets.workspace(REPO_ROOT)["version"]
    return stand_in_program(
        repo, into / platforms.host(repo).program, ANSWER.format(version=version), compiled=True
    )


@pytest.fixture(scope="session")
def stand_in(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """The stand-in program, compiled once for the whole session."""
    return compiled_stand_in(tmp_path_factory.mktemp("stand-in"))


@pytest.fixture
def repo() -> Repo:
    """The committed tree the tool builds from."""
    return Repo(REPO_ROOT)


@pytest.fixture
def version() -> str:
    """The version the workspace declares, which is the one every artifact carries."""
    return targets.workspace(REPO_ROOT)["version"]


@pytest.fixture
def program(tmp_path: Path, stand_in: Path) -> Path:
    """A runnable program a route's artifact can carry, reporting the tree's version."""
    return Path(shutil.copy2(stand_in, tmp_path / stand_in.name))


@pytest.fixture
def into(tmp_path: Path) -> Callable[[str], Path]:
    """A factory for directories the tool writes artifacts into."""

    def make(name: str) -> Path:
        made = tmp_path / name
        made.mkdir(parents=True, exist_ok=True)
        return made

    return make


@pytest.fixture(scope="module")
def built(tmp_path_factory: pytest.TempPathFactory, stand_in: Path) -> Path:
    """Every artifact the tool assembles from the committed tree, built once per module.

    What `just build-artifacts` leaves in `dist`, carrying the stand-in
    program: the two wheels, the four packages, the release tarball and its
    checksum file. Built once because the build compiles the Node client and
    packages the Rust one, and a journey that publishes it takes a copy.
    """
    root = tmp_path_factory.mktemp("built")
    program = Path(shutil.copy2(stand_in, root / stand_in.name))
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
