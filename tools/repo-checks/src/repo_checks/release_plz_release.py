"""Install the held release-plz archive after checking its committed SHA-256.

`test_release_plz_release.py` holds targets and digests to the toolchain release
and to `AGENTS.md`'s supported platforms.

`RELEASES`, the tag path, `Archive.name` and `Archive.program` restate how the
producer publishes a release, and what reconciles them with it is the gate
itself: `just test-e2e` runs `install-tools release-plz` first, and bootstrap
installs no release-plz, so every gate cell downloads the held archive from the
producer by those names, hashes it against `DIGESTS` and extracts the program
it names. A release published under any other address, archive or program name
fails that install closed, naming what it could not fetch or find, and fails the
gate with it.
"""

from __future__ import annotations

import os
import platform
import sys
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import RELEASE
from repo_checks.verified_download import (
    InstallerError,
    announce,
    held_to_producer,
    member_of,
    place,
    verified,
)

#: Where every release-plz release publishes its archives.
RELEASES = "https://github.com/release-plz/release-plz/releases/download"

#: The Rust target of each host this installer knows, keyed by what the
#: interpreter answers for the operating system and the processor. Exactly the
#: targets `AGENTS.md`'s supported-platform list names, because a gate cell on a
#: platform this omitted would run the release journeys against no program.
TARGETS = {
    ("linux", "x86_64"): "x86_64-unknown-linux-gnu",
    ("linux", "aarch64"): "aarch64-unknown-linux-gnu",
    ("darwin", "aarch64"): "aarch64-apple-darwin",
    ("win32", "x86_64"): "x86_64-pc-windows-msvc",
    ("win32", "aarch64"): "aarch64-pc-windows-msvc",
}

#: `platform.machine()` spells one processor differently per host — Linux
#: `x86_64` and `aarch64`, macOS `arm64`, Windows `AMD64` and `ARM64` — and is
#: read lowercased, so this maps every spelling it knows to one name.
ARCHITECTURES = {
    "x86_64": "x86_64",
    "amd64": "x86_64",
    "x64": "x86_64",
    "aarch64": "aarch64",
    "arm64": "aarch64",
}

#: SHA-256 values from the GitHub release API's per-asset `digest`. Upstream
#: publishes no checksums file for these archives. An install hashes the archive
#: against the matching release and target here before placing the program.
#:
#: Read the digests for a new release with:
#:
#:     gh api repos/release-plz/release-plz/releases/tags/release-plz-v<version> \
#:       --jq '.assets[] | "\(.name)  \(.digest)"'
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
DIGESTS: dict[str, dict[str, str]] = {
    "0.3.167": {
        "x86_64-unknown-linux-gnu": (
            "eebaca8d608e7863f3098bb75a84075b9af79b8b175a55c914b26c39036d99ac"
        ),
        "aarch64-unknown-linux-gnu": (
            "767b74e0bed10374615ca3f95626012cf0caa67fef2b606a3b21fca034caa381"
        ),
        "aarch64-apple-darwin": (
            "e9dc21a6a79ee59e6e9c5dc6d8fed54380e0d2a4f768c2950cbd8cf99f5f9fba"
        ),
        "x86_64-pc-windows-msvc": (
            "7b696a054c5d4d20e9e6adadf384811302ac9563acdac3b32aa43083daa6f197"
        ),
        "aarch64-pc-windows-msvc": (
            "fa99590beadee44893ea0c335c8864bee92dcaf3e7ddaae597517b3a9e161a33"
        ),
    }
}


@dataclass(frozen=True, slots=True)
class Archive:
    """The one archive of a release built for one target."""

    #: The release, without its leading `v`.
    version: str
    #: The Rust target the archive was built for.
    target: str

    @property
    def name(self) -> str:
        """The archive's file name, as the release publishes it."""
        return f"release-plz-{self.target}.tar.gz"

    @property
    def program(self) -> str:
        """What the program inside it is called, which is also its name on PATH."""
        return "release-plz.exe" if "windows" in self.target else "release-plz"

    @property
    def digest(self) -> str:
        """The SHA-256 this repository commits for it.

        Raises:
            InstallerError: If no digest is committed for that release, or none
                for that target within it.
        """
        recorded = DIGESTS.get(self.version)
        if recorded is None:
            msg = (
                f"this repository commits no SHA-256 for release-plz {self.version}: it records "
                f"{', '.join(sorted(DIGESTS))}. Add the release's own digests to "
                f"`repo_checks.release_plz_release.DIGESTS` in the change that holds it there."
            )
            raise InstallerError(msg)
        if self.target not in recorded:
            msg = (
                f"this repository commits no SHA-256 for release-plz {self.version} on "
                f"{self.target}: it records {', '.join(sorted(recorded))}"
            )
            raise InstallerError(msg)
        return recorded[self.target]


def archive_for(version: str, system: str, machine: str) -> Archive:
    """The archive of `version` built for one host.

    Raises:
        InstallerError: If `version` is not a release, or the host is not one
            this repository supports.
    """
    if not RELEASE.fullmatch(version):
        msg = f"`{version}` is not a release: name one as `0.3.167`"
        raise InstallerError(msg)
    target = TARGETS.get((system, ARCHITECTURES.get(machine.lower(), machine.lower())))
    if target is None:
        msg = (
            f"release-plz is taken prebuilt for {', '.join(sorted(TARGETS.values()))}, and this "
            f"host is {system} on {machine}. Install release-plz {version} yourself, with "
            f"`cargo install release-plz --locked --version {version}`."
        )
        raise InstallerError(msg)
    return Archive(version, target)


def cargo_bin() -> Path:
    """Where `cargo install` puts a program, which is where this puts one too.

    The directory this replaces an install into: every host that runs this
    repository's gate has a Rust toolchain, and a toolchain puts its own bin
    directory on PATH. Installing beside it is what makes `release-plz` reachable
    on a Windows runner as readily as on a Linux one, with nothing added to PATH.
    """
    home = os.environ.get("CARGO_HOME")
    return (Path(home) if home else Path.home() / ".cargo") / "bin"


def install(archive: Archive, into: Path, *, releases: str = RELEASES) -> Path:
    """Download, verify and install one archive's release program into a directory.

    Nothing is written into `into` until the archive has matched the SHA-256
    this repository commits for that release and that target.

    Raises:
        InstallerError: If no digest is committed, a download fails, the digests
            disagree, or the archive carries no program.
    """
    held_to_producer(releases, RELEASES)
    payload = verified(
        f"{releases}/release-plz-v{archive.version}/{archive.name}",
        name=archive.name,
        expected=archive.digest,
        source=f"the digest this repository commits for {archive.target}",
    )
    return place(
        member_of(payload, name=archive.name, member=archive.program), into, archive.program
    )


def install_release_plz(version: str, into: Path | None = None, *, releases: str = RELEASES) -> int:
    """Install release-plz `version` for this host into `into`, cargo's bin by default.

    Says what it installed and where, says so when that directory is not on
    PATH, and exits non-zero naming why when it installed nothing.
    """
    destination = into if into is not None else cargo_bin()
    try:
        held_to_producer(releases, RELEASES)
        archive = archive_for(version, sys.platform, platform.machine())
        installed = install(archive, destination, releases=releases)
    except InstallerError as refused:
        print(f"install-release-plz: {refused}", file=sys.stderr)
        return 1
    return announce(
        "install-release-plz", f"release-plz {version}", installed, destination, "release-plz"
    )
