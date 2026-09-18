"""`just lint` on a Unix host reports a finding in the tree's `cfg(windows)` code.

Three rounds of the Windows gate were lost to a lint or a format finding in code
a Linux host never compiles. This journey plants exactly that finding — a
warning, denied, inside a `#[cfg(windows)]` arm — in a real copy of the tree
and runs the pass the committed `lint` recipe ends in over it, with the real
toolchain: the native clippy over that crate passes, because the arm is dead
code to it, and the pass fails naming the file, the finding and the Windows
target. That the recipe reaches the pass is read off `just`'s own dry run of
it, so the whole native tier is not paid for again to prove one line of a
recipe.

The copy shares this checkout's cargo target directory, so what it pays for is
the workspace's own crates rather than every dependency of the Windows target.
"""

from __future__ import annotations

import json
import platform as host_platform
import shlex
from collections.abc import Callable

import pytest
from journey import REPO_ROOT, GateCopy, capture, clean_environment, output, plain, pythonpath
from repo_checks.expect import contains, failing, passing

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
    """The finding the native lint cannot see is the one the pass fails on."""
    copy = gate_copy()
    copy.edit(PLANTED_IN, ARM, PLANTED)
    environment = clean_environment(
        UV_PROJECT_ENVIRONMENT=str(copy.shared_venv),
        CARGO_TARGET_DIR=str(REPO_ROOT / "target"),
        PYTHONPATH=pythonpath(),
    )
    crate = shlex.split(
        json.loads(copy.read("crates/printobserver-oneharness/project.json"))["targets"]["lint"][
            "command"
        ]
    )

    natively = capture(crate, copy.root, timeout=1800, env=environment)
    passed = capture(
        ["uv", "run", "-q", "python", "-m", "repo_checks", "lint-windows-target"],
        copy.root,
        timeout=1800,
        env=environment,
    )
    recipe = capture(["just", "-n", "lint"], copy.root, timeout=120, env=environment)

    # The arm is dead code on this host: the crate's own lint, run natively over
    # the planted copy, sees nothing — which is the whole reason the pass exists.
    passing((natively.returncode, plain(output(natively))), describing="the native lint")
    said = plain(output(passed))
    failing((passed.returncode, said), naming="unused variable: `planted_finding`")
    contains(said, PLANTED_IN, describing="the file the finding is in")
    contains(
        said,
        f"printobserver-oneharness: clippy for {TARGET} reported findings",
        describing="which pass reported it",
    )
    # And the recipe a developer runs ends in that pass.
    contains(
        plain(output(recipe)).splitlines(),
        "uv run -q python -m repo_checks lint-windows-target",
        describing="`just lint`'s own commands",
    )
