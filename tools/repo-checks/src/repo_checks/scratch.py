"""Scratch copies of this repository's tree, and moving one's version as release automation does.

The suites that prove a property of the committed tree against a change to it
work on a copy, so the tree itself is never written to. The change most worth
proving against is the one release automation makes to every release pull
request: it moves the workspace version and regenerates nothing, so anything in
the tree that restates that version as text is stale on that pull request.
"""

from __future__ import annotations

import re
import shutil
import tomllib
from pathlib import Path

from repo_checks.shell import run

#: The JavaScript dependencies a copy links to rather than installs again: the
#: formatter and compiler a copy runs are the versions the locked install put in
#: the committed tree.
NODE_MODULES = "node_modules"

#: The workspace manifest, whose `[workspace.package]` version every crate
#: inherits.
WORKSPACE_MANIFEST = "Cargo.toml"

#: The lock file release automation rewrites beside the manifests.
LOCK_FILE = "Cargo.lock"


def _tracked(root: Path) -> list[str]:
    """Every file a clone would carry once the change in progress lands."""
    listing = run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root,
        check=True,
    ).stdout
    return [name for name in listing.split("\0") if name]


def copy_tree(source: Path, destination: Path) -> Path:
    """Copy the tree at `source` into a fresh directory a suite may write to."""
    destination.mkdir(parents=True, exist_ok=True)
    for name in _tracked(source):
        found = source / name
        if not found.exists() and not found.is_symlink():
            # A file the index still lists and the working tree no longer has:
            # a deletion nobody has staged yet. The copy is of what a clone
            # would carry, and a clone would not carry it.
            continue
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if found.is_symlink():
            target.symlink_to(found.readlink())
        else:
            shutil.copy2(found, target)
    (destination / NODE_MODULES).symlink_to(source / NODE_MODULES)
    return destination


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
