"""The fixture's LFS remote: a directory reached at a file URL, counting what it serves.

git-lfs hands a `file://` remote to a standalone transfer agent, and
`lfs.standalonetransferagent` names which one. This is that agent, speaking
git-lfs's custom transfer protocol — one JSON message a line on standard input,
one answer a line on standard output — over the remote repository's own
`lfs/objects` store, so everything on the client side is git-lfs's own: what it
asks for, when, and what it does with the answer. What the agent adds is the
one thing a directory cannot report, a line per object served:
`<oid> <bytes>`, appended to the file `LFS_FIXTURE_SERVED` names.

Every message is validated before it is acted on: an event the protocol does
not have, an object id that is not a sha256, or an upload naming no file is
refused with an error naming it, and nothing reaches the store.

`LFS_FIXTURE_REMOTE` names the remote repository's git directory, and
`LFS_FIXTURE_SCRATCH` a directory on the clones' own filesystem, because git-lfs
moves a served object into its store by renaming it.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import NewType

#: A Git LFS object id: the sha256 of the file it stands for.
Oid = NewType("Oid", str)
OID = re.compile(r"[0-9a-f]{64}")


class MessageError(ValueError):
    """Why a message from git-lfs could not be acted on."""


@dataclass(frozen=True, slots=True)
class Init:
    """The session's first message."""


@dataclass(frozen=True, slots=True)
class Download:
    """git-lfs asks for one object."""

    oid: Oid


@dataclass(frozen=True, slots=True)
class Upload:
    """git-lfs hands over one object, from a file of its own."""

    oid: Oid
    path: Path


@dataclass(frozen=True, slots=True)
class Terminate:
    """git-lfs is done with this agent."""


Message = Init | Download | Upload | Terminate


def _oid(raw: dict[str, object]) -> Oid:
    """The message's object id, or refuse one that is no sha256."""
    oid = raw.get("oid")
    if not isinstance(oid, str) or OID.fullmatch(oid) is None:
        msg = f"{oid!r} is not an object id"
        raise MessageError(msg)
    return Oid(oid)


def parse(line: str) -> Message:
    """One message from git-lfs, validated.

    Raises:
        MessageError: If it is no JSON object, names an event the protocol does
            not have, or lacks a field its event needs.
    """
    try:
        raw = json.loads(line)
    except ValueError as error:
        msg = f"{line!r} is not JSON"
        raise MessageError(msg) from error
    if not isinstance(raw, dict):
        msg = f"{line!r} is not a message"
        raise MessageError(msg)
    match raw.get("event"):
        case "init":
            return Init()
        case "download":
            return Download(_oid(raw))
        case "upload":
            path = raw.get("path")
            if not isinstance(path, str) or not Path(path).is_file():
                msg = f"the upload of {raw.get('oid')!r} names no file: {path!r}"
                raise MessageError(msg)
            return Upload(_oid(raw), Path(path))
        case "terminate":
            return Terminate()
        case event:
            msg = f"{event!r} is no event of the transfer protocol"
            raise MessageError(msg)


def _store(oid: Oid) -> Path:
    """Where the remote keeps one object, laid out as git-lfs lays out its own."""
    return Path(os.environ["LFS_FIXTURE_REMOTE"]) / "lfs" / "objects" / oid[:2] / oid[2:4] / oid


def _complete(oid: Oid, **fields: object) -> dict[str, object]:
    """The answer completing one transfer."""
    return {"event": "complete", "oid": oid, **fields}


def _download(request: Download, scratch: Path) -> dict[str, object]:
    """Serve one object, recording its bytes, or say it is not there."""
    source = _store(request.oid)
    if not source.is_file():
        return _complete(request.oid, error={"code": 404, "message": "not found"})
    served = scratch / request.oid
    shutil.copyfile(source, served)
    with Path(os.environ["LFS_FIXTURE_SERVED"]).open("a", encoding="utf-8") as record:
        record.write(f"{request.oid} {served.stat().st_size}\n")
    return _complete(request.oid, path=str(served))


def _upload(request: Upload) -> dict[str, object]:
    """Keep one pushed object in the remote's store."""
    target = _store(request.oid)
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(request.path, target)
    return _complete(request.oid)


def _answer(message: dict[str, object]) -> None:
    """Write one answer line and flush it, which git-lfs waits on."""
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def main() -> int:
    """Answer git-lfs until it says it is done, refusing any message it cannot act on."""
    with tempfile.TemporaryDirectory(dir=os.environ["LFS_FIXTURE_SCRATCH"]) as scratch:
        for line in sys.stdin:
            try:
                request = parse(line)
            except MessageError as error:
                _answer({"error": {"code": 400, "message": str(error)}})
                continue
            match request:
                case Init():
                    _answer({})
                case Download():
                    _answer(_download(request, Path(scratch)))
                case Upload():
                    _answer(_upload(request))
                case Terminate():
                    break
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
