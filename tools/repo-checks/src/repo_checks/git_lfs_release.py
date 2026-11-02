"""One exact Git LFS release, installed from its own published archive.

The real-print cases' G-code and frames are Git LFS objects, so a clone that
fetches them, cleans a new frame into a pointer, or pushes one needs `git-lfs`.
`repo-policy.toml` holds it at one release, and `just bootstrap` puts that
release in the user's own `~/.local/bin` where a host carries no `git-lfs` at
all — no package manager and no administrator rights, the way `gh` is installed.
The archive is the host's own from the release, checked against the
`sha256sums.asc` the same release publishes before anything reaches a path.

The download, the digest check, the extraction and the placement are
`verified_download`'s; what this module states is what a Git LFS release calls
its archives and where in each the program is.
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

#: Where every Git LFS release publishes its archives and its checksums file.
RELEASES = "https://github.com/git-lfs/git-lfs/releases/download"

#: The checksums file every release publishes: one `<sha256> *<archive>` a line,
#: clear-signed, which leaves those lines as they are.
CHECKSUMS = "sha256sums.asc"

#: The processor names a host reports, as the release names its archives.
ARCHITECTURES = {"x86_64": "amd64", "amd64": "amd64", "aarch64": "arm64", "arm64": "arm64"}

#: The operating systems the release builds for, as it names them.
SYSTEMS = {"linux": "linux", "darwin": "darwin", "win32": "windows"}


@dataclass(frozen=True, slots=True)
class Archive:
    """The one archive of a release built for this host."""

    #: The release, without its leading `v`.
    version: str
    #: The operating system as the release names it: `linux`, `darwin` or `windows`.
    system: str
    #: The processor as the release names it: `amd64` or `arm64`.
    architecture: str

    @property
    def name(self) -> str:
        """The archive's file name, as the checksums file lists it."""
        extension = "tar.gz" if self.system == "linux" else "zip"
        return f"git-lfs-{self.system}-{self.architecture}-v{self.version}.{extension}"

    @property
    def program(self) -> str:
        """The program's own file name on this system."""
        return "git-lfs.exe" if self.system == "windows" else "git-lfs"

    @property
    def member(self) -> str:
        """Where in the archive the program is."""
        return f"git-lfs-{self.version}/{self.program}"


def archive_for(version: str, system: str, machine: str) -> Archive:
    """The archive of `version` built for one host.

    Raises:
        InstallerError: If `version` is not a release, or the host is not one
            this installer knows.
    """
    if not RELEASE.fullmatch(version):
        msg = f"`{version}` is not a release: name one as `3.8.1`"
        raise InstallerError(msg)
    architecture = ARCHITECTURES.get(machine.lower())
    named = SYSTEMS.get(system)
    if named is None or architecture is None:
        msg = (
            f"this installer takes git-lfs for Linux, macOS and Windows on x86_64 or arm64, "
            f"and this host is {system} on {machine}. Install git-lfs {version} from "
            f"https://github.com/git-lfs/git-lfs/releases/tag/v{version} yourself."
        )
        raise InstallerError(msg)
    return Archive(version, named, architecture)


def install(archive: Archive, into: Path, *, releases: str = RELEASES) -> Path:
    """Download, verify and install one archive's `git-lfs` into a directory.

    Nothing is written into `into` until the archive has matched the digest
    the release's own checksums file lists for it.

    Raises:
        InstallerError: If a download fails, the digests disagree, or the
            archive carries no program.
    """
    held_to_producer(releases, RELEASES)
    base = f"{releases}/v{archive.version}"
    checksums = download(f"{base}/{CHECKSUMS}").decode("utf-8", errors="replace")
    expected = digest_for(checksums, archive.name)
    if expected is None:
        msg = f"the release's {CHECKSUMS} lists no checksum for {archive.name}"
        raise InstallerError(msg)
    payload = verified(
        f"{base}/{archive.name}",
        name=archive.name,
        expected=expected,
        source=f"the release's {CHECKSUMS}",
    )
    program = member_of(payload, name=archive.name, member=archive.member)
    return place(program, into, archive.program)


def install_git_lfs(version: str, into: Path | None = None, *, releases: str = RELEASES) -> int:
    """Install git-lfs `version` for this host into `into`, `~/.local/bin` by default.

    Says what it installed and where, says so when that directory is not on
    PATH, and exits non-zero naming why when it installed nothing.
    """
    destination = into if into is not None else Path.home() / ".local" / "bin"
    try:
        held_to_producer(releases, RELEASES)
        archive = archive_for(version, sys.platform, platform.machine())
        installed = install(archive, destination, releases=releases)
    except InstallerError as refused:
        print(f"install-git-lfs: {refused}", file=sys.stderr)
        return 1
    return announce("install-git-lfs", f"git-lfs {version}", installed, destination, "git-lfs")
