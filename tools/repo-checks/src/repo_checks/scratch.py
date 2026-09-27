"""Scratch copies of this repository's tree.

The suites that prove a property of the committed tree against a change to it
work on a copy, so the tree itself is never written to.
"""

from __future__ import annotations

import shutil
from pathlib import Path

from repo_checks.shell import run

#: The JavaScript dependencies a copy links to rather than installs again: the
#: formatter and compiler a copy runs are the versions the locked install put in
#: the committed tree.
NODE_MODULES = "node_modules"


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
