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
- **profile** — the profile's name, every `[profile]` table the workspace and
  `.cargo/config.toml` declare, and the variables cargo reads a profile or a
  compiler flag from.

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

from repo_checks import platforms
from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts.build import PROFILE, BuildError, program

#: Where the kept build lives, under the workspace root.
DIRECTORY = ".release-program"

#: The file beside a kept program recording the digest it was kept with.
DIGEST_FILE = "program.sha256"

#: Where the workspace's crates are, under its root.
CRATES = "crates"

#: The directory of a crate's own tests, which no release build compiles.
CRATE_TESTS = "tests"

#: The workspace files a build reads beside the crates.
WORKSPACE_FILES = ("Cargo.toml", "Cargo.lock")

#: The file a toolchain is declared in.
TOOLCHAIN_FILE = "rust-toolchain.toml"

#: Where cargo reads a profile from besides the workspace's manifest.
CARGO_CONFIG = ".cargo/config.toml"

#: The variables cargo reads compiler flags from.
FLAG_VARIABLES = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS")

#: The prefix of every variable cargo reads a profile setting from.
PROFILE_VARIABLE_PREFIX = "CARGO_PROFILE_"


@dataclass(frozen=True, slots=True)
class Key:
    """What a release build is a function of, one digest per input."""

    source: str
    toolchain: str
    profile: str

    @property
    def value(self) -> str:
        """The one name the three digests are kept under."""
        joined = f"source={self.source}\ntoolchain={self.toolchain}\nprofile={self.profile}\n"
        return hashlib.sha256(joined.encode()).hexdigest()


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


def prepared(repo: Repo, directory: Path | None = None) -> Prepared:
    """The release program for this tree: the kept one when its key holds, else built.

    Args:
        repo: The workspace to build.
        directory: Where the kept build lives; `DIRECTORY` under the root when
            omitted.

    Raises:
        BuildError: If a build was needed and failed.
    """
    kept = repo.root / DIRECTORY if directory is None else directory
    before = key(repo.root)
    entry = kept / before.value / platforms.host(repo).program
    if _intact(entry):
        return Prepared(entry, before, reused=True)
    built = program(repo)
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
    shutil.copy2(built, staging / entry.name)
    (staging / DIGEST_FILE).write_text(_digest(staging / entry.name), encoding="utf-8")
    staging.rename(entry.parent)


def _intact(entry: Path) -> bool:
    """Whether `entry` is the program that was kept: the same bytes, and runnable.

    A kept directory is restored from a cache rather than written here, so a
    file that is merely there — truncated, damaged, or no longer executable —
    is built over rather than run.
    """
    recorded = entry.parent / DIGEST_FILE
    if not entry.is_file() or not recorded.is_file():
        return False
    if recorded.read_text(encoding="utf-8").strip() != _digest(entry):
        return False
    return os.access(entry, os.X_OK)


def _digest(path: Path) -> str:
    """The SHA-256 of one file's bytes."""
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def _source(root: Path) -> str:
    """The digest of every file a release build of this workspace reads."""
    digest = hashlib.sha256()
    for path in _sources(root):
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


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


def _toolchain(root: Path) -> str:
    """The digest of the toolchain declared and the one installed.

    Raises:
        BuildError: If the compiler does not answer what it is.
    """
    declared = root / TOOLCHAIN_FILE
    answered = run([os.environ.get("RUSTC", "rustc"), "-vV"], cwd=root, timeout=300)
    if answered.returncode != 0:
        msg = f"`rustc -vV` failed, so the toolchain cannot be named:\n{answered.stderr}"
        raise BuildError(msg)
    lines = answered.stdout.splitlines()
    if (
        not lines
        or not lines[0].startswith("rustc ")
        or not any(line.startswith("release: ") for line in lines)
    ):
        msg = (
            "`rustc -vV` answered no compiler version, so the toolchain cannot be named:\n"
            f"{answered.stdout}"
        )
        raise BuildError(msg)
    digest = hashlib.sha256()
    digest.update(declared.read_bytes() if declared.is_file() else b"")
    digest.update(b"\0")
    digest.update(answered.stdout.encode())
    return digest.hexdigest()


def _profile(root: Path) -> str:
    """The digest of the profile a release build is made under, wherever it is set."""
    declared = {name: _profiles(root / name) for name in (WORKSPACE_FILES[0], CARGO_CONFIG)}
    variables = {
        name: value
        for name, value in sorted(os.environ.items())
        if name in FLAG_VARIABLES or name.startswith(PROFILE_VARIABLE_PREFIX)
    }
    document = {"name": PROFILE, "declared": declared, "variables": variables}
    return hashlib.sha256(json.dumps(document, sort_keys=True).encode()).hexdigest()


def _profiles(path: Path) -> object:
    """Every `[profile]` table one file declares, or nothing when it is absent."""
    if not path.is_file():
        return None
    with path.open("rb") as handle:
        return tomllib.load(handle).get("profile")
