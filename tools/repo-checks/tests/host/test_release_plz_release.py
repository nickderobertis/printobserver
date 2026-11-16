"""`install-release-plz`: one exact release program, verified before it reaches PATH.

Upstream publishes no checksums file beside its archives, so the digest this
installer holds a download to is the one this repository commits. That makes two
things worth proving over a stand-in release served on loopback: the archive
whose bytes match the committed digest is unpacked and its program run, and one
whose bytes do not reaches no path at all — the second by serving a *different*
archive under the name the digest was recorded for, which is what tampering
between the recording and the download looks like.

The committed digests themselves are read against the held release and against
`AGENTS.md`'s supported-platform list, because a release nobody recorded digests
for and a platform nobody recorded one for are the two ways a bump silently
stops verifying anything.
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
from repo_checks.model import Repo, toolchain_tools
from repo_checks.platforms import supported
from repo_checks.release_plz_release import (
    DIGESTS,
    TARGETS,
    Archive,
    archive_for,
    cargo_bin,
    install,
)
from repo_checks.shell import run
from repo_checks.verified_download import InstallerError
from standin_release import Release, serving
from treecopy import REPO_ROOT

#: A release the stand-in publishes and nothing else holds anything at, so the
#: committed digests are never what a served archive is checked against.
VERSION = "0.0.1"

#: The release `repo-policy.toml` holds the release program at.
HELD = next(
    tool.version for tool in toolchain_tools(Repo(REPO_ROOT)) if tool.command == "release-plz"
)


def _program(release: str) -> bytes:
    """A release program that answers `--version` the way the real one does."""
    return f"#!{sys.executable}\nprint('release-plz {release}')\n".encode()


def _archive_bytes(archive: Archive, *, carrying: bytes, named: str | None = None) -> bytes:
    """The archive the release would publish for one target: one program at its root.

    `named` puts that program under some other name, which is what an installer
    meets when a producer changes its layout.
    """
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as bundle:
        info = tarfile.TarInfo(named if named is not None else archive.program)
        info.size = len(carrying)
        info.mode = 0o755
        bundle.addfile(info, io.BytesIO(carrying))
    return buffer.getvalue()


@pytest.fixture
def serving_release() -> Iterator[tuple[str, Release]]:
    """A loopback server answering the release's archives under its own tag."""
    release = Release()
    with serving(f"/release-plz-v{VERSION}/", release) as base:
        yield base, release


def _publish(
    release: Release, archive: Archive, monkeypatch: pytest.MonkeyPatch, *, tampered: bool = False
) -> None:
    """Publish the host's archive, and record the digest this repository commits for it.

    `tampered` records the digest of the archive that was reviewed and then
    serves a different one under the same name.
    """
    recorded = _archive_bytes(archive, carrying=_program(VERSION))
    served = _archive_bytes(archive, carrying=_program("0.0.2 (not what was recorded)"))
    release.files[archive.name] = served if tampered else recorded
    monkeypatch.setitem(DIGESTS, VERSION, {archive.target: hashlib.sha256(recorded).hexdigest()})


@pytest.mark.skipif(sys.platform == "win32", reason="the archive's program is a script")
def test_the_verb_installs_a_verified_release_and_the_installed_copy_answers_it(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """`python -m repo_checks install-release-plz` against a stand-in, end to end."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, monkeypatch)
    into = tmp_path / "bin"

    equal(main(["install-release-plz", VERSION, "--releases", base, "--into", str(into)]), 0)

    installed = into / archive.program
    contains(run([str(installed)], check=True).stdout, f"release-plz {VERSION}")
    equal(
        release.asked,
        [f"/release-plz-v{VERSION}/{archive.name}"],
        describing="what the installer downloaded",
    )


def test_the_verb_places_the_archives_own_program_under_the_name_this_host_runs(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """On every host, Windows included, what lands on PATH is what the archive carried.

    The two journeys that run the installed program skip Windows, where a
    script is no `.exe`; this one does not run it, so it is what drives the
    Windows target's extraction and placement: `release-plz.exe` out of the
    archive and into the directory, byte for byte, under the name a Windows
    shell resolves `release-plz` to.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, monkeypatch)
    into = tmp_path / "bin"

    equal(main(["install-release-plz", VERSION, "--releases", base, "--into", str(into)]), 0)

    equal(
        archive.program,
        "release-plz.exe" if sys.platform == "win32" else "release-plz",
        describing="the name the program is placed under on this host",
    )
    equal(
        sorted(path.name for path in into.iterdir()),
        [archive.program],
        describing="what the install left in the directory",
    )
    equal((into / archive.program).read_bytes(), _program(VERSION))


@pytest.mark.skipif(sys.platform == "win32", reason="the archive's program is a script")
def test_a_second_install_replaces_the_program_the_first_one_put_there(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A bump installs over the copy already there rather than beside it.

    Every install after the first meets a program at the name it writes, and the
    write is staged and moved onto it: what a caller reaching for the program
    finds is the copy that was there or the whole new one, never a partial file.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, monkeypatch)
    into = tmp_path / "bin"
    install(archive, into, releases=base)

    replacing = _archive_bytes(archive, carrying=_program("0.0.2"))
    release.files[archive.name] = replacing
    monkeypatch.setitem(DIGESTS, VERSION, {archive.target: hashlib.sha256(replacing).hexdigest()})
    installed = install(archive, into, releases=base)

    contains(run([str(installed)], check=True).stdout, "release-plz 0.0.2")
    equal(
        sorted(path.name for path in into.iterdir()),
        [archive.program],
        describing="what the two installs left in the directory",
    )


