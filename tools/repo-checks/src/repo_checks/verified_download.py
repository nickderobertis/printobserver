"""One verified download, shared by every installer that puts a held release on PATH.

`repo-policy.toml` holds three programs at an exact release — GitHub CLI, the
release program and PowerShell — and no package manager installs an exact
release of any of them on every host this repository supports. Each is taken
from the release's own published artifact instead, and none of them reaches a
path until the bytes that were downloaded hash to a SHA-256 somebody can check:
one the release itself publishes, or one this repository commits.

That sequence — refuse an address nothing vouches for, download, hash, compare,
unpack, place — is the same for all three, so it is written here once. What an
installer states is what its release calls its artifacts and where the digest
comes from; nothing about downloading one.
"""

from __future__ import annotations

import hashlib
import io
import tarfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile
import zlib
from pathlib import Path

#: How long one download may take, in seconds.
DOWNLOAD_TIMEOUT = 120

#: The hosts an installer may download from over plain HTTP: the loopback
#: addresses a suite serves an installer its own stand-in release on, and no
#: name that a resolver decides.
LOOPBACK = frozenset({"127.0.0.1", "::1"})

#: The one forge every release these installers take is published on, which is
#: the only host reached over TLS. `https` on its own is not enough for two of
#: the three: gh and PowerShell read the checksums file from the same release
#: the archive comes from, so a host free to serve both would be a host free to
#: serve bytes and the digest vouching for them. Holding the origin to the
#: producer's own forge is what stops `--releases` naming such a host.
#: `test_verified_download.py` holds each installer's own address to it.
FORGE = "github.com"


class InstallerError(Exception):
    """Why a held release could not be installed, in words a caller acts on."""


def permitted(url: str) -> bool:
    """Whether an installer downloads from this address at all.

    The producer's forge over TLS, and a loopback address over plain HTTP, which
    is how a suite serves an installer a stand-in release. `--releases` is a
    caller's input, so this is what keeps one of these installers from being an
    arbitrary downloader — and the host matters as much as the scheme, because
    two of the three installers read the digest that vouches for an archive out
    of a file served beside it. Any `https` host would therefore be a host free
    to hand an installer bytes and its own approval of them together.

    The address is **parsed** rather than read off the front, because everything
    before an `@` in an authority is userinfo: `http://127.0.0.1:80@example.com`
    begins with a loopback address, names `example.com` as its host, and would
    pass any check made on the text.
    """
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme == "https":
        return parsed.hostname == FORGE
    return parsed.scheme == "http" and parsed.hostname in LOOPBACK


def download(url: str) -> bytes:
    """One file of a release, or why it could not be fetched.

    Raises:
        InstallerError: If the address is not one an installer downloads from,
            or the download fails.
    """
    if not permitted(url):
        msg = f"{url} is not an address this installer downloads from"
        raise InstallerError(msg)
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        with urllib.request.urlopen(url, timeout=DOWNLOAD_TIMEOUT) as answer:  # noqa: S310
            return answer.read()
    except (urllib.error.URLError, OSError) as error:
        msg = f"{url} could not be downloaded: {error}"
        raise InstallerError(msg) from error


def verified(url: str, *, name: str, expected: str, source: str) -> bytes:
    """Download one artifact and answer its bytes only if they hash to `expected`.

    `name` is what the artifact is called and `source` is what states the digest
    — the release's own checksums file, or this repository's committed one — so
    that a refusal names both halves of the comparison a reader has to make.

    Raises:
        InstallerError: If the download fails, or the digests disagree.
    """
    payload = download(url)
    actual = hashlib.sha256(payload).hexdigest()
    if actual != expected:
        msg = f"{name} hashes to {actual}, and {source} lists {expected}: nothing was installed"
        raise InstallerError(msg)
    return payload


def digest_for(listing: str, name: str) -> str | None:
    """The SHA-256 a checksums listing states for one file, or none if it states none.

    Both listings this repository reads are one file per line, the digest first
    and the name second: `gh`'s separates them with two spaces, and PowerShell's
    writes the name with the `*` a binary-mode digest carries. Reading them on
    whitespace takes either without a second reader.
    """
    for line in listing.splitlines():
        parts = line.strip().split()
        if len(parts) == 2 and len(parts[0]) == 64 and parts[1].lstrip("*") == name:
            return parts[0].lower()
    return None


def member_of(payload: bytes, *, name: str, member: str) -> bytes:
    """One file out of a verified archive, which is `.zip` or `.tar.gz` by its name.

    Raises:
        InstallerError: If the archive carries no such file, or cannot be read.
    """
    try:
        if name.endswith(".zip"):
            with zipfile.ZipFile(io.BytesIO(payload)) as bundle:
                return bundle.read(member)
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as bundle:
            extracted = bundle.extractfile(member)
            if extracted is None:
                raise KeyError(member)
            return extracted.read()
    except (KeyError, tarfile.TarError, zipfile.BadZipFile, zlib.error) as error:
        msg = f"{name} carries no {member}: {error}"
        raise InstallerError(msg) from error


def unpack(payload: bytes, *, name: str, into: Path) -> None:
    """Every file of a verified `.tar.gz` into a directory of its own.

    A program that is a whole runtime rather than one file is unpacked whole.
    `filter="data"` is the extraction the interpreter vouches for: an entry
    naming a path outside `into`, a link leaving it, or a device is refused
    rather than written.

    Raises:
        InstallerError: If the archive cannot be read, or carries an entry the
            data filter refuses.
    """
    try:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as bundle:
            bundle.extractall(into, filter="data")
    except (tarfile.TarError, zlib.error, OSError) as error:
        msg = f"{name} could not be unpacked into {into}: {error}"
        raise InstallerError(msg) from error


def place(program: bytes, into: Path, name: str) -> Path:
    """Write one program into a directory and make it executable, atomically.

    It is staged beside its own name and moved onto it, so a caller that reaches
    for the program while this runs finds either the copy that was there or the
    whole new one, never a partial file.
    """
    into.mkdir(parents=True, exist_ok=True)
    target = into / name
    staged = into / f".{name}.part"
    staged.write_bytes(program)
    staged.chmod(0o755)
    staged.replace(target)
    return target
