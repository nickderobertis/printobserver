"""A release build of the program, reused while what it was built from is unchanged.

The printer integration tier's three client journeys each stand up a world over
the real `printobserver` program built under the release profile, and on a
runner whose cache carries no release build of this workspace's own crates the
first of them spent minutes compiling it inside a test. So the build is kept, in
a directory of its own, under a key naming the three things a release build is a
function of:

- **source** — every file under `crates/` but each crate's own `tests/`, which
  no release build compiles, and the workspace's `Cargo.toml` and `Cargo.lock`;
- **toolchain** — `rust-toolchain.toml` as declared and `rustc -vV` as
  installed, so a toolchain the declaration resolved differently is a different
  one;
- **profile** — the profile's name, every `[profile]` table the workspace's
  manifest declares, the whole of the workspace's own `.cargo/config.toml`, and
  the variables cargo reads a profile setting or compiler flags from, for every
  target or for one.

A key that matches is the build that key was taken over; any other key builds,
and the build replaces whatever the directory held, so it never holds more than
one program. The directory is what the integration job caches, and this module
is what decides whether a restored copy is used: a copy restored under a key
this tree no longer has is built over rather than trusted.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import NewType

from repo_checks import platforms
from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts.build import PROFILE, BuildError, program

#: Where the kept build lives, under the workspace root. It ignores itself, by
#: a `.gitignore` of its own; the one other place that names it is the
#: integration job's cache step, which `test_release_program.py` holds to it.
DIRECTORY = ".release-program"

#: The field `release-program-key` prints, as the `<field>=<value>` line a
#: workflow step appends to `GITHUB_OUTPUT`.
KEY_FIELD = "key"

#: The file beside a kept program recording the digest it was kept with.
DIGEST_FILE = "program.sha256"

IGNORE_FILE = ".gitignore"

CRATES = "crates"

#: The directory of a crate's own tests, which no release build compiles.
CRATE_TESTS = "tests"

WORKSPACE_FILES = ("Cargo.toml", "Cargo.lock")

TOOLCHAIN_FILE = "rust-toolchain.toml"

CARGO_CONFIG = ".cargo/config.toml"

FLAG_VARIABLES = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS")

PROFILE_VARIABLE_PREFIX = "CARGO_PROFILE_"

#: A variable naming flags for one target is `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`.
TARGET_VARIABLE_PREFIX = "CARGO_TARGET_"
TARGET_VARIABLE_SUFFIX = "_RUSTFLAGS"

BUILD_REMEDY = (
    "fix what cargo reported — or unset CARGO_TARGET_DIR, if it moved the program out of "
    "`target` — and run `just release-program` again"
)

TOOLCHAIN_REMEDY = (
    "unset RUSTC or point it at a Rust compiler, and install the toolchain "
    "`rust-toolchain.toml` declares with `rustup toolchain install`"
)


#: A SHA-256 digest, as the lowercase hexadecimal it is written in.
Digest = NewType("Digest", str)


@dataclass(frozen=True, slots=True)
class Key:
    """What a release build is a function of, one digest per input."""

    source: Digest
    toolchain: Digest
    profile: Digest

    @property
    def value(self) -> Digest:
        """The one name the three digests are kept under."""
        joined = f"source={self.source}\ntoolchain={self.toolchain}\nprofile={self.profile}\n"
        return Digest(hashlib.sha256(joined.encode()).hexdigest())


@dataclass(frozen=True, slots=True)
class Prepared:
    """A release program, and whether it was reused rather than built."""

    path: Path
    key: Key
    reused: bool


def key(root: Path) -> Key:
    """The key this workspace's release build is kept under now.

    Raises:
        BuildError: If the toolchain does not say what it is.
    """
    return Key(source=_source(root), toolchain=_toolchain(root), profile=_profile(root))


def prepared(repo: Repo) -> Prepared:
    """The release program for this tree: the kept one when its key holds, else built.

    Raises:
        BuildError: If a build was needed and failed.
    """
    kept = repo.root / DIRECTORY
    before = key(repo.root)
    entry = kept / before.value / platforms.host(repo).program
    if _intact(entry):
        return Prepared(entry, before, reused=True)
    try:
        built = program(repo)
    except BuildError as failed:
        msg = f"{failed}\nNothing was kept; {BUILD_REMEDY}"
        raise BuildError(msg) from failed
    after = key(repo.root)
    if after != before:
        # The tree moved under the build, so what it built is of neither key:
        # it is handed back for this one use and kept under nothing.
        return Prepared(built, after, reused=False)
    _keep(built, kept, entry)
    return Prepared(entry, before, reused=False)


def _keep(built: Path, kept: Path, entry: Path) -> None:
    """Put `built` at `entry`, with its digest beside it, as the only program `kept` holds."""
    if kept.exists():
        shutil.rmtree(kept)
    staging = kept / f"{entry.parent.name}.partial"
    staging.mkdir(parents=True)
    (kept / IGNORE_FILE).write_text("*\n", encoding="utf-8")
    shutil.copy2(built, staging / entry.name)
    (staging / DIGEST_FILE).write_text(_digest(staging / entry.name), encoding="utf-8")
    staging.rename(entry.parent)


def _intact(entry: Path) -> bool:
    """Whether `entry` is the program that was kept, byte for byte.

    A kept directory is restored from a cache rather than written here, so a
    file that is merely there — missing, truncated, or kept with no record of
    what it was — is built over rather than run.
    """
    recorded = entry.parent / DIGEST_FILE
    if not entry.is_file() or not recorded.is_file():
        return False
    return recorded.read_bytes().strip() == _digest(entry).encode()


def _digest(path: Path) -> Digest:
    with path.open("rb") as handle:
        return Digest(hashlib.file_digest(handle, "sha256").hexdigest())


def _source(root: Path) -> Digest:
    """The digest of every file a release build of this workspace reads."""
    digest = hashlib.sha256()
    for path in _sources(root):
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return Digest(digest.hexdigest())


def _sources(root: Path) -> list[Path]:
    """The workspace files and every crate file outside each crate's own tests."""
    found = [root / name for name in WORKSPACE_FILES if (root / name).is_file()]
    crates = root / CRATES
    if crates.is_dir():
        for crate in sorted(crates.iterdir()):
            if not crate.is_dir():
                continue
            for top in sorted(crate.iterdir()):
                if top.name in {CRATE_TESTS, "target"}:
                    continue
                if top.is_file():
                    found.append(top)
                else:
                    found.extend(path for path in sorted(top.rglob("*")) if path.is_file())
    return found


