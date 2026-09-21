"""One exact GitHub CLI release, installed from its own published archive.

`gh` is held at one release in `repo-policy.toml`'s toolchain because the one
thing here that runs it — the end-to-end journey proving a real `gh skill
install` of this repository's Agent Skill — is a claim about that release's
behaviour. No package manager installs an exact `gh` release on every host, so
this takes the official archive for the host from the release itself, checks it
against the checksums file the same release publishes, and only then puts the
program on PATH. A download the checksums do not vouch for is never installed.

Linux on x86_64 and arm64 and macOS are the hosts it knows; any other is refused
by name rather than guessed at.
"""

from __future__ import annotations

import hashlib
import io
import os
import platform
import sys
import tarfile
import urllib.error
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import RELEASE

#: Where every `gh` release publishes its archives and its checksums file.
RELEASES = "https://github.com/cli/cli/releases/download"

#: How long one download may take, in seconds.
DOWNLOAD_TIMEOUT = 120

#: The processor names a host reports, as the release names its archives.
ARCHITECTURES = {"x86_64": "amd64", "amd64": "amd64", "aarch64": "arm64", "arm64": "arm64"}


class GhReleaseError(Exception):
    """Why the held release could not be installed, in words a caller acts on."""


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
        GhReleaseError: If `version` is not a release, or the host is not one
            this installer knows.
    """
    if not RELEASE.fullmatch(version):
        msg = f"`{version}` is not a release: name one as `2.100.0`"
        raise GhReleaseError(msg)
    architecture = ARCHITECTURES.get(machine.lower())
    named = {"linux": "linux", "darwin": "macOS"}.get(system)
    if named is None or architecture is None:
        msg = (
            f"this installer takes gh for Linux on x86_64 or arm64 and for macOS, and this "
            f"host is {system} on {machine}. Install gh {version} from "
            f"https://github.com/cli/cli/releases/tag/v{version} yourself."
        )
        raise GhReleaseError(msg)
    return Archive(version, named, architecture)


def _download(url: str) -> bytes:
    """One file of a release, or why it could not be fetched.

    Raises:
        GhReleaseError: If the download fails.
    """
    if not url.startswith(("https://", "http://127.0.0.1:")):
        msg = f"{url} is not an address this installer downloads from"
        raise GhReleaseError(msg)
    try:
        with urllib.request.urlopen(url, timeout=DOWNLOAD_TIMEOUT) as answer:  # noqa: S310
            return answer.read()
    except (urllib.error.URLError, OSError) as error:
        msg = f"{url} could not be downloaded: {error}"
        raise GhReleaseError(msg) from error


def _expected_digest(checksums: str, archive: Archive) -> str:
    """The SHA-256 the release's checksums file lists for one archive.

    Raises:
        GhReleaseError: If the file lists none for it.
    """
    for line in checksums.splitlines():
        digest, _, listed = line.strip().partition("  ")
        if listed == archive.name and len(digest) == 64:
            return digest.lower()
    msg = f"the release's {archive.checksums} lists no checksum for {archive.name}"
    raise GhReleaseError(msg)


def _program_in(payload: bytes, archive: Archive) -> bytes:
    """The `gh` program out of one verified archive.

    Raises:
        GhReleaseError: If the archive carries no program where the release puts it.
    """
    member = f"{archive.stem}/bin/gh"
    try:
        if archive.name.endswith(".zip"):
            with zipfile.ZipFile(io.BytesIO(payload)) as bundle:
                return bundle.read(member)
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as bundle:
            extracted = bundle.extractfile(member)
            if extracted is None:
                raise KeyError(member)
            return extracted.read()
    except (KeyError, tarfile.TarError, zipfile.BadZipFile) as error:
        msg = f"{archive.name} carries no {member}: {error}"
        raise GhReleaseError(msg) from error


def install(archive: Archive, into: Path, *, releases: str = RELEASES) -> Path:
    """Download, verify and install one archive's `gh` into a directory.

    Nothing is written into `into` until the archive has matched the digest
    the release's own checksums file lists for it.

    Raises:
        GhReleaseError: If a download fails, the digests disagree, or the
            archive carries no program.
    """
    base = f"{releases}/v{archive.version}"
    checksums = _download(f"{base}/{archive.checksums}").decode("utf-8", errors="replace")
    expected = _expected_digest(checksums, archive)
    payload = _download(f"{base}/{archive.name}")
    actual = hashlib.sha256(payload).hexdigest()
    if actual != expected:
        msg = (
            f"{archive.name} hashes to {actual}, and the release's {archive.checksums} lists "
            f"{expected}: nothing was installed"
        )
        raise GhReleaseError(msg)
    program = _program_in(payload, archive)
    into.mkdir(parents=True, exist_ok=True)
    target = into / "gh"
    staged = into / ".gh.part"
    staged.write_bytes(program)
    staged.chmod(0o755)
    staged.replace(target)
    return target


def install_gh(version: str, into: Path | None = None) -> int:
    """Install gh `version` for this host into `into`, `~/.local/bin` by default."""
    destination = into if into is not None else Path.home() / ".local" / "bin"
    try:
        archive = archive_for(version, sys.platform, platform.machine())
        installed = install(archive, destination)
    except GhReleaseError as refused:
        print(f"install-gh: {refused}", file=sys.stderr)
        return 1
    print(f"install-gh: installed gh {version} at {installed}", file=sys.stderr)
    if str(destination) not in os.environ.get("PATH", "").split(os.pathsep):
        print(
            f"install-gh: {destination} is not on PATH; put it there before running gh",
            file=sys.stderr,
        )
    return 0
