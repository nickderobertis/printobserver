"""The Windows-target lint pass, driven through the command and the committed wrappers.

Every program the pass reaches for — `rustup`, `uv`, `cargo`, zig — stands in
here as a recording program on `PATH`, so each test reads exactly what the pass
asked of it: which target it inserted, where, under which compiler, and what
it did instead where the host is Windows or the target is absent. The pass over
the real toolchain, and a finding it reports that the native lint does not, is
`tests/repo-e2e`'s journey.
"""

from __future__ import annotations

import json
import os
import shlex
import stat
import sys
from pathlib import Path

import pytest
from repo_checks import windows_lint
from repo_checks.expect import absent, contains, equal
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import REPO_ROOT

TARGET = "x86_64-pc-windows-gnu"


def program(directory: Path, name: str, code: str) -> None:
    """Put a program `name` running the Python `code` in `directory`."""
    if sys.platform == "win32":
        (directory / f"{name}.py").write_text(code, encoding="utf-8")
        (directory / f"{name}.cmd").write_text(
            f'@"{sys.executable}" "%~dp0{name}.py" %*\r\n', encoding="utf-8"
        )
        return
    written = directory / name
    written.write_text(f"#!{sys.executable}\n{code}", encoding="utf-8")
    written.chmod(0o755)


def recording(record: Path, *, exit_code: int = 0, stdout: str = "") -> str:
    """A program's code that appends its argv and environment to `record` as JSON."""
    return (
        "import json, os, sys\n"
        f"with open({str(record)!r}, 'a', encoding='utf-8') as out:\n"
        "    out.write(json.dumps({'argv': sys.argv[1:], "
        "'env': {k: v for k, v in os.environ.items() if k.startswith(('CC_', 'AR_', "
        "'PRINTOBSERVER_ZIG'))}}) + '\\n')\n"
        f"sys.stdout.write({stdout!r})\n"
        f"raise SystemExit({exit_code})\n"
    )


def recorded(record: Path) -> list[dict]:
    """Every invocation a recording program wrote."""
    if not record.exists():
        return []
    return [json.loads(line) for line in record.read_text(encoding="utf-8").splitlines()]


