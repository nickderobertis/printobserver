"""The commands the recipes and hooks run that do something rather than check it."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import os
import shutil
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.commands import coverage, install_hooks, install_tools
from repo_checks.expect import absent, contains, equal
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

#: The release the committed toolchain holds release-plz at, read rather than
#: restated so these journeys follow a bump.
HELD = next(
    str(tool["version"])
    for tool in Repo(REPO_ROOT).policy["toolchain"]["tool"]
    if tool["command"] == "release-plz"
)

#: A release no toolchain holds anything at: the cached copy from before a bump.
STALE = "0.0.0-stale"

# A stand-in for `cargo install`, which reaches crates.io and compiles for
# minutes: it writes the program the install would have put in
# CARGO_STANDIN_INTO, answering `--version` with the release it was asked for,
# — or with CARGO_STANDIN_ANSWERS where that is set, as a build naming no release
# answers — and records every invocation to CARGO_STANDIN_RECORD. Everything else
# the install path does — reading the committed declaration, asking what is on
# PATH, deciding — is the real command's.
CARGO_STANDIN = """
import os
import pathlib
import sys

arguments = sys.argv[1:]
with open(os.environ["CARGO_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    record.write(" ".join(arguments) + "\\n")
release = arguments[arguments.index("--version") + 1] if "--version" in arguments else "0.0.1"
release = os.environ.get("CARGO_STANDIN_ANSWERS", release)
into = pathlib.Path(os.environ["CARGO_STANDIN_INTO"])
answer = f"print({arguments[1] + ' ' + release!r})\\n"
if sys.platform == "win32":
    (into / f"{arguments[1]}.py").write_text(answer, encoding="utf-8")
    (into / f"{arguments[1]}.cmd").write_text(
        f'@"{sys.executable}" "%~dp0{arguments[1]}.py" %*\\r\\n', encoding="utf-8"
    )
else:
    program = into / arguments[1]
    program.write_text(f"#!{sys.executable}\\n{answer}", encoding="utf-8")
    program.chmod(0o755)
"""

POLICY = """
schema_version = 1

[gate.coverage]
rust = 95
python = 95
typescript = 95

[[toolchain.tool]]
command = "{command}"
install = "{install}"
"""


def test_install_hooks_points_git_at_the_committed_hooks(
    tree: Callable[[], Tree],
) -> None:
    """A clean clone's bootstrap wires the hooks it ships with."""
    fresh = tree()
    run(["git", "init", "-q", "-b", "main"], cwd=fresh.root, check=True)

    equal(install_hooks(fresh.repo), 0)

    configured = run(["git", "config", "core.hooksPath"], cwd=fresh.root, check=True).stdout.strip()
    equal(configured, ".githooks")


def test_install_hooks_is_a_no_op_outside_a_git_repository(
    tree: Callable[[], Tree],
) -> None:
    """The bootstrap journey copies the tree without its history and must still run."""
    fresh = tree()

    equal(install_hooks(fresh.repo), 0)


def test_install_tools_skips_a_tool_already_on_the_path(tmp_path: Path) -> None:
    """Bootstrap is idempotent: a present tool is not reinstalled."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="git", install="false"), encoding="utf-8"
    )

    equal(install_tools(Repo(root)), 0)


def test_install_tools_reports_an_install_it_could_not_do(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A failed install names the command to run by hand rather than failing silently."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="a-tool-that-does-not-exist", install="false"),
        encoding="utf-8",
    )

    equal(install_tools(Repo(root)), 1)
    contains(capsys.readouterr().err, "Run `false` by hand")


def program(directory: Path, name: str, code: str) -> None:
    """Put a program `name` running the Python `code` in `directory`.

    A POSIX host runs it by its interpreter line. A Windows host finds a program
    by its suffix and runs no interpreter line, so there the code sits beside a
    `.cmd` that hands it to this interpreter.
    """
    if sys.platform == "win32":
        (directory / f"{name}.py").write_text(code, encoding="utf-8")
        (directory / f"{name}.cmd").write_text(
            f'@"{sys.executable}" "%~dp0{name}.py" %*\r\n', encoding="utf-8"
        )
        return
    written = directory / name
    written.write_text(f"#!{sys.executable}\n{code}", encoding="utf-8")
    written.chmod(0o755)


def answering(release: str) -> str:
    """A program's code that answers `--version` the way release-plz does."""
    return f"print({f'release-plz {release}'!r})\n"


def failing_with(status: int) -> str:
    """A program's code that exits with `status` and says nothing."""
    return f"raise SystemExit({status})\n"


def toolchain(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, shadow: str | None = None
) -> tuple[Path, Path, Path]:
    """A tree carrying the committed toolchain declaration, and a PATH of stand-ins alone.

    The PATH holds the stand-in `cargo` and the directory it installs into — the
    two tools the policy holds at no release already there, and a `rustup`
    answering that the Windows lint target's standard library is too, so what
    is decided is release-plz's — and, with `shadow`, a directory ahead of both
    holding a release-plz answering that release. Returns the tree, the
    directory installs land in, and the record of what `cargo` was asked.
    """
    root = tmp_path / "tree"
    root.mkdir()
    shutil.copy2(REPO_ROOT / "repo-policy.toml", root / "repo-policy.toml")
    installs = tmp_path / "cargo-bin"
    installs.mkdir()
    program(installs, "cargo", CARGO_STANDIN)
    for present in ("cargo-nextest", "cargo-llvm-cov"):
        program(installs, present, f"print({f'{present} 0.0.1'!r})\n")
    windows_lint = Repo(REPO_ROOT).policy["toolchain"]["windows_lint"]["target"]
    program(installs, "rustup", f"print({windows_lint!r})\n")
    record = tmp_path / "cargo-invocations"
    record.touch()
    directories = [installs]
    if shadow is not None:
        shadowing = tmp_path / "shadow"
        shadowing.mkdir()
        program(shadowing, "release-plz", answering(shadow))
        directories.insert(0, shadowing)
    monkeypatch.setenv("PATH", os.pathsep.join(str(directory) for directory in directories))
    monkeypatch.setenv("CARGO_STANDIN_INTO", str(installs))
    monkeypatch.setenv("CARGO_STANDIN_RECORD", str(record))
    return root, installs, record


@pytest.mark.parametrize(
    ("stale", "said"),
    [
        (answering(STALE), f"answers {STALE}, not the held {HELD}"),
        (failing_with(1), f"answers no release, not the held {HELD}"),
    ],
)
def test_install_tools_replaces_a_held_tool_on_the_path_at_another_release(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    stale: str,
    said: str,
) -> None:
    """A cached release-plz from before the pin is reinstalled at the held release."""
    root, installs, record = toolchain(tmp_path, monkeypatch)
    program(installs, "release-plz", stale)

    equal(main(["install-tools", "--root", str(root)]), 0)

    contains(capsys.readouterr().err, said)
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"install release-plz --locked --version {HELD}"],
        describing="what `cargo` was asked to install",
    )
    contains(run(["release-plz", "--version"], check=True).stdout, f"release-plz {HELD}")


