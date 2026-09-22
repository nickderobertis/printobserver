"""A PowerShell release, laid out the way that project lays one out.

Two suites serve one of these: the one proving the installer itself, and the one
proving that a bootstrap on a host carrying no PowerShell reaches that installer
and gets a `pwsh` it can run. Both need the same thing published — an archive
carrying a program and a runtime file, and the `hashes.sha256` beside it, in the
UTF-16 that project writes it in — so it is written here once.
"""

from __future__ import annotations

import hashlib
import io
import sys
import tarfile

from repo_checks.powershell_release import HASHES, Archive
from standin_release import Release

#: One of the files a real archive carries beside the program: PowerShell is a
#: whole runtime, so what a suite proves is that the archive is unpacked rather
#: than one file picked out of it.
RUNTIME_FILE = "System.Management.Automation.dll"


def program(release: str) -> bytes:
    """A `pwsh` that answers `--version` the way the real one does."""
    return f"#!{sys.executable}\nprint('PowerShell {release}')\n".encode()


def archive_bytes(*, carrying: bytes | None) -> bytes:
    """The archive the release would publish: the program and a runtime file at its root.

    `carrying=None` is an archive with the runtime and no program at all, which
    is what an installer meets when a producer changes its layout.
    """
    buffer = io.BytesIO()
    members = [(RUNTIME_FILE, b"a runtime file the program needs\n", 0o644)]
    if carrying is not None:
        members.insert(0, ("pwsh", carrying, 0o755))
    with tarfile.open(fileobj=buffer, mode="w:gz") as bundle:
        for name, payload, mode in members:
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            info.mode = mode
            bundle.addfile(info, io.BytesIO(payload))
    return buffer.getvalue()


def publish(release: Release, archive: Archive, *, tampered: bool = False) -> None:
    """Put the host's archive and the release's own hashes file on the stand-in.

    The hashes file is UTF-16 with a byte-order mark and names each archive after
    a `*`, exactly as PowerShell publishes it; `tampered` lists the digest of the
    archive that was published and serves a different one under its name.
    """
    listed = archive_bytes(carrying=program(archive.version))
    served = archive_bytes(carrying=program(f"{archive.version}-not-what-the-release-listed"))
    release.files[archive.name] = served if tampered else listed
    release.files[HASHES] = (
        f"{'0' * 64} *powershell-{archive.version}-linux-arm32.tar.gz\n"
        f"{hashlib.sha256(listed).hexdigest()} *{archive.name}\n"
    ).encode("utf-16")