@pytest.fixture
def stand_ins(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A `PATH` whose `rustup` carries the target and whose `uv` and `cargo` record."""
    programs = tmp_path / "bin"
    programs.mkdir()
    program(
        programs,
        "rustup",
        recording(tmp_path / "rustup", stdout=f"{TARGET}\nx86_64-unknown-linux-gnu\n"),
    )
    program(programs, "uv", recording(tmp_path / "uv", stdout=f"{tmp_path / 'zig'}\n"))
    program(programs, "cargo", recording(tmp_path / "cargo"))
    monkeypatch.setenv("PATH", f"{programs}{os.pathsep}{os.environ['PATH']}")
    return tmp_path


def test_each_crates_own_lint_is_run_again_for_the_windows_target(
    stand_ins: Path, committed: Repo
) -> None:
    """Every crate's committed clippy command, with the target inserted before its `--`."""
    equal(windows_lint.lint_windows_target(committed, host="Linux"), 0)

    invocations = recorded(stand_ins / "cargo")
    equal(len(invocations), len(committed.crate_names), describing="one clippy run per crate")
    for invocation in invocations:
        argv = invocation["argv"]
        contains(argv, "--target", describing=f"the target flag in {argv}")
        equal(argv[argv.index("--target") + 1], TARGET, describing="the target linted for")
        equal(
            argv.index("--target") < argv.index("--"),
            True,
            describing=f"the target to be cargo's argument rather than clippy's: {argv}",
        )
        equal(argv[-2:], ["-D", "warnings"], describing="the committed lint's own severity")
        equal(
            invocation["env"]["CC_x86_64_pc_windows_gnu"],
            str(REPO_ROOT / "scripts" / "zig-cc.sh"),
            describing="the C compiler cargo's build scripts are handed",
        )
        equal(
            invocation["env"]["AR_x86_64_pc_windows_gnu"],
            str(REPO_ROOT / "scripts" / "zig-ar.sh"),
            describing="the archiver cargo's build scripts are handed",
        )
        equal(invocation["env"]["PRINTOBSERVER_ZIG"], str(stand_ins / "zig"))
        equal(invocation["env"]["PRINTOBSERVER_ZIG_TARGET"], "x86_64-windows-gnu")
    # Each retargeted command is the committed one and nothing else: the crate's
    # own features, its own `--all-targets`, its own `--locked`.
    for project in committed.project_paths:
        if project.parent.parent != committed.path("crates"):
            continue
        lint = json.loads(project.read_text(encoding="utf-8"))["targets"]["lint"]["command"]
        own = shlex.split(lint)[1:]
        retargeted = [
            [a for a in inv["argv"] if a not in ("--target", TARGET)]
            for inv in invocations
            if project.parent.name in inv["argv"]
        ]
        contains(retargeted, own, describing=f"{project.parent.name}'s own lint, retargeted")


def test_a_finding_for_the_windows_target_fails_the_pass_naming_the_crate(
    stand_ins: Path, committed: Repo, capsys: pytest.CaptureFixture[str]
) -> None:
    """One crate's clippy exits non-zero: the pass fails, and its output is the finding."""
    programs = stand_ins / "bin"
    program(
        programs,
        "cargo",
        "import sys\n"
        "if 'printobserver-oneharness' in sys.argv:\n"
        "    print('error: unused variable: `unused`', file=sys.stderr)\n"
        "    raise SystemExit(101)\n",
    )

    equal(windows_lint.lint_windows_target(committed, host="Linux"), 1)

    error = capsys.readouterr().err
    contains(error, "error: unused variable: `unused`", describing="clippy's own finding")
    contains(error, f"printobserver-oneharness: clippy for {TARGET} reported findings")


def test_a_windows_host_runs_nothing_because_its_native_lint_is_the_pass(
    stand_ins: Path, committed: Repo, capsys: pytest.CaptureFixture[str]
) -> None:
    """On Windows the tree's `cfg(windows)` code is what the lint tier compiled."""
    equal(windows_lint.lint_windows_target(committed, host="Windows"), 0)

    equal(recorded(stand_ins / "cargo"), [], describing="no clippy run on a Windows host")
    contains(capsys.readouterr().out, "the native lint tier is the Windows-target pass")


def test_a_host_without_the_target_says_so_and_names_what_installs_it(
    stand_ins: Path, committed: Repo, capsys: pytest.CaptureFixture[str]
) -> None:
    """Skipped rather than failed, and never silently: the missing target is named."""
    program(stand_ins / "bin", "rustup", "print('x86_64-unknown-linux-gnu')\n")

    equal(windows_lint.lint_windows_target(committed, host="Linux"), 0)

    equal(recorded(stand_ins / "cargo"), [], describing="no clippy run without the target")
    error = capsys.readouterr().err
    contains(error, f"{TARGET}: standard library not installed")
    contains(error, f"rustup target add {TARGET}")


def test_bootstrap_adds_the_target_on_a_host_that_is_not_windows(
    stand_ins: Path, committed: Repo
) -> None:
    """Present, nothing is added; absent, it is added; on Windows, never."""
    equal(windows_lint.install_target(committed, host="Linux"), 0)
    equal(
        [inv["argv"] for inv in recorded(stand_ins / "rustup")],
        [["target", "list", "--installed"]],
        describing="a target already installed is not added again",
    )

    program(
        stand_ins / "bin",
        "rustup",
        recording(stand_ins / "rustup", stdout="x86_64-unknown-linux-gnu\n"),
    )
    equal(windows_lint.install_target(committed, host="Linux"), 0)
    contains(
        [inv["argv"] for inv in recorded(stand_ins / "rustup")],
        ["target", "add", TARGET],
        describing="the absent target added",
    )

    (stand_ins / "rustup").unlink()
    equal(windows_lint.install_target(committed, host="Windows"), 0)
    equal(recorded(stand_ins / "rustup"), [], describing="rustup never asked on Windows")


@pytest.mark.skipif(sys.platform == "win32", reason="the wrappers are for a Unix host's cargo")
def test_the_compiler_wrapper_drops_the_rust_triple_and_names_zigs_own_target(
    tmp_path: Path,
) -> None:
    """cc-rs hands a clang-like compiler `--target=<rust triple>`; zig gets its own instead."""
    zig = tmp_path / "zig"
    zig.write_text(
        f"#!{sys.executable}\nimport sys\nprint(' '.join(sys.argv[1:]))\n", encoding="utf-8"
    )
    zig.chmod(zig.stat().st_mode | stat.S_IXUSR)
    environment = dict(os.environ)
    environment["PRINTOBSERVER_ZIG"] = str(zig)
    environment["PRINTOBSERVER_ZIG_TARGET"] = "x86_64-windows-gnu"

    compiled = run(
        [
            str(REPO_ROOT / "scripts" / "zig-cc.sh"),
            "-O0",
            f"--target={TARGET}",
            "-o",
            "out.o",
            "-c",
            "in.c",
        ],
        env=environment,
    )
    archived = run(
        [str(REPO_ROOT / "scripts" / "zig-ar.sh"), "cq", "lib.a", "out.o"], env=environment
    )

    equal(compiled.returncode, 0)
    equal(
        shlex.split(compiled.stdout),
        ["cc", "-target", "x86_64-windows-gnu", "-O0", "-o", "out.o", "-c", "in.c"],
        describing="what zig was asked to compile",
    )
    absent(compiled.stdout, f"--target={TARGET}", describing="the Rust triple, dropped")
    equal(
        shlex.split(archived.stdout), ["ar", "cq", "lib.a", "out.o"], describing="what zig archived"
    )
