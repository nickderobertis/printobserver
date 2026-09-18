"""`just lint` on a Unix host reports a finding in the tree's `cfg(windows)` code.

Three rounds of the Windows gate were lost to a lint or a format finding in code
a Linux host never compiles. This journey plants exactly that finding — a
warning, denied, inside a `#[cfg(windows)]` arm — in a real copy of the tree
and runs the committed `lint` recipe over it, the way a developer does before a
push. The native clippy over every crate passes, because the arm is dead code
to it; the recipe still fails, naming the file, the finding and the Windows
target, because the pass after the native lint compiled that arm.

The copy shares this checkout's cargo target directory, so what it pays for is
the workspace's own crates rather than every dependency of the Windows target.
"""

from __future__ import annotations

import platform as host_platform
from collections.abc import Callable

import pytest
from journey import REPO_ROOT, GateCopy, capture, clean_environment, plain
from repo_checks.expect import contains, failing

TARGET = "x86_64-pc-windows-gnu"
PLANTED_IN = "crates/printobserver-oneharness/src/sign_in.rs"
ARM = (
    "        #[cfg(windows)]\n"
    "        let linked = std::os::windows::fs::symlink_dir(target, link);\n"
)
PLANTED = (
    "        #[cfg(windows)]\n"
    "        let linked = {\n"
    "            let planted_finding = 1;\n"
    "            std::os::windows::fs::symlink_dir(target, link)\n"
    "        };\n"
)


@pytest.mark.skipif(
    host_platform.system() == "Windows",
    reason="the Windows target is this host's own: the native lint tier compiles the arm",
)
def test_lint_reports_a_finding_in_windows_only_code_from_a_unix_host(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The finding the native lint cannot see is the one the recipe fails on."""
    copy = gate_copy()
    copy.edit(PLANTED_IN, ARM, PLANTED)
    environment = clean_environment(
        UV_PROJECT_ENVIRONMENT=str(copy.shared_venv),
        CARGO_TARGET_DIR=str(REPO_ROOT / "target"),
    )

    linted = capture(["just", "lint"], copy.root, timeout=1800, env=environment)

    said = plain(linted.stdout + linted.stderr)
    failing((linted.returncode, said), naming="unused variable: `planted_finding`")
    contains(said, PLANTED_IN, describing="the file the finding is in")
    contains(
        said,
        f"printobserver-oneharness: clippy for {TARGET} reported findings",
        describing="which pass reported it",
    )
    # The native lint over the same copy saw nothing: the arm is dead code on
    # this host, which is the whole reason the second pass exists.
    contains(
        said,
        "Successfully ran target lint",
        describing="the native lint tier, which compiled no `cfg(windows)` arm",
    )
