"""Assembling a package of the JavaScript registry, and a release tarball.

Both are tar archives with a manifest, and both are written here rather than by
driving a packer, for the same reason the wheels are: what the command-line
distribution publishes is a **per-platform** package the caller's own package
manager selects, and the launcher beside it names those packages by version — so
the two are assembled together or they do not resolve.
"""

from __future__ import annotations

import gzip
import hashlib
import io
import json
import tarfile
from dataclasses import dataclass, field
from pathlib import Path

#: The directory every package of the JavaScript registry is unpacked from.
PACKAGE_ROOT = "package"

#: A fixed instant every archive records, so that one built twice from one tree
#: is the same bytes twice. A release artifact whose digest moved without its
#: content moving is one nobody can verify.
FIXED_TIME = 0


@dataclass(slots=True)
class Archive:
    """One gzipped tar being assembled."""

    #: What goes in it, by the path it takes inside the archive.
    contents: dict[str, bytes] = field(default_factory=dict)
    #: Which of those are executable.
    executable: set[str] = field(default_factory=set)

    def add(self, inside: str, content: bytes, *, executable: bool = False) -> None:
        """Put one file in the archive."""
        self.contents[inside] = content
        if executable:
            self.executable.add(inside)

    def write(self, target: Path) -> Path:
        """Write the archive, and answer where it was written."""
        target.parent.mkdir(parents=True, exist_ok=True)
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w") as archive:
            for inside, content in sorted(self.contents.items()):
                info = tarfile.TarInfo(inside)
                info.size = len(content)
                info.mtime = FIXED_TIME
                info.mode = 0o755 if inside in self.executable else 0o644
                archive.addfile(info, io.BytesIO(content))
        target.write_bytes(gzip.compress(raw.getvalue(), mtime=FIXED_TIME))
        return target


@dataclass(frozen=True, slots=True)
class NodePackage:
    """What one package of the JavaScript registry says about itself."""

    name: str
    version: str
    description: str
    license: str
    repository: str

    def manifest(self, **rest: object) -> dict[str, object]:
        """The `package.json` a registry and an installed package are read from."""
        return {
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "license": self.license,
            "repository": {"type": "git", "url": self.repository},
            **rest,
        }

    @property
    def file_name(self) -> str:
        """What the packed tarball is called, as `npm pack` names one."""
        stem = self.name.removeprefix("@").replace("/", "-")
        return f"{stem}-{self.version}.tgz"


def packed(package: NodePackage, manifest: dict[str, object], archive: Archive, into: Path) -> Path:
    """Write one package of the JavaScript registry."""
    archive.add(
        f"{PACKAGE_ROOT}/package.json",
        (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode(),
    )
    return archive.write(into / package.file_name)


def digest_of(path: Path) -> str:
    """One artifact's digest, as the checksum file states it."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checksums(paths: list[Path]) -> bytes:
    """The checksum file a release publishes beside its artifacts.

    The same format `sha256sum` writes and reads, so a caller with no script at
    all can verify a download with the tool their machine already has.
    """
    return "".join(f"{digest_of(path)}  {path.name}\n" for path in sorted(paths)).encode()