def test_the_verb_refuses_a_tampered_archive_before_anything_reaches_path(
    serving_release: tuple[str, Release],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """An archive that is not the one the committed digest was taken from installs nothing."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, monkeypatch, tampered=True)
    into = tmp_path / "bin"

    equal(main(["install-release-plz", VERSION, "--releases", base, "--into", str(into)]), 1)

    said = capsys.readouterr().err
    contains(said, "install-release-plz: ")
    contains(said, "nothing was installed")
    contains(said, f"the digest this repository commits for {archive.target}")
    truth(not into.exists(), describing="nothing written where the program goes")


def test_a_staging_name_the_filesystem_refuses_preserves_the_installed_program(
    serving_release: tuple[str, Release],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A directory at the staging name blocks writing without replacing the working copy."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, monkeypatch)
    into = tmp_path / "bin"
    equal(main(["install-release-plz", VERSION, "--releases", base, "--into", str(into)]), 0)
    capsys.readouterr()
    installed = into / archive.program
    before = installed.read_bytes()
    (into / f".{archive.program}.part").mkdir()

    equal(main(["install-release-plz", VERSION, "--releases", base, "--into", str(into)]), 1)

    contains(capsys.readouterr().err, "could not be placed")
    equal(installed.read_bytes(), before)


def test_the_verb_says_what_it_installed_and_that_its_directory_is_off_path(
    serving_release: tuple[str, Release],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A directory off PATH is an install that worked and a program nobody can run."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, monkeypatch)
    into = tmp_path / "bin"
    monkeypatch.setenv("PATH", str(tmp_path / "elsewhere"))

    equal(main(["install-release-plz", VERSION, "--releases", base, "--into", str(into)]), 0)

    said = capsys.readouterr().err
    contains(said, f"installed release-plz {VERSION} at {into / archive.program}")
    contains(said, f"{into} is not on PATH")


def test_a_release_no_digest_is_committed_for_is_refused_naming_itself(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """A bump that records no digests verifies nothing, so it installs nothing."""
    base, _ = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"commits no SHA-256 for release-plz {VERSION}")
    contains(str(raised.value), f"it records {HELD}")


def test_a_target_no_digest_is_committed_for_is_refused_naming_it(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A release recorded for some targets and not this one vouches for nothing here."""
    base, _ = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    monkeypatch.setitem(DIGESTS, VERSION, {"riscv64gc-unknown-linux-gnu": "0" * 64})

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"on {archive.target}")
    contains(str(raised.value), "riscv64gc-unknown-linux-gnu")


def test_a_release_the_forge_does_not_serve_is_refused(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A download that fails says which address it was."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, monkeypatch)
    release.files.clear()

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"{archive.name} could not be downloaded")


def test_an_archive_carrying_no_release_program_is_refused_naming_what_it_lacks(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A digest vouches for bytes, not for what they are: the extraction says the rest."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    published = _archive_bytes(archive, carrying=_program(VERSION), named="README")
    release.files[archive.name] = published
    monkeypatch.setitem(DIGESTS, VERSION, {archive.target: hashlib.sha256(published).hexdigest()})

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"{archive.name} carries no {archive.program}")
    truth(not (tmp_path / "bin").exists(), describing="nothing written where the program goes")


@pytest.mark.parametrize(
    ("system", "machine"),
    [("linux", "riscv64"), ("freebsd14", "amd64"), ("darwin", "x86_64")],
)
def test_a_host_the_installer_does_not_know_is_refused_by_name(system: str, machine: str) -> None:
    """Rather than guess at an archive, the installer names the host it cannot serve."""
    with pytest.raises(InstallerError) as raised:
        archive_for(VERSION, system, machine)

    refused([str(raised.value)], f"this host is {system} on {machine}")


def test_a_version_that_is_not_a_release_is_refused() -> None:
    """The release reaches a URL, so it is read as a release or not at all."""
    with pytest.raises(InstallerError) as raised:
        archive_for("newest", "linux", "x86_64")

    contains(str(raised.value), "`newest` is not a release")


@pytest.mark.parametrize(
    "releases",
    [
        "http://example.invalid",
        # Everything before an `@` in an authority is userinfo, so this address
        # begins with the loopback address and resolves to somebody else's host.
        "http://127.0.0.1:80@example.invalid",
        "ftp://127.0.0.1:8080",
        "file:///tmp",
    ],
)
def test_an_address_that_is_not_https_or_loopback_is_never_fetched(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, releases: str
) -> None:
    """Only a producer's https address and a loopback stand-in are downloaded from."""
    archive = archive_for(VERSION, sys.platform, platform.machine())
    monkeypatch.setitem(DIGESTS, VERSION, {archive.target: "0" * 64})

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=releases)

    contains(str(raised.value), "is not an address this installer downloads from")
    truth(not (tmp_path / "bin").exists(), describing="nothing written where the program goes")


def test_every_supported_platform_has_a_target_and_a_committed_digest() -> None:
    """A gate cell on a platform with no digest would run the release journeys against nothing."""
    committed = Repo(REPO_ROOT)
    equal(
        sorted(TARGETS.values()),
        sorted(entry.target for entry in supported(committed)),
        describing="the targets this installer takes release-plz for",
    )
    equal(
        sorted(DIGESTS[str(HELD)]),
        sorted(TARGETS.values()),
        describing=f"the targets a digest is committed for at the held {HELD}",
    )


def test_the_default_directory_is_the_one_a_toolchain_puts_on_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Where `cargo install` put it, so the program is reachable with nothing added to PATH."""
    monkeypatch.delenv("CARGO_HOME", raising=False)
    equal(cargo_bin(), Path.home() / ".cargo" / "bin")

    monkeypatch.setenv("CARGO_HOME", str(tmp_path / "cargo"))
    equal(cargo_bin(), tmp_path / "cargo" / "bin", describing="a host that moved CARGO_HOME")
