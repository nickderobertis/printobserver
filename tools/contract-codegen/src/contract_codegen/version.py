"""Where each client's `CONTRACT_VERSION` comes from, stated once.

No generated file carries the workspace version as text. Release automation
moves that version on every release pull request and regenerates nothing, so a
client that restated it would be stale on exactly the pull request a release is
cut from. Instead:

- the Rust client reads it from the crate it is compiled as, which inherits the
  workspace's version;
- the Python and Node clients, used from a checkout, read it at import time out
  of the workspace manifest of the tree they sit in, found relative to their
  own module;
- the Python and Node packages a release publishes carry it as a literal,
  stamped over that computation by `release_artifacts` when they are built.

The computation sits between two marker comments so that the stamp replaces
exactly it, and refuses a module in which it cannot find them.
"""

from __future__ import annotations

from pathlib import PurePosixPath

#: The manifest whose `[workspace.package]` version every crate inherits,
#: relative to the repository root.
WORKSPACE_MANIFEST = "Cargo.toml"

#: The generated module each client declares `CONTRACT_VERSION` in, relative to
#: the repository root.
RUST_MODULE = "crates/printobserver-sdk/src/contract.rs"
PYTHON_MODULE = "python/printobserver-sdk/src/printobserver_sdk/contract.py"
NODE_MODULE = "npm/printobserver-sdk/src/contract.ts"

#: SemVer's numeric identifier, which carries no leading zero: `01.2.3` is no
#: version Cargo accepts, so no client may report one.
_NUMBER = r"(?:0|[1-9]\d*)"
_PRERELEASE = r"(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
_BUILD = r"[0-9A-Za-z-]+"

#: What a version the workspace declares must look like for a client to report
#: it: Cargo's own shape, a SemVer version. Written in the syntax Python's and
#: JavaScript's regular expressions share, because both clients test it, and in
#: pieces short enough that a generated module can state it one piece per line.
VERSION_PATTERN_PIECES = (
    rf"{_NUMBER}\.{_NUMBER}\.{_NUMBER}",
    rf"(?:-{_PRERELEASE}",
    rf"(?:\.{_PRERELEASE})*)?",
    rf"(?:\+{_BUILD}(?:\.{_BUILD})*)?",
)
VERSION_PATTERN = "".join(VERSION_PATTERN_PIECES)

#: The two comment lines around the computation in each generated module. They
#: are what `release_artifacts` finds it by: the stamp replaces exactly what lies
#: between them and refuses a module in which they are not both there once.
BEGIN = "contract-version: read from the workspace manifest, from here"
END = "contract-version: read from the workspace manifest, to here"


def manifest_from(module: str) -> str:
    """The workspace manifest, as a path relative to the directory `module` sits in.

    Derived from where the generator writes the module rather than written
    beside each emitter, so moving a client moves the path it reads with it.
    """
    depth = len(PurePosixPath(module).parent.parts)
    return "../" * depth + WORKSPACE_MANIFEST