def test_install_tools_installs_a_held_tool_absent_from_the_path_at_its_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A host with no release-plz at all gets the held release, not the newest."""
    root, _, record = toolchain(tmp_path, monkeypatch)

    equal(main(["install-tools", "--root", str(root)]), 0)

    contains(capsys.readouterr().err, "installing release-plz")
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"install release-plz --locked --version {HELD}"],
        describing="what `cargo` was asked to install",
    )
    contains(run(["release-plz", "--version"], check=True).stdout, f"release-plz {HELD}")


def test_install_tools_reports_a_replacement_it_could_not_install(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A stale copy the install could not replace fails naming the command to run by hand."""
    root, installs, _ = toolchain(tmp_path, monkeypatch)
    program(installs, "release-plz", answering(STALE))
    program(installs, "cargo", failing_with(101))

    equal(main(["install-tools", "--root", str(root)]), 1)

    contains(
        capsys.readouterr().err,
        f"failed to install release-plz. Run `cargo install release-plz --locked --version {HELD}`",
    )
    contains(run(["release-plz", "--version"], check=True).stdout, f"release-plz {STALE}")


def test_install_tools_refuses_an_install_whose_program_names_no_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A program that cannot say it is the held release is not accepted as it."""
    root, _, _ = toolchain(tmp_path, monkeypatch)
    monkeypatch.setenv("CARGO_STANDIN_ANSWERS", "(built from an unknown revision)")

    equal(main(["install-tools", "--root", str(root)]), 1)

    contains(capsys.readouterr().err, f"still answers no release after installing {HELD}")


@pytest.mark.parametrize(
    ("version", "install", "said"),
    [
        ("latest", "false {version}", "holds `release-plz` at 'latest', which is not a release"),
        ("0.3.167\\nreleased=v9", "false {version}", "which is not a release"),
        ("0.3.167", "false", "never substitutes `{version}`"),
    ],
)
def test_a_held_release_the_installer_cannot_act_on_is_refused_before_anything_is_installed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], version: str, install: str, said: str
) -> None:
    """Neither the installer nor a workflow's output is handed a release it cannot act on."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        f'[[toolchain.tool]]\ncommand = "release-plz"\nversion = "{version}"\n'
        f'install = "{install}"\n',
        encoding="utf-8",
    )

    equal(install_tools(Repo(root)), 1)
    installing = capsys.readouterr()
    contains(installing.err, said)
    absent(installing.err, "failed to install")
    equal(main(["tool-version", "release-plz", "--root", str(root)]), 1)
    answering_version = capsys.readouterr()
    equal(answering_version.out, "", describing="what the workflow's output would be handed")
    contains(answering_version.err, said)


