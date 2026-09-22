"""The commands the recipes and hooks run that do something rather than check it."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import os
import platform
import shutil
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.commands import coverage, install_hooks, install_tools
from repo_checks.expect import absent, contains, equal
from repo_checks.model import Repo
from repo_checks.powershell_release import HASHES
from repo_checks.powershell_release import archive_for as powershell_archive_for
from repo_checks.shell import run
from standin_powershell import publish as publish_powershell
from standin_release import Release, serving
from treecopy import REPO_ROOT, Tree


def _held(command: str) -> str:
    """The release the committed toolchain holds one tool at, read rather than restated."""
    return next(
        str(tool["version"])
        for tool in Repo(REPO_ROOT).policy["toolchain"]["tool"]
        if tool["command"] == command
    )


#: The releases the committed toolchain holds, read rather than restated so
#: these journeys follow a bump.
HELD = _held("release-plz")
PWSH_HELD = _held("pwsh")

#: A release no toolchain holds anything at: the cached copy from before a bump.
STALE = "0.0.0-stale"

# A stand-in for `uv`, which is how every tool the toolchain holds at a release
# is installed: the committed install runs `uv run -q python -m repo_checks
# install-<something> <release>`, which downloads a real release over the
# network. This writes the program that verb installs into INSTALL_STANDIN_INTO,
# answering `--version` with the release it was asked for — or with
# INSTALL_STANDIN_ANSWERS where that is set, as a build naming no release
# answers — and records every invocation to INSTALL_STANDIN_RECORD. Everything
# else the install path does — reading the committed declaration, asking what is
# on PATH, deciding — is the real command's.
INSTALL_STANDIN = """
import os
import pathlib
import sys

PROGRAMS = {
    "install-gh": "gh",
    "install-release-plz": "release-plz",
    "install-powershell": "pwsh",
}
arguments = sys.argv[1:]
with open(os.environ["INSTALL_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    record.write(" ".join(arguments) + "\\n")
name = PROGRAMS[arguments[arguments.index("-m") + 2]]
release = os.environ.get("INSTALL_STANDIN_ANSWERS", arguments[-1])
into = pathlib.Path(os.environ["INSTALL_STANDIN_INTO"])
answer = f"print({name + ' version ' + release!r})\\n"
if sys.platform == "win32":
    (into / f"{name}.py").write_text(answer, encoding="utf-8")
    (into / f"{name}.cmd").write_text(
        f'@"{sys.executable}" "%~dp0{name}.py" %*\\r\\n', encoding="utf-8"
    )
else:
    program = into / name
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
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    shadow: str | None = None,
    provides: str | None = "pwsh",
    provided_release: str = PWSH_HELD,
    uv: str = INSTALL_STANDIN,
) -> tuple[Path, Path, Path]:
    """A tree carrying the committed toolchain declaration, and a PATH of stand-ins alone.

    The PATH holds the stand-in `uv` and the directory it installs into — the
    two cargo tools the policy holds at no release already there, a `rustup`
    answering that the Windows lint target's standard library is there too, and
    the PowerShell `provides` names at `provided_release`, so that a test about
    one tool decides nothing about another. `provides=None` is a host carrying
    no PowerShell at all; `shadow` puts a directory ahead of both holding a
    release-plz answering that release. Returns the tree, the directory installs
    land in, and the record of what `uv` was asked.
    """
    root = tmp_path / "tree"
    root.mkdir()
    shutil.copy2(REPO_ROOT / "repo-policy.toml", root / "repo-policy.toml")
    installs = tmp_path / "toolchain-bin"
    installs.mkdir()
    program(installs, "uv", uv)
    for present in ("cargo-nextest", "cargo-llvm-cov"):
        program(installs, present, f"print({f'{present} 0.0.1'!r})\n")
    if provides is not None:
        program(installs, provides, f"print({f'PowerShell {provided_release}'!r})\n")
    windows_lint = Repo(REPO_ROOT).policy["toolchain"]["windows_lint"]["target"]
    program(installs, "rustup", f"print({windows_lint!r})\n")
    record = tmp_path / "install-invocations"
    record.touch()
    directories = [installs]
    if shadow is not None:
        shadowing = tmp_path / "shadow"
        shadowing.mkdir()
        program(shadowing, "release-plz", answering(shadow))
        directories.insert(0, shadowing)
    monkeypatch.setenv("PATH", os.pathsep.join(str(directory) for directory in directories))
    monkeypatch.setenv("INSTALL_STANDIN_INTO", str(installs))
    monkeypatch.setenv("INSTALL_STANDIN_RECORD", str(record))
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

    equal(main(["install-tools", "release-plz", "--root", str(root)]), 0)

    contains(capsys.readouterr().err, said)
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"run -q python -m repo_checks install-release-plz {HELD}"],
        describing="what `uv` was asked to install",
    )
    contains(run(["release-plz", "--version"], check=True).stdout, f"release-plz version {HELD}")


