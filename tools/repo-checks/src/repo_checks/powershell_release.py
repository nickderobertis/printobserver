"""One exact PowerShell release, installed from its own published archive.

The install-script and Windows service journeys run under `pwsh` on Linux and
macOS as well as Windows. This installer supplies it where the host carries
neither `pwsh` nor `powershell`.

`pwsh` is a whole .NET runtime rather than one file, so the release's archive is
unpacked whole into a directory of its own and the program inside it is linked
into the directory the caller asked for. The release publishes `hashes.sha256`
beside its archives, and nothing is unpacked until the download matches the
digest that file states for it.

A **Windows** host is refused by name, and that is the point rather than a gap:
every Windows host carries `powershell`, which is what
`release_artifacts.installing` runs a script under where `pwsh` is absent, so
there is nothing for this to install.
"""

from __future__ import annotations

import os
import platform
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import RELEASE
from repo_checks.verified_download import (
    InstallerError,
    digest_for,
    download,
    held_to_producer,
    unpack,
    verified,
)

#: Where every PowerShell release publishes its archives and its hashes file.
RELEASES = "https://github.com/PowerShell/PowerShell/releases/download"

#: The name of the checksums file every release publishes beside its archives.
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
HASHES = "hashes.sha256"

#: What the release calls the build for each host it can be installed on, keyed
#: by what the interpreter answers for the operating system and the processor.
#: `win32` is deliberately absent: a Windows host already carries PowerShell.
#:
#: These names are the producer's rather than this repository's, and what
#: reconciles them with it is the release's own `hashes.sha256`: it lists one
#: line per archive the release publishes, by name, and `_expected_digest`
#: refuses an archive name that file does not list — naming the file and the
#: name it looked for. So a flavour PowerShell renames stops this installer
#: with the producer's own listing as the evidence, rather than downloading
#: something else or silently installing an older build.
#:
#: `tests/test_powershell_release.py` drives that refusal against a stand-in
#: release serving a hashes file that lists another name.
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
FLAVOURS = {
    ("linux", "x86_64"): "linux-x64",
    ("linux", "aarch64"): "linux-arm64",
    ("darwin", "aarch64"): "osx-arm64",
    ("darwin", "x86_64"): "osx-x64",
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


@dataclass(frozen=True, slots=True)
class Archive:
    """The one archive of a release built for this host."""

    #: The release, without its leading `v`.
    version: str
    #: The build as the release names it: `linux-x64`, `osx-arm64`, and so on.
    flavour: str

    @property
    def name(self) -> str:
        """The archive's file name, as `hashes.sha256` lists it."""
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] see suppressions.toml.
        return f"powershell-{self.version}-{self.flavour}.tar.gz"


def archive_for(version: str, system: str, machine: str) -> Archive:
    """The archive of `version` built for one host.

    Raises:
        InstallerError: If `version` is not a release, or the host is one this
            installer does not install PowerShell on.
    """
    if not RELEASE.fullmatch(version):
        msg = f"`{version}` is not a release: name one as `7.6.6`"
        raise InstallerError(msg)
    flavour = FLAVOURS.get((system, ARCHITECTURES.get(machine.lower(), machine.lower())))
    if flavour is None:
        if system == "win32":
            msg = (
                f"a Windows host carries PowerShell already — `powershell` at the least — so "
                f"there is nothing here to install. This installer takes PowerShell {version} "
                f"for Linux and macOS."
            )
            raise InstallerError(msg)
        msg = (
            f"this installer takes PowerShell for {', '.join(sorted(FLAVOURS.values()))}, and "
            f"this host is {system} on {machine}. Install PowerShell {version} from "
            f"https://github.com/PowerShell/PowerShell/releases/tag/v{version} yourself."
        )
        raise InstallerError(msg)
    return Archive(version, flavour)


def listing(payload: bytes) -> str:
    """A release's `hashes.sha256`, whatever the producer encoded it as.

    PowerShell writes that file as UTF-16 with a byte-order mark, which read as
    UTF-8 is one line of interleaved NULs that lists nothing. The mark is what
    says which it is, so this asks rather than assuming either.
    """
    if payload[:2] in (b"\xff\xfe", b"\xfe\xff"):
        return payload.decode("utf-16", errors="replace")
    return payload.decode("utf-8", errors="replace")


def _expected_digest(hashes: str, archive: Archive) -> str:
    """The SHA-256 the release's hashes file lists for one archive.

    Raises:
        InstallerError: If the file lists none for it.
    """
    listed = digest_for(hashes, archive.name)
    if listed is None:
        msg = f"the release's {HASHES} lists no checksum for {archive.name}"
        raise InstallerError(msg)
    return listed


def runtime_directory(into: Path, version: str) -> Path:
    """Where the unpacked runtime goes, given the directory the program is linked into.

    `~/.local/bin` beside `~/.local/share/powershell-<version>`: the program
    directory holds programs and the runtime sits beside it, which is the layout
    a host that already carries a hand-installed PowerShell 7 uses.
    """
    return into.parent / "share" / f"powershell-{version}"