def _toolchain(root: Path) -> Digest:
    """The digest of the toolchain declared and the one installed.

    Raises:
        BuildError: If the compiler does not answer what it is.
    """
    declared = root / TOOLCHAIN_FILE
    answered = run([os.environ.get("RUSTC", "rustc"), "-vV"], cwd=root, timeout=300)
    if answered.returncode != 0:
        msg = (
            f"`rustc -vV` failed, so the toolchain cannot be named; {TOOLCHAIN_REMEDY}:\n"
            f"{answered.stderr}"
        )
        raise BuildError(msg)
    lines = answered.stdout.splitlines()
    if (
        not lines
        or not lines[0].startswith("rustc ")
        or not any(line.startswith("release: ") for line in lines)
    ):
        msg = (
            "`rustc -vV` answered no compiler version, so the toolchain cannot be named; "
            f"{TOOLCHAIN_REMEDY}:\n{answered.stdout}"
        )
        raise BuildError(msg)
    digest = hashlib.sha256()
    digest.update(declared.read_bytes() if declared.is_file() else b"")
    digest.update(b"\0")
    digest.update(answered.stdout.encode())
    return Digest(digest.hexdigest())


def _profile(root: Path) -> Digest:
    """The digest of the profile and compiler flags the workspace and environment set.

    The manifest's `[profile]` tables, the workspace's own `.cargo/config.toml`
    whole — its `[build]` and `[target]` flags as much as its profiles — and
    every variable cargo reads a profile setting or compiler flags from.
    """
    declared = {
        WORKSPACE_FILES[0]: (_toml(root / WORKSPACE_FILES[0]) or {}).get("profile"),
        CARGO_CONFIG: _toml(root / CARGO_CONFIG),
    }
    variables = {
        name: value
        for name, value in sorted(os.environ.items())
        if name in FLAG_VARIABLES
        or name.startswith(PROFILE_VARIABLE_PREFIX)
        or (name.startswith(TARGET_VARIABLE_PREFIX) and name.endswith(TARGET_VARIABLE_SUFFIX))
    }
    document = {"name": PROFILE, "declared": declared, "variables": variables}
    return Digest(hashlib.sha256(json.dumps(document, sort_keys=True).encode()).hexdigest())


def _toml(path: Path) -> dict[str, object] | None:
    """One TOML file's document, or nothing when it is absent."""
    if not path.is_file():
        return None
    with path.open("rb") as handle:
        return tomllib.load(handle)
