"""`install-powershell`: one exact PowerShell release, verified before it is unpacked.

The release is served by a stand-in over real HTTP on loopback, laid out the way
the forge lays one out — the host's archive and the release's own
`hashes.sha256` side by side, that file encoded as PowerShell encodes it — so
what is proven is the installer's own download, digest check, extraction and
linking. `pwsh` inside the archive is a script answering `--version`, and the
link the install leaves is run to prove it reaches what the archive carried.
"""

from __future__ import annotations

import hashlib
import io
import platform
import sys
import tarfile
from collections.abc import Iterator
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal, refused, truth
from repo_checks.powershell_release import (
    HASHES,
    archive_for,
    install,
    listing,
    runtime_directory,
)
from repo_checks.shell import run
from repo_checks.verified_download import InstallerError
from standin_powershell import RUNTIME_FILE, archive_bytes, program
from standin_powershell import publish as _publish
from standin_release import Release, serving

VERSION = "7.6.6"


def _escaping_archive() -> bytes:
    """An archive whose one member names a path above where it is unpacked."""
    buffer = io.BytesIO()
    payload = b"a file nobody asked this archive to write\n"
    with tarfile.open(fileobj=buffer, mode="w:gz") as bundle:
        info = tarfile.TarInfo("../escaped")
        info.size = len(payload)
        bundle.addfile(info, io.BytesIO(payload))
    return buffer.getvalue()