def test_install_tools_installs_a_held_tool_absent_from_the_path_at_its_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A host with no release-plz at all gets the held release, not the newest."""
    root, _, record = toolchain(tmp_path, monkeypatch)

    equal(main(["install-tools", "release-plz", "--root", str(root)]), 0)

    contains(capsys.readouterr().err, "installing release-plz")
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"run -q python -m repo_checks install-release-plz {HELD}"],
        describing="what `uv` was asked to install",
    )
    contains(run(["release-plz", "--version"], check=True).stdout, f"release-plz version {HELD}")


def test_install_tools_reports_a_replacement_it_could_not_install(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A stale copy the install could not replace fails naming the command to run by hand."""
    root, installs, _ = toolchain(tmp_path, monkeypatch)
    program(installs, "release-plz", answering(STALE))
    program(installs, "uv", failing_with(101))

    equal(main(["install-tools", "release-plz", "--root", str(root)]), 1)

    contains(
        capsys.readouterr().err,
        f"failed to install release-plz. Run `uv run -q python -m repo_checks "
        f"install-release-plz {HELD}`",
    )
    contains(run(["release-plz", "--version"], check=True).stdout, f"release-plz {STALE}")


def test_install_tools_refuses_an_install_whose_program_names_no_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A program that cannot say it is the held release is not accepted as it."""
    root, _, _ = toolchain(tmp_path, monkeypatch)
    monkeypatch.setenv("INSTALL_STANDIN_ANSWERS", "(built from an unknown revision)")

    equal(main(["install-tools", "release-plz", "--root", str(root)]), 1)

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

    equal(main(["install-tools", "release-plz", "--root", str(root)]), 1)

    said = capsys.readouterr().err
    contains(said, f"{shutil.which('release-plz')} still answers {STALE}")
    contains(said, "a copy earlier on PATH shadows the one installed")
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"run -q python -m repo_checks install-release-plz {HELD}"],
        describing="what `uv` was asked to install",
    )


def test_install_tools_refuses_an_install_that_landed_off_the_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A program on no directory PATH names is a tool nothing that follows can run.

    It is the one outcome an install that exited zero can still leave, and it is
    what a host whose `~/.local/bin` is off PATH meets. What it must not read as
    is a copy shadowing the installed one, which is the other way a tool ends up
    answering wrong after an install that worked.
    """
    root, _, record = toolchain(tmp_path, monkeypatch)
    elsewhere = tmp_path / "off-the-path"
    elsewhere.mkdir()
    monkeypatch.setenv("INSTALL_STANDIN_INTO", str(elsewhere))

    equal(main(["install-tools", "release-plz", "--root", str(root)]), 1)

    said = capsys.readouterr().err
    contains(said, f"release-plz is on no directory PATH names after installing {HELD}")
    absent(said, "shadows the one installed")
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"run -q python -m repo_checks install-release-plz {HELD}"],
        describing="what `uv` was asked to install",
    )


