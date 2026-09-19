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
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: A program every route's artifact carries, in place of the real one. It runs,
#: and it says which version it is, which is what a route's own assertion is.
#: The version is the workspace's own rather than a number written here: release
#: automation moves that one, and a copy kept by hand is stale the first time it
#: does.
#:
#: Compiled rather than a script, because a route's artifact states what the
#: program inside it was built against, and on macOS that floor is read out of
#: the program's own Mach-O load commands — which a shell script does not have.
#: Compiled on every host, so the stand-in this host's journeys carry is built
#: the same way as the one a macOS runner's carry.
STAND_IN = """fn main() {{
    if std::env::args().nth(1).as_deref() == Some("--version") {{
        println!("printobserver {version}");
        return;
    }}
    eprintln!("printobserver: a stand-in program, which does nothing");
    std::process::exit(1);
}}
"""


def compiled_stand_in(into: Path) -> Path:
    """The stand-in program, compiled for this host into `into`.

    Compiled from the repository root so the toolchain `rust-toolchain.toml`
    pins is the one that builds it, and stripped so the artifacts carrying it
    stay small.

    Raises:
        AssertionError: If it would not compile.
    """
    into.mkdir(parents=True, exist_ok=True)
    source = into / "stand_in.rs"
    source.write_text(
        STAND_IN.format(version=targets.workspace(REPO_ROOT)["version"]), encoding="utf-8"
    )
    program = into / "printobserver"
    compiled = run(
        [
            "rustc",
            "--edition",
            "2021",
            "-C",
            "opt-level=s",
            "-C",
            "strip=symbols",
            "-C",
            "panic=abort",
            "-o",
            str(program),
            str(source),
        ],
        cwd=REPO_ROOT,
        timeout=300,
    )
    equal(
        compiled.returncode,
        0,
        describing=f"compiling the stand-in program:\n{compiled.stdout}{compiled.stderr}",
    )
    return program


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
    return Path(shutil.copy2(stand_in, tmp_path / "printobserver"))


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
    program = Path(shutil.copy2(stand_in, root / "printobserver"))
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