def install(archive: Archive, into: Path, *, releases: str = RELEASES) -> Path:
    """Download, verify and install one archive's PowerShell, linked into `into`.

    Nothing is unpacked until the archive has matched the digest the release's
    own hashes file lists for it.

    The archive is unpacked beside the runtime directory and moved onto it only
    once the program is there, so an archive that turns out to carry none leaves
    the runtime a previous install put there as it was.

    A runtime already installed is never deleted before its replacement is in
    place: it is renamed aside first, the staged one is moved onto the name it
    vacated, and only then is the old one removed. There is therefore no moment
    at which the working runtime has been destroyed and the new one is not yet
    there — the state an install interrupted between a delete and a move would
    otherwise leave, which is a `pwsh` on PATH pointing at nothing.

    A refused replacement names both locations where the prior runtime can be,
    so an operator can put it back if the filesystem stopped the swap.

    Raises:
        InstallerError: If a download fails, the digests disagree, the archive
            cannot be unpacked, it carries no program, or the runtime already
            installed cannot be replaced.
    """
    # Absolute, whatever the caller passed: the runtime goes beside `into` and
    # `pwsh` inside it is linked into `into`, so a relative directory would put
    # the link's own target under the link's directory, where nothing is.
    into = into.resolve()
    held_to_producer(releases, RELEASES)
    base = f"{releases}/v{archive.version}"
    hashes = listing(download(f"{base}/{HASHES}"))
    payload = verified(
        f"{base}/{archive.name}",
        name=archive.name,
        expected=_expected_digest(hashes, archive),
        source=f"the release's {HASHES}",
    )
    runtime = runtime_directory(into, archive.version)
    staged = runtime.with_name(f".{runtime.name}.part")
    try:
        shutil.rmtree(staged)
    except FileNotFoundError:
        pass
    except OSError as failed:
        msg = f"{staged} staging directory could not be cleared: {failed}"
        raise InstallerError(msg) from failed
    unpack(payload, name=archive.name, into=staged)
    if not (staged / "pwsh").is_file():
        shutil.rmtree(staged, ignore_errors=True)
        msg = f"{archive.name} carries no pwsh: it unpacks to no such program"
        raise InstallerError(msg)
    try:
        # The file was just unpacked by this process. Making it executable
        # before the directory swap keeps a chmod refusal off the live link.
        (staged / "pwsh").chmod(0o755)
    except OSError as failed:
        shutil.rmtree(staged, ignore_errors=True)
        msg = f"{archive.name}'s pwsh could not be made executable: {failed}"
        raise InstallerError(msg) from failed
    superseded = runtime.with_name(f".{runtime.name}.superseded")
    if superseded.is_dir():
        if not runtime.exists():
            try:
                # Both entries share the directory just used for staging, so
                # only an external filesystem change can refuse this rename.
                superseded.replace(runtime)
            except OSError as failed:
                shutil.rmtree(staged, ignore_errors=True)
                msg = (
                    f"the saved PowerShell runtime at {superseded} could not be restored "
                    f"to {runtime}: {failed}. It remains at {superseded}."
                )
                raise InstallerError(msg) from failed
        elif (runtime / "pwsh").is_file():
            shutil.rmtree(superseded)
        else:
            shutil.rmtree(staged, ignore_errors=True)
            msg = (
                f"{runtime} could not be replaced: another entry holds its name while the "
                f"previous PowerShell runtime is at {superseded}. Move that entry away, "
                f"then install again."
            )
            raise InstallerError(msg)
    try:
        if runtime.exists():
            runtime.replace(superseded)
        # llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
        staged.replace(runtime)
    except OSError as failed:
        shutil.rmtree(staged, ignore_errors=True)
        msg = (
            f"{runtime} could not be replaced with the runtime unpacked beside it: {failed}. "
            f"Nothing was deleted: the runtime this was replacing is at {runtime}, or at "
            f"{superseded} if it had already been put aside. `pwsh` is linked to {runtime}, "
            f"so whichever of the two holds it belongs there."
        )
        raise InstallerError(msg) from failed
    shutil.rmtree(superseded, ignore_errors=True)
    program = runtime / "pwsh"
    linked = into / "pwsh"
    staging = into / ".pwsh.part"
    try:
        into.mkdir(parents=True, exist_ok=True)
        staging.unlink(missing_ok=True)
        staging.symlink_to(program)
        staging.replace(linked)
    except OSError as failed:
        msg = (
            f"PowerShell {archive.version} is installed at {runtime}, and `pwsh` could not be "
            f"linked into {into}: {failed}. Link {program} there yourself, or install again "
            f"into a directory this can write."
        )
        raise InstallerError(msg) from failed
    return linked


def install_powershell(version: str, into: Path | None = None, *, releases: str = RELEASES) -> int:
    """Install PowerShell `version` for this host into `into`, `~/.local/bin` by default.

    Says what it installed and where, says so when that directory is not on
    PATH, and exits non-zero naming why when it installed nothing.
    """
    destination = (into if into is not None else Path.home() / ".local" / "bin").resolve()
    try:
        archive = archive_for(version, sys.platform, platform.machine())
        installed = install(archive, destination, releases=releases)
    except InstallerError as refused:
        print(f"install-powershell: {refused}", file=sys.stderr)
        return 1
    print(f"install-powershell: installed PowerShell {version} at {installed}", file=sys.stderr)
    if str(destination) not in os.environ.get("PATH", "").split(os.pathsep):
        print(
            f"install-powershell: {destination} is not on PATH; put it there before running pwsh",
            file=sys.stderr,
        )
    return 0