def test_install_tools_refuses_a_held_tool_a_copy_earlier_on_the_path_shadows(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """An install the PATH does not reach is refused, naming the copy it reaches instead."""
    root, _, record = toolchain(tmp_path, monkeypatch, shadow=STALE)

    equal(main(["install-tools", "--root", str(root)]), 1)

    said = capsys.readouterr().err
    contains(said, f"{shutil.which('release-plz')} still answers {STALE}")
    contains(said, "a copy earlier on PATH shadows the one installed")
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"install release-plz --locked --version {HELD}"],
        describing="what `cargo` was asked to install",
    )


def test_install_tools_accepts_a_held_tool_on_the_path_at_its_release_without_reinstalling(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The held release already on PATH is left as it is, and nothing is installed."""
    root, installs, record = toolchain(tmp_path, monkeypatch)
    program(installs, "release-plz", answering(HELD))

    equal(main(["install-tools", "--root", str(root)]), 0)

    equal(record.read_text(encoding="utf-8"), "", describing="what `cargo` was asked")
    absent(capsys.readouterr().err, "installing")


#: The release the committed toolchain holds gh at, which it does not bootstrap.
GH_HELD = next(
    str(tool["version"])
    for tool in Repo(REPO_ROOT).policy["toolchain"]["tool"]
    if tool["command"] == "gh"
)

# A stand-in for `uv`, recording what it was asked and putting the `gh` the
# committed install would have put on PATH, answering the release it was asked
# for.
UV_STANDIN = """
import os
import pathlib
import sys

arguments = sys.argv[1:]
with open(os.environ["UV_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    record.write(" ".join(arguments) + "\\n")
into = pathlib.Path(os.environ["CARGO_STANDIN_INTO"])
answer = f"print({'gh version ' + arguments[-1] + ' (a stand-in)'!r})\\n"
if sys.platform == "win32":
    (into / "gh.py").write_text(answer, encoding="utf-8")
    (into / "gh.cmd").write_text(f'@"{sys.executable}" "%~dp0gh.py" %*\\r\\n', encoding="utf-8")
else:
    program = into / "gh"
    program.write_text(f"#!{sys.executable}\\n{answer}", encoding="utf-8")
    program.chmod(0o755)
"""


def test_install_tools_leaves_a_tool_declared_not_to_bootstrap_alone(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """Bootstrap installs every host's tools, and not one job's: gh is left off PATH."""
    root, installs, record = toolchain(tmp_path, monkeypatch)
    program(installs, "release-plz", answering(HELD))
    uv_record = tmp_path / "uv-invocations"
    uv_record.touch()
    program(installs, "uv", UV_STANDIN)
    monkeypatch.setenv("UV_STANDIN_RECORD", str(uv_record))

    equal(main(["install-tools", "--root", str(root)]), 0)

    absent(capsys.readouterr().err, "gh")
    equal(uv_record.read_text(encoding="utf-8"), "", describing="what `uv` was asked")
    equal(record.read_text(encoding="utf-8"), "", describing="what `cargo` was asked")


def test_install_tools_installs_a_tool_it_is_named_whatever_bootstrap_says(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The job that needs gh names it, and gets the held release through its install."""
    root, installs, record = toolchain(tmp_path, monkeypatch)
    uv_record = tmp_path / "uv-invocations"
    uv_record.touch()
    program(installs, "uv", UV_STANDIN)
    monkeypatch.setenv("UV_STANDIN_RECORD", str(uv_record))

    equal(main(["install-tools", "gh", "--root", str(root)]), 0)

    contains(capsys.readouterr().err, "installing gh")
    equal(
        uv_record.read_text(encoding="utf-8").splitlines(),
        [f"run -q python -m repo_checks install-gh {GH_HELD}"],
        describing="what `uv` was asked to install",
    )
    equal(record.read_text(encoding="utf-8"), "", describing="what `cargo` was asked")
    contains(run(["gh", "--version"], check=True).stdout, f"gh version {GH_HELD}")


def test_install_tools_refuses_a_tool_the_toolchain_does_not_declare(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """Naming a tool nobody declared installs nothing and says what is declared."""
    root, _, record = toolchain(tmp_path, monkeypatch)

    equal(main(["install-tools", "hub", "--root", str(root)]), 1)

    contains(capsys.readouterr().err, "declares no toolchain tool `hub`")
    equal(record.read_text(encoding="utf-8"), "", describing="what `cargo` was asked")


def test_a_bootstrap_that_is_not_a_boolean_is_refused_before_anything_is_installed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """`bootstrap = "no"` is not `false`, and a guess at which was meant is not made."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="gh", install="false") + 'bootstrap = "no"\n', encoding="utf-8"
    )

    equal(install_tools(Repo(root)), 1)
    contains(capsys.readouterr().err, "`bootstrap` for `gh` is 'no'")


def test_tool_version_answers_the_release_the_toolchain_holds(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The one line a release job reads the held release-plz off."""
    equal(main(["tool-version", "release-plz", "--root", str(REPO_ROOT)]), 0)

    equal(capsys.readouterr().out, f"version={HELD}\n")


@pytest.mark.parametrize(
    ("tool", "said"),
    [
        ("cargo-nextest", "holds `cargo-nextest` at no release"),
        ("release-please", "declares no toolchain tool `release-please`"),
    ],
)
def test_tool_version_refuses_a_tool_held_at_no_release(
    capsys: pytest.CaptureFixture[str], tool: str, said: str
) -> None:
    """An empty answer would install the newest release, so none is given."""
    equal(main(["tool-version", tool, "--root", str(REPO_ROOT)]), 1)

    captured = capsys.readouterr()
    equal(captured.out, "")
    contains(captured.err, said)


def test_tool_version_needs_a_tool() -> None:
    """Asked for no tool, the command says so rather than answering for one."""
    with pytest.raises(SystemExit):
        main(["tool-version", "--root", str(REPO_ROOT)])


def test_coverage_fails_where_no_coverage_was_measured(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A run with nothing to report is below the floor, not above it."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="git", install="false"), encoding="utf-8"
    )

    equal(coverage(Repo(root)), 1)
    error = capsys.readouterr().err
    contains(error, "could not find `Cargo.toml`")
    contains(error, "below the")


EXEMPTION = """
[gate.coverage.exemptions.{platform}]
target = "{target}"
toolchain = "rustc 1.97.1 and its bundled llvm-profdata"
diagnostics = {diagnostics}
{reference}
"""

#: What the ARM toolchain's own profile reader prints, as the policy lists it.
DIAGNOSTICS = (
    '["malformed instrumentation profile data: symbol name is empty", "no profile can be merged"]'
)

#: What the ARM toolchain's own profile reader prints before it exits non-zero.
UNREADABLE = (
    "import sys\n"
    'print("malformed instrumentation profile data: symbol name is empty", file=sys.stderr)\n'
    'print("no profile can be merged", file=sys.stderr)\n'
    "raise SystemExit(1)\n"
)


def exempting(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    platform: str = "windows-aarch64",
    target: str = "aarch64-pc-windows-msvc",
    diagnostics: str = DIAGNOSTICS,
    reference: str = 'reference = "https://github.com/rust-lang/rust/issues/150123"',
    cargo: str = UNREADABLE,
) -> Repo:
    """A tree carrying one coverage exemption, on a Windows ARM gate whose reader refuses.

    The supported-platform list is the one `AGENTS.md` block the exemption is
    read against — it is where a platform's Rust target comes from.
    """
    root = tmp_path / "tree"
    root.mkdir()
    policy = POLICY.format(command="git", install="false") + EXEMPTION.format(
        platform=platform, target=target, diagnostics=diagnostics, reference=reference
    )
    (root / "repo-policy.toml").write_text(policy, encoding="utf-8")
    (root / "AGENTS.md").write_text(
        "[//]: # (BEGIN supported-platforms)\n"
        "- `windows-aarch64` — runner `windows-11-arm`, Rust target "
        "`aarch64-pc-windows-msvc`, service manager `windows-service`, install path: no — owed\n"
        "[//]: # (END supported-platforms)\n",
        encoding="utf-8",
    )
    programs = tmp_path / "bin"
    programs.mkdir()
    program(programs, "cargo", cargo)
    program(programs, "uv", "raise SystemExit(0)\n")
    monkeypatch.setenv("PATH", f"{programs}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setenv("PRINTOBSERVER_PLATFORM", platform)
    return Repo(root)


def test_coverage_reports_the_native_windows_arm_toolchain_exemption(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The ARM gate names its unreadable native profile instead of a false floor miss."""
    repo = exempting(tmp_path, monkeypatch)

    equal(coverage(repo), 0)

    out = capsys.readouterr().out
    contains(
        out,
        "no readable profile on aarch64-pc-windows-msvc, exempt by policy: rustc 1.97.1 and "
        "its bundled llvm-profdata; https://github.com/rust-lang/rust/issues/150123",
    )
    contains(out, "coverage: rust lines no readable profile, exempt (floor 95%)")


def test_a_coverage_exemption_naming_no_reference_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A toolchain refusal nobody has reported upstream is a floor miss, and says why."""
    repo = exempting(tmp_path, monkeypatch, reference="")

    equal(coverage(repo), 1)

    error = capsys.readouterr().err
    contains(error, "the coverage exemption for `windows-aarch64` states no `reference`")
    contains(error, "below the 95% floor")


def test_a_coverage_exemption_naming_another_target_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The exemption is held to the platform's own Rust target, read off the list."""
    repo = exempting(tmp_path, monkeypatch, target="x86_64-pc-windows-msvc")

    equal(coverage(repo), 1)

    error = capsys.readouterr().err
    contains(
        error,
        "the coverage exemption for `windows-aarch64` names target `x86_64-pc-windows-msvc`, "
        "and that platform's Rust target is `aarch64-pc-windows-msvc`",
    )


def test_a_coverage_exemption_whose_reference_is_not_an_upstream_issue_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A reference that is not a GitHub issue is a note, and a note exempts nothing."""
    repo = exempting(tmp_path, monkeypatch, reference='reference = "see the wiki"')

    equal(coverage(repo), 1)

    error = capsys.readouterr().err
    contains(error, "names `see the wiki` as its reference")
    contains(error, "held to an upstream issue on GitHub")


def test_a_coverage_exemption_for_a_platform_the_list_does_not_name_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The gate runs as a listed platform; an entry for any other has no target to hold it to."""
    repo = exempting(tmp_path, monkeypatch, platform="windows-riscv64")

    equal(coverage(repo), 1)

    error = capsys.readouterr().err
    contains(
        error,
        "the coverage exemption for `windows-riscv64` names a platform AGENTS.md's "
        "supported-platform list does not",
    )


def test_a_coverage_exemption_whose_diagnostics_are_not_a_list_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """One string iterates per character, each trivially printed; it is refused, not matched."""
    repo = exempting(tmp_path, monkeypatch, diagnostics='"no profile can be merged"')

    equal(coverage(repo), 1)

    contains(
        capsys.readouterr().err,
        "states `diagnostics` as 'no profile can be merged' rather than a list of strings",
    )


def test_a_coverage_exemption_does_not_cover_a_readable_profile_below_the_floor(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A floor genuinely missed on the exempt platform is still a floor missed."""
    repo = exempting(
        tmp_path,
        monkeypatch,
        cargo='print("TOTAL  13221  1087  91.78%  1455  144  90.10%  9625  517  90.00%  0  0  -")\n'
        "raise SystemExit(1)\n",
    )

    equal(coverage(repo), 1)

    error = capsys.readouterr().err
    contains(error, "did not apply")
    contains(error, "so the floor was missed rather than unreadable")


def test_coverage_states_each_total_beside_its_floor_on_a_pass(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A passing run still leaves each platform's measured figure in its log."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="git", install="false"), encoding="utf-8"
    )
    programs = tmp_path / "bin"
    programs.mkdir()
    program(
        programs,
        "cargo",
        'print("Filename  Regions  Missed Regions  Cover  Functions  Missed Functions  '
        'Executed  Lines  Missed Lines  Cover  Branches  Missed Branches  Cover")\n'
        'print("TOTAL  13221  1087  91.78%  1455  144  90.10%  9625  517  96.63%  0  0  -")\n',
    )
    program(
        programs,
        "uv",
        "import sys\n"
        'if "report" in sys.argv:\n'
        '    print("Name  Stmts  Miss  Branch  BrPart  Cover")\n'
        '    print("TOTAL  8386  435  2828  248  97%")\n',
    )
    monkeypatch.setenv("PATH", f"{programs}{os.pathsep}{os.environ['PATH']}")

    equal(coverage(Repo(root)), 0)

    out = capsys.readouterr().out
    contains(
        out, "TOTAL  13221  1087  91.78%", describing="the Rust per-file table, printed on a pass"
    )
    contains(
        out, "TOTAL  8386  435  2828", describing="the Python per-file table, printed on a pass"
    )
    equal(
        out.strip().splitlines()[-1],
        "coverage: rust lines 96.63% (floor 95%), python lines 97% (floor 95%)",
    )
