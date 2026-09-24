"""One exact GitHub CLI release, installed from its own published archive.

`gh` is held at one release in `repo-policy.toml`'s toolchain because the one
thing here that runs it — the end-to-end journey proving a real `gh skill
install` of this repository's Agent Skill — is a claim about that release's
behaviour. No package manager installs an exact `gh` release on every host, so
this takes the official archive for the host from the release itself, checks it
against the checksums file the same release publishes, and only then puts the
program on PATH. A download the checksums do not vouch for is never installed.

The download, the digest check, the extraction and the placement are
`verified_download`'s, shared with the two installers beside this one; what this
module states is what a `gh` release calls its archives.

Linux on x86_64 and arm64 and macOS are the hosts it knows; any other is refused
by name rather than guessed at.
"""

from __future__ import annotations

import platform
import sys
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import RELEASE
from repo_checks.verified_download import (
    InstallerError,
    announce,
    digest_for,
    download,
    held_to_producer,
    member_of,
    place,
    verified,
)

#: Where every `gh` release publishes its archives and its checksums file.
RELEASES = "https://github.com/cli/cli/releases/download"

#: The processor names a host reports, as the release names its archives.
ARCHITECTURES = {"x86_64": "amd64", "amd64": "amd64", "aarch64": "arm64", "arm64": "arm64"}


@dataclass(frozen=True, slots=True)
class Archive:
    """The one archive of a release built for this host."""

    #: The release, without its leading `v`.
    version: str
    #: The operating system as the release names it: `linux` or `macOS`.
    system: str
    #: The processor as the release names it: `amd64` or `arm64`.
    architecture: str

    @property
    def stem(self) -> str:
        """The archive's name without its extension, which is also its top directory."""
        return f"gh_{self.version}_{self.system}_{self.architecture}"

    @property
    def name(self) -> str:
        """The archive's file name, as the checksums file lists it."""
        return f"{self.stem}.{'tar.gz' if self.system == 'linux' else 'zip'}"

    @property
    def checksums(self) -> str:
        """The name of the checksums file the same release publishes."""
        return f"gh_{self.version}_checksums.txt"


def archive_for(version: str, system: str, machine: str) -> Archive:
    """The archive of `version` built for one host.

    Raises:
        InstallerError: If `version` is not a release, or the host is not one
            this installer knows.
    """
    if not RELEASE.fullmatch(version):
        msg = f"`{version}` is not a release: name one as `2.100.0`"
        raise InstallerError(msg)
    architecture = ARCHITECTURES.get(machine.lower())
    named = {"linux": "linux", "darwin": "macOS"}.get(system)
    if named is None or architecture is None:
        msg = (
            f"this installer takes gh for Linux on x86_64 or arm64 and for macOS, and this "
            f"host is {system} on {machine}. Install gh {version} from "
            f"https://github.com/cli/cli/releases/tag/v{version} yourself."
        )
        raise InstallerError(msg)
    return Archive(version, named, architecture)


def _expected_digest(checksums: str, archive: Archive) -> str:
    """The SHA-256 the release's checksums file lists for one archive.

    Raises:
        InstallerError: If the file lists none for it.
    """
    listed = digest_for(checksums, archive.name)
    if listed is None:
        msg = f"the release's {archive.checksums} lists no checksum for {archive.name}"
        raise InstallerError(msg)
    return listed


def _program_in(payload: bytes, archive: Archive) -> bytes:
    """The `gh` program out of one verified archive.

    Raises:
        InstallerError: If the archive carries no program where the release puts it.
    """
    return member_of(payload, name=archive.name, member=f"{archive.stem}/bin/gh")


def install(archive: Archive, into: Path, *, releases: str = RELEASES) -> Path:
    """Download, verify and install one archive's `gh` into a directory.

    Nothing is written into `into` until the archive has matched the digest
    the release's own checksums file lists for it.

    Raises:
        InstallerError: If a download fails, the digests disagree, or the
            archive carries no program.
    """
    held_to_producer(releases, RELEASES)
    base = f"{releases}/v{archive.version}"
    checksums = download(f"{base}/{archive.checksums}").decode("utf-8", errors="replace")
    payload = verified(
        f"{base}/{archive.name}",
        name=archive.name,
        expected=_expected_digest(checksums, archive),
        source=f"the release's {archive.checksums}",
    )
    return place(_program_in(payload, archive), into, "gh")


def install_gh(version: str, into: Path | None = None, *, releases: str = RELEASES) -> int:
    """Install gh `version` for this host into `into`, `~/.local/bin` by default.

    Says what it installed and where, says so when that directory is not on
    PATH, and exits non-zero naming why when it installed nothing.
    """
    destination = into if into is not None else Path.home() / ".local" / "bin"
    try:
        held_to_producer(releases, RELEASES)
        archive = archive_for(version, sys.platform, platform.machine())
        installed = install(
            archive, destination, releases=releases
        )  # pragma: unreached on win32 - archive_for takes no Windows archive
    except InstallerError as refused:
        print(f"install-gh: {refused}", file=sys.stderr)
        return 1
    return announce(
        "install-gh", f"gh {version}", installed, destination, "gh"
    )  # pragma: unreached on win32 - archive_for takes no Windows archive
