"""Every cargo invocation inside a clone builds into that clone's own `target`, with line tables.

`.cargo/config.toml` at the root is the whole of that contract: `build.target-dir`
names the clone's own `target` and `profile.dev.debug` keeps dev and test builds to
line tables. Neither shows in any test's outcome — a build that landed somewhere
else, or one carrying full debuginfo, passes exactly as this one does — so each
journey here reads what cargo itself reports over a copy of the committed tree,
from inside that copy, the way a second clone or worktree would.
"""

from __future__ import annotations

import json
from pathlib import Path

from journey import REPO_ROOT, CopiesTheTree, GateCopy, capture, clean_environment, output
from repo_checks.expect import absent, contains, equal, passing, truth

#: The crate the profile is read off: the one under every other, with the
#: smallest dependency graph to compile into a copy that starts with none built.
CRATE = "printobserver-types"
#: The debuginfo level cargo hands rustc under the dev profile: line tables only.
LINE_TABLES = "-C debuginfo=1"

#: Every variable cargo reads ahead of the file for one of its two keys. The
#: gate's runners set one of them: `actions-rust-lang/setup-rust-toolchain`
#: exports `CARGO_PROFILE_DEV_DEBUG=0` to keep its cache small, and under it the
#: copy built at level 0 — carrying `-C strip=debuginfo` and no `debuginfo=` at
#: all — while the file still said 1. What these journeys prove is the file, so
#: cargo is handed none of them, and the one journey that sets one sets it after.
OVERRIDES = (
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET_DIR",
    "CARGO_PROFILE_DEV_DEBUG",
    "CARGO_PROFILE_TEST_DEBUG",
)

#: A crate that is no member of the workspace, planted inside the copy: its own
#: `[workspace]` table is what keeps cargo from reading it as a stray member of
#: the one above it, and it depends on nothing so a build of it costs nothing.
BESIDE = "beside"
BESIDE_MANIFEST = f'''[package]
name = "{BESIDE}"
version = "0.0.0"
edition = "2024"

[workspace]
'''


def _file_alone(**extra: str) -> dict[str, str]:
    """The environment cargo reads the copy's `.cargo/config.toml` under, and nothing over it."""
    environment = clean_environment()
    for name in OVERRIDES:
        environment.pop(name, None)
    environment.update(extra)
    return environment


def _metadata_target(cwd: Path, **environment: str) -> Path:
    """Where cargo, asked from `cwd`, says it will build.

    `--no-deps` so the answer costs no resolution, and resolved because
    pytest's temporary directory may sit behind a symbolic link cargo has
    already followed.
    """
    result = capture(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd,
        timeout=120,
        env=_file_alone(**environment),
    )
    passing(result, describing=f"`cargo metadata` from {cwd}")
    metadata = json.loads(result.stdout)
    reported = metadata.get("target_directory") if isinstance(metadata, dict) else None
    truth(
        isinstance(reported, str),
        describing="`cargo metadata` to answer an object naming `target_directory`",
    )
    return Path(str(reported)).resolve()


def _planted_beside(copy: GateCopy) -> Path:
    copy.write(f"{BESIDE}/Cargo.toml", BESIDE_MANIFEST)
    copy.write(f"{BESIDE}/src/lib.rs", "//! A crate outside the workspace.\n")
    return copy.root / BESIDE


def _unit(cwd: Path, command: list[str], crate: str) -> str:
    """The one `rustc` invocation for `crate`, read off the verbose `command` run from `cwd`.

    `cargo check` where the crate has dependencies, because the profile and
    the target directory are settled before rustc runs and show on its command
    line whether or not it emits code; a full build of the copy would prove the
    same two things at the cost of linking every dependency.
    """
    result = capture(command, cwd, timeout=900, env=_file_alone())
    passing(result, describing=f"`{' '.join(command)}`")
    unit = f"--crate-name {crate.replace('-', '_')} "
    invocations = [line for line in output(result).splitlines() if unit in line]
    equal(len(invocations), 1, describing=f"the number of rustc invocations for {crate}")
    return invocations[0]


def _check(copy: GateCopy, *flags: str) -> str:
    return _unit(copy.root, ["cargo", "check", "-v", "-p", CRATE, *flags], CRATE)


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_cargo_builds_into_the_clones_own_target_from_anywhere_inside_it(
    gate_copy: CopiesTheTree,
) -> None:
    """A copy of the tree resolves `target` against its own root, not this checkout's.

    From the workspace root, from inside a member crate and from a crate that
    is no member at all: the file is found by walking up from wherever cargo
    was invoked, and its `target-dir` is resolved against the directory holding
    `.cargo/`, so two clones never build into one directory and a clone never
    builds into two.
    """
    copy = gate_copy(node_modules=False)
    own = (copy.root / "target").resolve()
    beside = _planted_beside(copy)

    equal(_metadata_target(copy.root), own, describing="the target directory from the root")
    equal(
        _metadata_target(copy.root / "crates" / CRATE),
        own,
        describing="the target directory from inside a member crate",
    )
    equal(
        _metadata_target(beside),
        own,
        describing="the target directory from a crate outside the workspace",
    )
    truth(
        own != (REPO_ROOT / "target").resolve(),
        describing="the copy's target directory to be its own rather than this checkout's",
    )

    built = _unit(beside, ["cargo", "build", "-v"], BESIDE)
    contains(built, LINE_TABLES, describing="the non-member crate's rustc invocation")
    contains(
        built,
        f"--out-dir {copy.root / 'target' / 'debug' / 'deps'}",
        describing="the non-member crate's rustc invocation",
    )


def test_an_invocation_naming_its_own_target_directory_still_wins(
    gate_copy: CopiesTheTree, tmp_path: Path
) -> None:
    """`CARGO_TARGET_DIR` overrides the file, so a caller that needs another directory has one.

    The Windows lint journey depends on this: it builds a copy into this
    checkout's `target` so the pass does not compile the workspace from
    nothing, and a contract that trapped it would cost that tier its cache.
    """
    copy = gate_copy(node_modules=False)
    elsewhere = tmp_path / "elsewhere"

    equal(
        _metadata_target(copy.root, CARGO_TARGET_DIR=str(elsewhere)),
        elsewhere.resolve(),
        describing="the target directory under `CARGO_TARGET_DIR`",
    )


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_dev_builds_carry_line_tables_and_release_builds_carry_none(
    gate_copy: CopiesTheTree,
) -> None:
    """Dev and test hand rustc `debuginfo=1` under the copy's `target`; release hands it none.

    Read off the invocation cargo prints for the crate's own unit rather than
    off the profile summary, because `[unoptimized + debuginfo]` is what cargo
    says at every level above zero. The `test` profile is asked for by name:
    it inherits `dev`, and this is what says nothing in the tree sets it back.
    """
    copy = gate_copy(node_modules=False)
    debug_deps = f"--out-dir {copy.root / 'target' / 'debug' / 'deps'}"

    dev = _check(copy)
    contains(dev, LINE_TABLES, describing="the dev rustc invocation")
    contains(dev, debug_deps, describing="the dev rustc invocation")
    truth(
        (copy.root / "target" / "debug").is_dir(),
        describing="the dev build to have landed under the copy's own `target`",
    )

    test = _check(copy, "--profile", "test")
    contains(test, LINE_TABLES, describing="the test rustc invocation")
    contains(test, debug_deps, describing="the test rustc invocation")

    release = _check(copy, "--release")
    absent(release, "-C debuginfo=", describing="the release rustc invocation")
    contains(
        release,
        f"--out-dir {copy.root / 'target' / 'release' / 'deps'}",
        describing="the release rustc invocation",
    )
