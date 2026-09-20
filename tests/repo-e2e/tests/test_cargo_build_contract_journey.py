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
from collections.abc import Callable
from pathlib import Path

from journey import REPO_ROOT, GateCopy, capture, clean_environment, output
from repo_checks.expect import absent, contains, equal, passing, truth

#: The crate the profile is read off: the one under every other, with the
#: smallest dependency graph to compile into a copy that starts with none built.
CRATE = "printobserver-types"
#: How that crate names itself on its own `rustc` invocation, the space
#: included so `printobserver_types` does not match a crate named after it.
RUSTC_UNIT = f"--crate-name {CRATE.replace('-', '_')} "
#: The debuginfo level cargo hands rustc under the dev profile: line tables only.
LINE_TABLES = "-C debuginfo=1"


def _metadata_target(cwd: Path, **environment: str) -> Path:
    """Where cargo, asked from `cwd`, says it will build."""
    result = capture(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd,
        timeout=120,
        env=clean_environment(**environment),
    )
    passing(result, describing=f"`cargo metadata` from {cwd}")
    return Path(json.loads(result.stdout)["target_directory"]).resolve()


def _unit(copy: GateCopy, *flags: str) -> str:
    """The `rustc` invocation cargo runs for the crate, read off a verbose check.

    `cargo check` because the profile and the target directory are settled
    before rustc runs and show on its command line whether or not it emits
    code; a full build of the copy would prove the same two things at the
    cost of linking every dependency.
    """
    result = capture(
        ["cargo", "check", "-v", "-p", CRATE, *flags],
        copy.root,
        timeout=900,
        env=clean_environment(),
    )
    passing(result, describing=f"`cargo check -v -p {CRATE} {' '.join(flags)}`")
    invocations = [line for line in output(result).splitlines() if RUSTC_UNIT in line]
    equal(len(invocations), 1, describing=f"the number of rustc invocations for {CRATE}")
    return invocations[0]


def test_cargo_builds_into_the_clones_own_target_from_anywhere_inside_it(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A copy of the tree resolves `target` against its own root, not this checkout's.

    From the workspace root and from inside a member crate alike: the file is
    found by walking up from wherever cargo was invoked, and its `target-dir`
    is resolved against the directory holding `.cargo/`, so two clones never
    build into one directory.
    """
    copy = gate_copy(node_modules=False)
    own = (copy.root / "target").resolve()

    equal(_metadata_target(copy.root), own, describing="the target directory from the root")
    equal(
        _metadata_target(copy.root / "crates" / CRATE),
        own,
        describing="the target directory from inside a member crate",
    )
    truth(
        own != (REPO_ROOT / "target").resolve(),
        describing="the copy's target directory to be its own rather than this checkout's",
    )


def test_an_invocation_naming_its_own_target_directory_still_wins(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
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


def test_dev_builds_carry_line_tables_and_release_builds_carry_none(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """Dev hands rustc `debuginfo=1` under the copy's `target`; release hands it no debuginfo.

    Read off the invocation cargo prints for the crate's own unit rather than
    off the profile summary, because `[unoptimized + debuginfo]` is what cargo
    says at every level above zero.
    """
    copy = gate_copy(node_modules=False)

    dev = _unit(copy)
    contains(dev, LINE_TABLES, describing="the dev rustc invocation")
    contains(
        dev,
        f"--out-dir {copy.root / 'target' / 'debug' / 'deps'}",
        describing="the dev rustc invocation",
    )
    truth(
        (copy.root / "target" / "debug").is_dir(),
        describing="the dev build to have landed under the copy's own `target`",
    )

    release = _unit(copy, "--release")
    absent(release, "-C debuginfo=", describing="the release rustc invocation")
    contains(
        release,
        f"--out-dir {copy.root / 'target' / 'release' / 'deps'}",
        describing="the release rustc invocation",
    )
