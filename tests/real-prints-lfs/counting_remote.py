"""The fixture's LFS remote: a directory reached at a file URL, counting what it serves.

git-lfs hands a `file://` remote to a standalone transfer agent, and
`lfs.standalonetransferagent` names which one. This is that agent, speaking
git-lfs's custom transfer protocol — one JSON message a line on standard input,
one answer a line on standard output — over the remote repository's own
`lfs/objects` store, so everything on the client side is git-lfs's own: what it
asks for, when, and what it does with the answer. What the agent adds is the
one thing a directory cannot report, a line per object served:
`<oid> <bytes>`, appended to the file `LFS_FIXTURE_SERVED` names.

`LFS_FIXTURE_REMOTE` names the remote repository's git directory, and
`LFS_FIXTURE_SCRATCH` a directory on the clones' own filesystem, because git-lfs
moves a served object into its store by renaming it.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any


def _store(oid: str) -> Path:
    """Where the remote keeps one object, laid out as git-lfs lays out its own."""
    return Path(os.environ["LFS_FIXTURE_REMOTE"]) / "lfs" / "objects" / oid[:2] / oid[2:4] / oid


def _answer(message: dict[str, Any]) -> None:
    """Write one answer line and flush it, which git-lfs waits on."""
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def _download(request: dict[str, Any], scratch: Path) -> dict[str, Any]:
    """Serve one object, recording its bytes, or say it is not there."""
    oid = request["oid"]
    source = _store(oid)
    if not source.is_file():
        return {"event": "complete", "oid": oid, "error": {"code": 404, "message": "not found"}}
    served = scratch / oid
    shutil.copyfile(source, served)
    with Path(os.environ["LFS_FIXTURE_SERVED"]).open("a", encoding="utf-8") as record:
        record.write(f"{oid} {served.stat().st_size}\n")
    return {"event": "complete", "oid": oid, "path": str(served)}


def _upload(request: dict[str, Any]) -> dict[str, Any]:
    """Keep one pushed object in the remote's store."""
    oid = request["oid"]
    target = _store(oid)
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(request["path"], target)
    return {"event": "complete", "oid": oid}


def main() -> int:
    """Answer git-lfs until it says it is done."""
    with tempfile.TemporaryDirectory(dir=os.environ["LFS_FIXTURE_SCRATCH"]) as scratch:
        for line in sys.stdin:
            request = json.loads(line)
            event = request.get("event")
            if event == "init":
                _answer({})
            elif event == "download":
                _answer(_download(request, Path(scratch)))
            elif event == "upload":
                _answer(_upload(request))
            elif event == "terminate":
                break
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
