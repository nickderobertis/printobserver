"""`install-gh`: one exact GitHub CLI release, verified before it reaches PATH.

The release is served by a stand-in over real HTTP on loopback, laid out the
way the forge lays one out — the host's archive and the release's checksums
file side by side — so what is proven is the installer's own download, digest
check, extraction and placement. The program in the archive is a script
answering `--version`, and the installed copy is run to prove it is the one the
archive carried.
"""

from __future__ import annotations

import hashlib
import io
import platform
import sys
import tarfile
import zipfile
from collections.abc import Iterator
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import absent, contains, equal, refused, truth
from repo_checks.gh_release import Archive, archive_for, install, install_gh
from repo_checks.shell import run
from repo_checks.verified_download import InstallerError
from standin_release import Release
from standin_release import serving as serve

VERSION = "2.100.0"


def _program() -> bytes:
    """A `gh` that answers `--version` the way the real one does."""
    return f"#!{sys.executable}\nprint('gh version {VERSION} (a release stand-in)')\n".encode()


def _archive_bytes(archive: Archive) -> bytes:
    """The archive the release would publish for one host, carrying the stand-in."""
    member = f"{archive.stem}/bin/gh"
    buffer = io.BytesIO()
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(buffer, "w") as bundle:
            bundle.writestr(member, _program())
    else:
        with tarfile.open(fileobj=buffer, mode="w:gz") as bundle:
            payload = _program()
            info = tarfile.TarInfo(member)
            info.size = len(payload)
            info.mode = 0o755
            bundle.addfile(info, io.BytesIO(payload))
    return buffer.getvalue()


@pytest.fixture
def serving() -> Iterator[tuple[str, Release]]:
    """A loopback server answering the release's files under `/v<version>/`."""
    release = Release()
    with serve(f"/v{VERSION}/", release) as base:
        yield base, release


def _publish(release: Release, archive: Archive, *, listed: str | None = None) -> None:
    """Put the host's archive and the release's checksums file on the stand-in."""
    payload = _archive_bytes(archive)
    digest = listed if listed is not None else hashlib.sha256(payload).hexdigest()
    release.files[archive.name] = payload
    release.files[archive.checksums] = (
        f"{'0' * 64}  gh_{VERSION}_windows_amd64.zip\n{digest}  {archive.name}\n"
    ).encode()


@pytest.mark.skipif(sys.platform == "win32", reason="the archive's program is a script")
@pytest.mark.parametrize(("system", "machine"), [("linux", "x86_64"), ("darwin", "arm64")])
def test_a_verified_release_is_installed_and_answers_its_version(
    serving: tuple[str, Release], tmp_path: Path, system: str, machine: str
) -> None:
    """The archive the checksums vouch for is unpacked, and its program is the one on PATH."""
    base, release = serving
    archive = archive_for(VERSION, system, machine)
    _publish(release, archive)

    installed = install(archive, tmp_path / "bin", releases=base)

    equal(installed, tmp_path / "bin" / "gh")
    contains(run([str(installed), "--version"], check=True).stdout, f"gh version {VERSION}")
    equal(
        release.asked,
        [f"/v{VERSION}/{archive.checksums}", f"/v{VERSION}/{archive.name}"],
        describing="what the installer downloaded",
    )


def test_an_archive_the_checksums_do_not_vouch_for_is_never_installed(
    serving: tuple[str, Release], tmp_path: Path
) -> None:
    """A download whose digest disagrees with the release's own list reaches nothing."""
    base, release = serving
    archive = archive_for(VERSION, "linux", "x86_64")
    _publish(release, archive, listed="f" * 64)

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), "nothing was installed")
    truth(not (tmp_path / "bin").exists(), describing="nothing written where gh goes")


def test_a_release_listing_no_checksum_for_the_archive_is_refused(
    serving: tuple[str, Release], tmp_path: Path
) -> None:
    """An archive the checksums file does not name is one nothing vouches for."""
    base, release = serving
    archive = archive_for(VERSION, "linux", "arm64")
    _publish(release, archive)
    release.files[archive.checksums] = b"0" * 64 + b"  something-else.tar.gz\n"

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"lists no checksum for {archive.name}")


def test_a_release_the_forge_does_not_serve_is_refused(
    serving: tuple[str, Release], tmp_path: Path
) -> None:
    """A download that fails says which address it was."""
    base, _ = serving
    archive = archive_for(VERSION, "linux", "x86_64")

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"{archive.checksums} could not be downloaded")


@pytest.mark.parametrize(
    ("system", "machine"),
    [("win32", "AMD64"), ("linux", "riscv64"), ("freebsd14", "amd64")],
)
def test_a_host_the_installer_does_not_know_is_refused_by_name(system: str, machine: str) -> None:
    """Rather than guess at an archive, the installer names the host it cannot serve."""
    with pytest.raises(InstallerError) as raised:
        archive_for(VERSION, system, machine)

    refused([str(raised.value)], f"this host is {system} on {machine}")


def test_a_version_that_is_not_a_release_is_refused() -> None:
    """The release reaches a URL, so it is read as a release or not at all."""
    with pytest.raises(InstallerError) as raised:
        archive_for("latest", "linux", "x86_64")

    contains(str(raised.value), "`latest` is not a release")


def test_an_address_that_is_not_https_is_never_fetched(tmp_path: Path) -> None:
    """Only the forge's https address and a loopback stand-in are downloaded from."""
    archive = archive_for(VERSION, "linux", "x86_64")

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases="http://example.invalid")

    contains(str(raised.value), "is not an address this installer downloads from")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_command_says_what_it_installed_and_that_its_directory_is_off_path(
    serving: tuple[str, Release],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The entry point `just install-gh` reaches: its status and what it tells the caller."""
    base, release = serving
    _publish(release, archive_for(VERSION, sys.platform, platform.machine()))
    into = tmp_path / "bin"
    monkeypatch.setenv("PATH", str(tmp_path / "elsewhere"))

    equal(install_gh(VERSION, into, releases=base), 0)

    said = capsys.readouterr().err
    contains(said, f"install-gh: installed gh {VERSION} at {into / 'gh'}")
    contains(said, f"{into} is not on PATH")

    monkeypatch.setenv("PATH", str(into))
    equal(install_gh(VERSION, into, releases=base), 0)
    absent(capsys.readouterr().err, "is not on PATH")
    contains(run(["gh", "--version"], check=True).stdout, f"gh version {VERSION}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_command_exits_non_zero_naming_why_it_installed_nothing(
    serving: tuple[str, Release], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A release the checksums do not vouch for is one exit status and one sentence."""
    base, release = serving
    _publish(release, archive_for(VERSION, sys.platform, platform.machine()), listed="e" * 64)

    equal(install_gh(VERSION, tmp_path / "bin", releases=base), 1)

    contains(capsys.readouterr().err, "install-gh: ")
    truth(not (tmp_path / "bin" / "gh").exists(), describing="no gh where the install goes")


def test_the_repository_command_refuses_what_is_not_a_release(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """`python -m repo_checks install-gh` dispatches to the installer and passes its status on."""
    equal(main(["install-gh", "latest"]), 1)
    contains(capsys.readouterr().err, "install-gh: `latest` is not a release")

    with pytest.raises(SystemExit) as exited:
        main(["install-gh"])
    equal(exited.value.code, 2, describing="the status of install-gh named no release")
    contains(capsys.readouterr().err, "install-gh needs the release to install")
