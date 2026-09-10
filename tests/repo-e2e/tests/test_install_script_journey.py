"""The bundled install script, driven the way the install path's third route drives it.

Nothing here is mocked and nothing here reaches the network. A release
directory is staged in the shape release automation publishes — two releases,
each carrying this host's own platform artifact and the checksum file the
script verifies against — and the committed script is run against it with `sh`,
exactly as that route's own one-line command runs it.

Two releases rather than one, and the older one is a stand-in program that
reports a version of its own: what the selection assertions are about is which
release the script chose, and two artifacts carrying the same program could not
tell a reader that.
"""

from __future__ import annotations

import gzip
import hashlib
import io
import os
import tarfile
import tomllib
from pathlib import Path

import pytest
from journey import REPO_ROOT, clean_environment, run
from repo_checks.expect import contains, failing, passing, truth
from repo_checks.shell import run as shell_run

#: The committed script the third route fetches and runs.
SCRIPT = "scripts/install.sh"

#: The program every route puts on a path.
PROGRAM = "printobserver"

#: The file a release publishes its digests in.
CHECKSUMS = "SHA256SUMS"

#: The release the staged directory's older tag carries, and what its
#: stand-in program reports.
OLDER = "v0.0.1"

#: Where this host's own artifact is named, as the script names it.
PLATFORM = {"x86_64": "linux-x86_64", "aarch64": "linux-aarch64"}[os.uname().machine]

#: How long the program build is given the first time this tier runs.
BUILD_TIMEOUT_SECONDS = 2400


def _version() -> str:
    """The version release automation wrote into the workspace."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["version"])


def _program() -> Path:
    """The `printobserver` program this repository builds, built once."""
    built = REPO_ROOT / "target/release" / PROGRAM
    if not built.is_file():
        passing(
            run(
                ["cargo", "build", "--release", "--locked", "-p", PROGRAM],
                REPO_ROOT,
                timeout=BUILD_TIMEOUT_SECONDS,
            ),
            describing="building the program a release carries",
        )
    return built


def _artifact(into: Path, program: bytes) -> Path:
    """One release artifact, in the shape release automation publishes it."""
    into.mkdir(parents=True, exist_ok=True)
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        info = tarfile.TarInfo(PROGRAM)
        info.size = len(program)
        info.mode = 0o755
        archive.addfile(info, io.BytesIO(program))
    target = into / f"{PROGRAM}-{PLATFORM}.tar.gz"
    target.write_bytes(gzip.compress(raw.getvalue(), mtime=0))
    (into / CHECKSUMS).write_text(
        f"{hashlib.sha256(target.read_bytes()).hexdigest()}  {target.name}\n", encoding="utf-8"
    )
    return target


@pytest.fixture
def staged(tmp_path: Path) -> Path:
    """A release directory holding two releases, in the shape the forge serves.

    `latest/download` is what the forge serves for the newest release and
    `download/<tag>` for a pinned one, so a script proven against this is
    proven against the layout it will meet.
    """
    base = tmp_path / "releases"
    real = _program().read_bytes()
    stand_in = f'#!/bin/sh\necho "{PROGRAM} {OLDER.removeprefix("v")}"\n'.encode()

    _artifact(base / "download" / OLDER, stand_in)
    _artifact(base / "download" / f"v{_version()}", real)
    _artifact(base / "latest" / "download", real)
    return base


def _script(staged: Path, home: Path, *arguments: str) -> tuple[int, str]:
    """Run the script under a home of this journey's own, against a staged release."""
    environment = clean_environment(HOME=str(home), PRINTOBSERVER_RELEASE_BASE=str(staged))
    result = shell_run(
        ["sh", str(REPO_ROOT / SCRIPT), *arguments],
        cwd=home,
        env=environment,
        timeout=600,
    )
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def _reports(program: Path) -> str:
    """What one installed program says its own version is."""
    result = run([str(program), "--version"], program.parent, timeout=120)
    passing(result, describing=f"{program} reporting its own version")
    return (result.stdout + result.stderr).strip()


def test_it_installs_the_newest_release_into_the_default_directory(
    staged: Path, tmp_path: Path
) -> None:
    """Run with neither a pinned release nor a directory, it takes the newest."""
    home = tmp_path / "home-default"
    home.mkdir()

    code, said = _script(staged, home)

    passing((code, said), describing="the install script with no options")
    installed = home / ".local/bin" / PROGRAM
    truth(installed.is_file(), describing=f"{installed} to be the program it installed")
    contains(_reports(installed), _version(), describing="what the installed program reports")


