"""Moving a tree's version exactly as release automation's release pull request does.

`release-plz release-pr` moves the workspace version on every release pull
request and regenerates nothing, so anything in the tree that restates that
version as text is stale on that pull request, and fails the gate on the one
change a release is cut from. The suites that hold the tree to surviving that
change apply it to a scratch copy with `release_plz_bump`.
"""

from __future__ import annotations

import re
import tomllib
from pathlib import Path

#: The workspace manifest, whose `[workspace.package]` version every crate
#: inherits.
WORKSPACE_MANIFEST = "Cargo.toml"

#: The lock file release automation rewrites beside the manifests.
LOCK_FILE = "Cargo.lock"


def workspace_version(root: Path) -> str:
    """The version the workspace at `root` declares."""
    with (root / WORKSPACE_MANIFEST).open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["version"])


def next_minor(version: str) -> str:
    """The version a pre-1.0 release after `version` is cut at."""
    major, minor, _ = version.split(".", 2)
    return f"{major}.{int(minor) + 1}.0"


def release_plz_bump(root: Path, version: str) -> str:
    """Move the workspace at `root` to `version` the way a release pull request does.

    Exactly what `release-plz release-pr` writes: the `[workspace.package]`
    version, every internal crate's version requirement wherever a manifest
    names one by path, and the lock file's record of every workspace crate.
    Nothing is regenerated, which is the point.

    Returns:
        The version the workspace declared before.
    """
    was = workspace_version(root)
    quoted = re.escape(f'"{was}"')
    manifests = [root / WORKSPACE_MANIFEST, *sorted(root.glob("crates/*/Cargo.toml"))]
    for manifest in manifests:
        text = manifest.read_text(encoding="utf-8")
        # An internal requirement names its crate by path; an external one of
        # the same number is another project's and stays where it is.
        text = re.sub(
            rf"^(.*\bpath = \"[^\"]+\".*\bversion = ){quoted}",
            rf'\g<1>"{version}"',
            text,
            flags=re.MULTILINE,
        )
        if manifest == root / WORKSPACE_MANIFEST:
            text = re.sub(
                rf"(\[workspace\.package\][^\[]*?^version = ){quoted}",
                rf'\g<1>"{version}"',
                text,
                count=1,
                flags=re.MULTILINE | re.DOTALL,
            )
        manifest.write_text(text, encoding="utf-8")
    lock = root / LOCK_FILE
    blocks = lock.read_text(encoding="utf-8").split("\n[[package]]\n")
    local = {f'name = "{path.parent.name}"' for path in root.glob("crates/*/Cargo.toml")}
    moved = [
        block.replace(f'\nversion = "{was}"\n', f'\nversion = "{version}"\n', 1)
        if block.split("\n", 1)[0] in local and "\nsource = " not in block
        else block
        for block in blocks
    ]
    lock.write_text("\n[[package]]\n".join(moved), encoding="utf-8")
    if workspace_version(root) != version:
        msg = f"{root / WORKSPACE_MANIFEST} still declares {workspace_version(root)}"
        raise RuntimeError(msg)
    return was
