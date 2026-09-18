"""The Windows-target lint pass: every crate's own clippy, retargeted, from a Unix host.

Three rounds of the Windows gate were lost to a lint or a format finding in code
a Linux host never compiles — `cfg(windows)` code, which the native lint tier on
a Unix host skips over as dead. This pass runs each crate's committed `lint`
target again for the Windows target `repo-policy.toml` names, so the finding is
reported by `just lint` here, before a push, rather than by a Windows runner at
the end of a two-hour round.

Clippy for another target still runs every dependency's build script, and two of
this workspace's compile C for the target — which needs a cross C compiler no
Unix host carries by default. zig is that compiler: obtained through `uv` from
the `zig` dependency group, it bundles the mingw-w64 headers and libraries the
GNU Windows target links against, so the pass installs nothing on the host but
the target's own standard library, which `just bootstrap` adds.
"""

from __future__ import annotations

import json
import os
import platform as host_platform
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import PolicyValueError, Repo, policy_strings, policy_table
from repo_checks.shell import run

#: The environment the two wrappers beside this module read.
ZIG_ENV = "PRINTOBSERVER_ZIG"
ZIG_TARGET_ENV = "PRINTOBSERVER_ZIG_TARGET"

#: zig as the C compiler and the archiver cc-rs is handed for the target.
COMPILER = Path(__file__).with_name("zig-cc.sh")
ARCHIVER = Path(__file__).with_name("zig-ar.sh")


@dataclass(frozen=True, slots=True)
class WindowsLint:
    """`toolchain.windows_lint`: the Rust target the pass lints for, and zig's name for it."""

    target: str
    zig_target: str


@dataclass(frozen=True, slots=True)
class CrateLint:
    """One crate's committed `lint` target: its name and the clippy command it runs."""

    crate: str
    argv: list[str]

    def retargeted(self, target: str) -> list[str]:
        """The same command for `target`: cargo's flag, inserted before clippy's own."""
        split = self.argv.index("--") if "--" in self.argv else len(self.argv)
        return [*self.argv[:split], "--target", target, *self.argv[split:]]


def settings(repo: Repo) -> WindowsLint | None:
    """The pass's target, or `None` for a tree whose policy declares none.

    Raises:
        PolicyValueError: If the table is there and either field is not a
            non-empty string.
    """
    table = policy_table(repo, "toolchain").get("windows_lint")
    if not table:
        return None
    named = policy_strings(table, ("target", "zig_target"), "toolchain.windows_lint")
    return WindowsLint(named["target"], named["zig_target"])


def _target_installed(target: str) -> bool:
    """Whether this host's toolchain carries `target`'s standard library."""
    installed = run(["rustup", "target", "list", "--installed"])
    return target in installed.stdout.split()


def install_target(repo: Repo, *, host: str | None = None) -> int:
    """Add the Windows target's standard library on a host that is not Windows.

    Part of `just bootstrap`, so that `just lint` covers the tree's Windows
    code on every developer's host rather than only on the runners.
    """
    try:
        declared = settings(repo)
    except PolicyValueError as malformed:
        print(f"{malformed}. Correct it; no target was added.", file=sys.stderr)
        return 1
    if declared is None or (host or host_platform.system()) == "Windows":
        return 0
    if _target_installed(declared.target):
        return 0
    print(f"adding the {declared.target} standard library, for `just lint`'s Windows-target pass")
    return run(["rustup", "target", "add", declared.target]).returncode


def _zig(repo: Repo) -> str:
    """The zig program the `zig` dependency group carries, resolved once."""
    located = run(
        [
            "uv",
            "run",
            "-q",
            "--group",
            "zig",
            "python",
            "-c",
            "import pathlib, ziglang; print(pathlib.Path(ziglang.__file__).parent / 'zig')",
        ],
        cwd=repo.root,
        check=True,
    )
    return located.stdout.strip()


def crate_lints(repo: Repo) -> list[CrateLint]:
    """Each crate's committed clippy command."""
    lints: list[CrateLint] = []
    for project in repo.project_paths:
        if project.parent.parent != repo.path("crates"):
            continue
        targets = json.loads(project.read_text(encoding="utf-8")).get("targets", {})
        argv = shlex.split(str(targets.get("lint", {}).get("command", "")))
        if argv[:2] == ["cargo", "clippy"]:
            lints.append(CrateLint(project.parent.name, argv))
    return lints


def lint_windows_target(repo: Repo, *, host: str | None = None) -> int:
    """Run every crate's own `lint` target for the Windows target.

    On a Windows host the native lint tier is this pass, so nothing runs. On a
    Unix host without the target's standard library the pass says so and skips,
    naming what installs it. Otherwise each crate's committed clippy command is
    run once more with `--target` inserted before its `--`, so what is linted
    for Windows is exactly what is linted natively, and every crate that
    reports a finding fails the pass with that finding on its own output.
    """
    try:
        declared = settings(repo)
    except PolicyValueError as malformed:
        print(f"{malformed}. Correct it; nothing was linted for Windows.", file=sys.stderr)
        return 1
    if declared is None:
        print("no `toolchain.windows_lint` target declared; nothing linted for Windows")
        return 0
    target = declared.target
    if (host or host_platform.system()) == "Windows":
        print(f"{target}: this host's own; the native lint tier is the Windows-target pass")
        return 0
    if not _target_installed(target):
        print(
            f"{target}: standard library not installed, so the tree's `cfg(windows)` code "
            f"was not linted here. `rustup target add {target}` (what `just bootstrap` runs).",
            file=sys.stderr,
        )
        return 0
    environment = dict(os.environ)
    environment[ZIG_ENV] = _zig(repo)
    environment[ZIG_TARGET_ENV] = declared.zig_target
    suffix = target.replace("-", "_")
    environment[f"CC_{suffix}"] = str(COMPILER)
    environment[f"AR_{suffix}"] = str(ARCHIVER)
    lints = crate_lints(repo)
    failed = False
    for lint in lints:
        result = run(lint.retargeted(target), cwd=repo.root, env=environment)
        if result.returncode != 0:
            print(result.stderr, file=sys.stderr, end="")
            print(f"{lint.crate}: clippy for {target} reported findings", file=sys.stderr)
            failed = True
    print(f"{target}: linted {len(lints)} crates for the Windows target")
    return 1 if failed else 0