def test_a_pinned_release_is_the_one_installed(staged: Path, tmp_path: Path) -> None:
    """`--version` takes that release rather than the newest."""
    home = tmp_path / "home-pinned"
    home.mkdir()

    code, said = _script(staged, home, "--version", OLDER)

    passing((code, said), describing=f"the install script pinned to {OLDER}")
    installed = home / ".local/bin" / PROGRAM
    contains(_reports(installed), OLDER.removeprefix("v"), describing="the pinned program")


def test_an_install_directory_is_the_one_installed_into(staged: Path, tmp_path: Path) -> None:
    """`--to` installs there, and leaves the default directory alone."""
    home = tmp_path / "home-elsewhere"
    elsewhere = home / "opt/bin"
    home.mkdir()

    code, said = _script(staged, home, "--to", str(elsewhere))

    passing((code, said), describing="the install script with an install directory")
    truth((elsewhere / PROGRAM).is_file(), describing=f"{elsewhere / PROGRAM} to be installed")
    truth(
        not (home / ".local/bin" / PROGRAM).exists(),
        describing="the default directory to be left alone",
    )


def test_an_altered_artifact_is_refused_before_anything_reaches_a_path(
    staged: Path, tmp_path: Path
) -> None:
    """Verification happens before anything reaches a path, not after."""
    home = tmp_path / "home-altered"
    into = home / "bin"
    home.mkdir()
    altered = staged / "latest/download" / f"{PROGRAM}-{PLATFORM}.tar.gz"
    altered.write_bytes(altered.read_bytes() + b"an alteration nobody published")

    code, said = _script(staged, home, "--to", str(into))

    failing((code, said), naming="does not match the digest")
    truth(
        not (into / PROGRAM).exists(),
        describing="nothing to have been installed at the directory it was given",
    )
    contains(said, "Nothing was installed", describing="what the refusal said")


def test_a_platform_it_publishes_nothing_for_stops_with_a_next_action(
    staged: Path, tmp_path: Path
) -> None:
    """A platform that cannot be resolved is a stop rather than a guess."""
    home = tmp_path / "home-platform"
    home.mkdir()
    shims = home / "shims"
    shims.mkdir()
    (shims / "uname").write_text(
        '#!/bin/sh\ncase "$1" in\n  -s) echo Haiku ;;\n  -m) echo m68k ;;\n'
        "  *) echo Haiku ;;\nesac\n",
        encoding="utf-8",
    )
    (shims / "uname").chmod(0o755)
    environment = clean_environment(
        HOME=str(home),
        PRINTOBSERVER_RELEASE_BASE=str(staged),
        PATH=f"{shims}{os.pathsep}{os.environ['PATH']}",
    )
    result = shell_run(["sh", str(REPO_ROOT / SCRIPT)], cwd=home, env=environment, timeout=600)
    said = (result.stdout or "") + (result.stderr or "")

    failing((result.returncode, said), naming="publishes no program for")
    contains(said, "build it from source", describing="the next action it named")


def test_a_download_that_cannot_be_obtained_stops_with_a_next_action(
    staged: Path, tmp_path: Path
) -> None:
    """A release with nothing to download is a stop rather than a silent success."""
    home = tmp_path / "home-missing"
    home.mkdir()

    code, said = _script(staged, home, "--version", "v9.9.9")

    failing((code, said), naming="has no")
    contains(said, "--version", describing="the next action it named")
    truth(
        not (home / ".local/bin" / PROGRAM).exists(),
        describing="nothing to have been installed",
    )


def test_a_release_publishing_no_digests_is_refused(staged: Path, tmp_path: Path) -> None:
    """What cannot be verified is not installed."""
    home = tmp_path / "home-nodigest"
    home.mkdir()
    (staged / "latest/download" / CHECKSUMS).unlink()

    code, said = _script(staged, home)

    failing((code, said), naming="cannot be verified")
    contains(said, "Nothing was installed", describing="what the refusal said")


def test_the_script_is_committed_where_its_own_fetch_url_names() -> None:
    """The one-line command that route states fetches a path this repository has."""
    truth((REPO_ROOT / SCRIPT).is_file(), describing=f"{SCRIPT} to be committed")
    contains(
        (REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"),
        f"main/{SCRIPT} | sh",
        describing="the install path's third route",
    )