def test_install_tools_accepts_a_held_tool_on_the_path_at_its_release_without_reinstalling(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The held release already on PATH is left as it is, and nothing is installed."""
    root, installs, record = toolchain(tmp_path, monkeypatch)
    program(installs, "release-plz", answering(HELD))

    equal(main(["install-tools", "release-plz", "--root", str(root)]), 0)

    equal(record.read_text(encoding="utf-8"), "", describing="what `uv` was asked")
    absent(capsys.readouterr().err, "installing")


#: The release the committed toolchain holds gh at, which it does not bootstrap.
GH_HELD = _held("gh")


def test_install_tools_leaves_a_tool_declared_not_to_bootstrap_alone(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """Bootstrap installs every host's tools, and not one job's: gh is left off PATH."""
    root, _, record = toolchain(tmp_path, monkeypatch)

    equal(main(["install-tools", "--root", str(root)]), 0)

    said = capsys.readouterr().err
    absent(said, "installing gh")
    absent(said, "installing release-plz")
    equal(record.read_text(encoding="utf-8"), "", describing="what `uv` was asked")
    equal(shutil.which("gh"), None, describing="a gh bootstrap left off PATH")
    equal(shutil.which("release-plz"), None, describing="a release-plz bootstrap left off PATH")


def test_install_tools_installs_a_tool_it_is_named_whatever_bootstrap_says(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The job that needs gh names it, and gets the held release through its install."""
    root, _, record = toolchain(tmp_path, monkeypatch)

    equal(main(["install-tools", "gh", "--root", str(root)]), 0)

    contains(capsys.readouterr().err, "installing gh")
    equal(
        record.read_text(encoding="utf-8").splitlines(),
        [f"run -q python -m repo_checks install-gh {GH_HELD}"],
        describing="what `uv` was asked to install",
    )
    contains(run(["gh", "--version"], check=True).stdout, f"gh version {GH_HELD}")


# A `uv` that runs the real install verb rather than standing in for it. The
# committed install for `pwsh` is `uv run -q python -m repo_checks
# install-powershell <release>`, and this runs exactly that verb, pointed at the
# stand-in release this suite serves and at a directory on the stand-in PATH. It
# is given this interpreter's own import path, because `repo_checks` reads its
# siblings and a subprocess inherits none of what pytest arranged. So what the
# bootstrap journey below drives is the real declaration, the real
# `install-tools` decision, and the real installer doing the whole of its own
# work: downloading, checking the digest, unpacking and linking.
FORWARDING_UV = """
import os
import sys

sys.path[:0] = os.environ["STANDIN_IMPORT_PATH"].split(os.pathsep)
from repo_checks.__main__ import main

arguments = sys.argv[1:]
verb = arguments[arguments.index("-m") + 2 :]
raise SystemExit(
    main(
        [
            *verb,
            "--releases",
            os.environ["STANDIN_RELEASES"],
            "--into",
            os.environ["STANDIN_INTO"],
        ]
    )
)
"""


@pytest.mark.skipif(sys.platform == "win32", reason="the PowerShell installer refuses Windows")
def test_bootstrap_installs_powershell_from_its_release_and_no_release_plz_at_all(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """`just bootstrap`'s own install path, on a host carrying no PowerShell.

    The one tool it installs is `pwsh`, taken from the release and verified
    before it is unpacked; release-plz is a job's rather than every host's, so
    nothing here fetches or compiles it.
    """
    root, installs, _ = toolchain(tmp_path, monkeypatch, provides=None, uv=FORWARDING_UV)
    release = Release()
    archive = powershell_archive_for(PWSH_HELD, sys.platform, platform.machine())
    with serving(f"/v{PWSH_HELD}/", release) as base:
        publish_powershell(release, archive)
        monkeypatch.setenv("STANDIN_RELEASES", base)
        monkeypatch.setenv("STANDIN_INTO", str(installs))
        monkeypatch.setenv("STANDIN_IMPORT_PATH", os.pathsep.join(sys.path))

        equal(main(["install-tools", "--root", str(root)]), 0)

    said = capsys.readouterr().err
    contains(said, "installing pwsh")
    absent(said, "installing release-plz")
    equal(
        release.asked,
        [f"/v{PWSH_HELD}/{HASHES}", f"/v{PWSH_HELD}/{archive.name}"],
        describing="what the bootstrap downloaded",
    )
    contains(run(["pwsh", "--version"], check=True).stdout, f"PowerShell {PWSH_HELD}")
    equal(shutil.which("release-plz"), None, describing="a release-plz bootstrap did not install")


@pytest.mark.parametrize(("provides", "release"), [("pwsh", "7.4.1"), ("powershell", "5.1.22621")])
def test_bootstrap_installs_nothing_where_the_host_already_provides_powershell(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    provides: str,
    release: str,
) -> None:
    """A PowerShell this repository did not install is not one it holds at a release.

    Neither of these answers the held release, and neither is replaced: a Windows
    host's `powershell` is part of the operating system, and a developer's own
    `pwsh` is theirs.
    """
    root, _, record = toolchain(tmp_path, monkeypatch, provides=provides, provided_release=release)

    equal(main(["install-tools", "--root", str(root)]), 0)

    absent(capsys.readouterr().err, "pwsh")
    equal(record.read_text(encoding="utf-8"), "", describing="what `uv` was asked")


def test_install_tools_refuses_a_tool_the_toolchain_does_not_declare(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """Naming a tool nobody declared installs nothing and says what is declared."""
    root, _, record = toolchain(tmp_path, monkeypatch)

    equal(main(["install-tools", "hub", "--root", str(root)]), 1)

    contains(capsys.readouterr().err, "declares no toolchain tool `hub`")
    equal(record.read_text(encoding="utf-8"), "", describing="what `uv` was asked")


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