@pytest.fixture
def serving_release() -> Iterator[tuple[str, Release]]:
    """A loopback server answering the release's files under `/v<version>/`."""
    release = Release()
    with serving(f"/v{VERSION}/", release) as base:
        yield base, release


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_installs_a_verified_release_and_the_link_reaches_its_program(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """`python -m repo_checks install-powershell` against a stand-in, end to end."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)

    runtime = runtime_directory(into, VERSION)
    truth((runtime / RUNTIME_FILE).is_file(), describing="the whole archive unpacked")
    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    equal(
        release.asked,
        [f"/v{VERSION}/{HASHES}", f"/v{VERSION}/{archive.name}"],
        describing="what the installer downloaded, the hashes file first",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_refuses_a_tampered_archive_before_anything_is_unpacked(
    serving_release: tuple[str, Release], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """An archive the release's own hashes do not vouch for is never unpacked."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, tampered=True)
    into = tmp_path / "bin"

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 1)

    said = capsys.readouterr().err
    contains(said, "install-powershell: ")
    contains(said, "nothing was installed")
    truth(not into.exists(), describing="nothing written where pwsh goes")
    truth(
        not runtime_directory(into, VERSION).exists(),
        describing="nothing unpacked where the runtime goes",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_an_archive_carrying_no_program_leaves_the_runtime_a_good_install_left(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """The archive is unpacked beside the runtime and moved onto it, never into it."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    install(archive, into, releases=base)

    broken = archive_bytes(carrying=None)
    release.files[archive.name] = broken
    release.files[HASHES] = (f"{hashlib.sha256(broken).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )
    with pytest.raises(InstallerError):
        install(archive, into, releases=base)

    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_pointed_at_a_relative_directory_leaves_a_link_that_runs(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """`--into bin` is a directory a caller names from where they stand, and it works.

    The runtime goes BESIDE the directory the program is linked into, so a
    relative directory taken as written would leave `bin/pwsh` pointing at
    `share/powershell-<release>/pwsh` resolved from `bin/` — a link under
    itself, where nothing is, and a verb that exits zero having installed a
    program nobody can run.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    monkeypatch.chdir(tmp_path)

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", "bin"]), 0)

    linked = tmp_path / "bin" / "pwsh"
    truth(linked.is_symlink(), describing="the link the install left")
    truth(linked.resolve().is_file(), describing=f"what it points at: {linked.readlink()}")
    contains(run([str(linked), "--version"], check=True).stdout, f"PowerShell {VERSION}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_says_what_it_installed_and_that_its_directory_is_off_path(
    serving_release: tuple[str, Release],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A directory off PATH is an install that worked and a program nobody can run."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"
    monkeypatch.setenv("PATH", str(tmp_path / "elsewhere"))

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)

    said = capsys.readouterr().err
    contains(said, f"installed PowerShell {VERSION} at {into / 'pwsh'}")
    contains(said, f"{into} is not on PATH")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_second_install_replaces_the_runtime_and_the_link_the_first_one_left(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """Installing again over a runtime already there leaves one runtime and one link."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    install(archive, into, releases=base)

    replacing = archive_bytes(carrying=program("7.6.6 (the second install)"), runtime=b"newer\n")
    release.files[archive.name] = replacing
    release.files[HASHES] = (f"{hashlib.sha256(replacing).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )
    linked = install(archive, into, releases=base)

    runtime = runtime_directory(into, VERSION)
    contains(run([str(linked), "--version"], check=True).stdout, "the second install")
    equal((runtime / RUNTIME_FILE).read_bytes(), b"newer\n", describing="the unpacked runtime")
    equal(
        sorted(path.name for path in runtime.parent.iterdir()),
        [runtime.name],
        describing="what the two installs left beside the runtime",
    )
    equal(sorted(path.name for path in into.iterdir()), ["pwsh"], describing="the link directory")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_an_archive_whose_member_leaves_the_directory_is_unpacked_nowhere(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """A digest vouches for bytes; the archive inside them is still read as untrusted.

    The member here names a path above the directory it is unpacked into, which
    is how an archive writes over a file nobody asked it to touch.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    escaping = _escaping_archive()
    release.files[archive.name] = escaping
    release.files[HASHES] = (f"{hashlib.sha256(escaping).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )
    into = tmp_path / "bin"

    with pytest.raises(InstallerError) as raised:
        install(archive, into, releases=base)

    above = runtime_directory(into, VERSION).parent
    contains(str(raised.value), "could not be unpacked")
    truth(not (above / "escaped").exists(), describing="nothing written above the directory")
    truth(not into.exists(), describing="nothing written where pwsh goes")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_release_listing_no_checksum_for_the_archive_is_refused(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """An archive the hashes file does not name is one nothing vouches for."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    release.files[HASHES] = f"{'0' * 64} *something-else.tar.gz\n".encode("utf-16")

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"the release's {HASHES} lists no checksum for {archive.name}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_release_the_forge_does_not_serve_is_refused(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """A download that fails says which address it was."""
    base, _ = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"{HASHES} could not be downloaded")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
@pytest.mark.parametrize(
    ("published", "said"),
    [
        (b"not a tar.gz at all", "could not be unpacked"),
        (archive_bytes(carrying=None), "carries no pwsh"),
    ],
    ids=["unreadable", "no-program"],
)
def test_an_archive_that_is_not_a_powershell_is_refused_naming_what_it_lacks(
    serving_release: tuple[str, Release], tmp_path: Path, published: bytes, said: str
) -> None:
    """A digest vouches for bytes, not for what they are: the unpacking says the rest."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    release.files[archive.name] = published
    release.files[HASHES] = (f"{hashlib.sha256(published).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), said)
    truth(not (tmp_path / "bin").exists(), describing="nothing written where pwsh goes")
    truth(
        not runtime_directory(tmp_path / "bin", VERSION).exists(),
        describing="no runtime left where one would go",
    )
    equal(
        sorted(path.name for path in (tmp_path / "share").iterdir())
        if (tmp_path / "share").exists()
        else [],
        [],
        describing="what the refused install left beside the runtime",
    )


def test_a_windows_host_is_told_it_carries_powershell_already() -> None:
    """There is nothing to install where `powershell` is part of the operating system."""
    with pytest.raises(InstallerError) as raised:
        archive_for(VERSION, "win32", "AMD64")

    refused([str(raised.value)], "a Windows host carries PowerShell already")


@pytest.mark.parametrize(("system", "machine"), [("linux", "riscv64"), ("freebsd14", "amd64")])
def test_a_host_the_installer_does_not_know_is_refused_by_name(system: str, machine: str) -> None:
    """Rather than guess at an archive, the installer names the host it cannot serve."""
    with pytest.raises(InstallerError) as raised:
        archive_for(VERSION, system, machine)

    refused([str(raised.value)], f"this host is {system} on {machine}")


def test_a_version_that_is_not_a_release_is_refused() -> None:
    """The release reaches a URL, so it is read as a release or not at all."""
    with pytest.raises(InstallerError) as raised:
        archive_for("lts", "linux", "x86_64")

    contains(str(raised.value), "`lts` is not a release")


def test_the_hashes_file_is_read_as_its_producer_encoded_it() -> None:
    """Read as UTF-8, PowerShell's UTF-16 hashes file lists nothing at all."""
    line = f"{'a' * 64} *powershell-{VERSION}-linux-x64.tar.gz"

    equal(listing(line.encode("utf-16")).strip(), line)
    equal(listing(line.encode("utf-8")).strip(), line, describing="a producer that writes UTF-8")
