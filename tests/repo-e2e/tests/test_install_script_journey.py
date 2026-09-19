"""The bundled install script, driven the way the install path's third route drives it.

Nothing here is mocked and nothing here reaches the network. A release
directory is staged in the shape release automation publishes — two releases,
each carrying this host's own platform artifact and the checksum file the
script verifies against — and the committed script is run against it with `sh`,
exactly as that route's own one-line command runs it.

Two releases rather than one, and the older one is a stand-in program that
reports a version of its own: what the selection assertions are about is which
release the script chose, and two artifacts carrying the same program could not
tell a reader that.
"""

from __future__ import annotations

import gzip
import hashlib
import io
import os
import platform as host_platform
import tarfile
import tomllib
from pathlib import Path

import pytest
from journey import NO_ROUTE_HERE, REPO_ROOT, clean_environment, run
from repo_checks import install_path
from repo_checks.checks_platforms import SCRIPT_PLATFORM
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.platforms import HOSTS, host, install_platforms
from repo_checks.shell import run as shell_run

#: The committed script the third route fetches and runs.
SCRIPT = "scripts/install.sh"

#: The program every route puts on a path.
PROGRAM = "printobserver"

#: The file a release publishes its digests in.
CHECKSUMS = "SHA256SUMS"

#: The release the staged directory's older tag carries, and what its
#: stand-in program reports.
OLDER = "v0.0.1"


def _platform() -> str:
    """The platform this host's own artifact is named for, as the script names it.

    Read through `repo_checks.platforms`, the one place that knows which
    platform a host is — `os.uname` does not exist on Windows, and a journey
    that raised on import there could not even say it had nothing to run.
    """
    return host(Repo(REPO_ROOT)).id


#: Where this host's own artifact is named, as the platform declaration names it.
PLATFORM = _platform()

#: The platforms the shell form reaches: its own `uname` arms, read off the
#: committed script the way `just check-repo` reads them. A platform the
#: install path targets that is not among them — Windows — is reached by the
#: PowerShell form and its own journey.
REACHED = frozenset(SCRIPT_PLATFORM.findall((REPO_ROOT / SCRIPT).read_text(encoding="utf-8")))

#: Why this journey has nothing to drive on this host, or `None`: a host the
#: install path does not target, or one this script has no arm for.
NOT_HERE: str | None = (
    NO_ROUTE_HERE
    if NO_ROUTE_HERE is not None
    else None
    if PLATFORM in REACHED
    else f"{SCRIPT} has no arm for `{PLATFORM}`, which the PowerShell form reaches instead"
)
SHELL_ROUTE_JOURNEY = pytest.mark.skipif(NOT_HERE is not None, reason=NOT_HERE or "")

#: How long the program build is given the first time this tier runs.
BUILD_TIMEOUT_SECONDS = 2400


def _version() -> str:
    """The version release automation wrote into the workspace."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["version"])


def _program() -> Path:
    """The `printobserver` program this repository builds, built once."""
    built = REPO_ROOT / "target/release" / PROGRAM
    if not built.is_file():
        passing(
            run(
                ["cargo", "build", "--release", "--locked", "-p", PROGRAM],
                REPO_ROOT,
                timeout=BUILD_TIMEOUT_SECONDS,
            ),
            describing="building the program a release carries",
        )
    return built


def _artifact(into: Path, program: bytes, platform: str = PLATFORM) -> Path:
    """One release artifact, in the shape release automation publishes it.

    Its digest is added to the checksum file beside it rather than replacing
    what that file already lists, so one release can carry several platforms'.
    """
    into.mkdir(parents=True, exist_ok=True)
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        info = tarfile.TarInfo(PROGRAM)
        info.size = len(program)
        info.mode = 0o755
        archive.addfile(info, io.BytesIO(program))
    target = into / f"{PROGRAM}-{platform}.tar.gz"
    target.write_bytes(gzip.compress(raw.getvalue(), mtime=0))
    with (into / CHECKSUMS).open("a", encoding="utf-8") as listing:
        listing.write(f"{hashlib.sha256(target.read_bytes()).hexdigest()}  {target.name}\n")
    return target


@pytest.fixture
def staged(tmp_path: Path) -> Path:
    """A release directory holding two releases, in the shape the forge serves.

    `latest/download` is what the forge serves for the newest release and
    `download/<tag>` for a pinned one, so a script proven against this is
    proven against the layout it will meet.

    Staged only where the install path targets this platform and this script
    reaches it: the script is the third route's shell form, and on a platform
    whose record answers `install path: no` there is no artifact of this
    host's for it to stage, while a Windows host is reached by the PowerShell
    form and its own journey — so a journey asking for one is skipped naming
    which rather than built for.
    """
    if NOT_HERE is not None:
        pytest.skip(NOT_HERE)
    base = tmp_path / "releases"
    real = _program().read_bytes()
    stand_in = f'#!/bin/sh\necho "{PROGRAM} {OLDER.removeprefix("v")}"\n'.encode()

    _artifact(base / "download" / OLDER, stand_in)
    _artifact(base / "download" / f"v{_version()}", real)
    _artifact(base / "latest" / "download", real)
    return base


def _script(staged: Path, home: Path, *arguments: str) -> tuple[int, str]:
    """Run the script under a home of this journey's own, against a staged release."""
    environment = clean_environment(HOME=str(home), PRINTOBSERVER_RELEASE_BASE=str(staged))
    result = shell_run(
        ["sh", str(REPO_ROOT / SCRIPT), *arguments],
        cwd=home,
        env=environment,
        timeout=600,
    )
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def _reports(program: Path) -> str:
    """What one installed program says its own version is."""
    result = run([str(program), "--version"], program.parent, timeout=120)
    passing(result, describing=f"{program} reporting its own version")
    return (result.stdout + result.stderr).strip()


def test_it_installs_the_newest_release_into_the_default_directory(
    staged: Path, tmp_path: Path
) -> None:
    """Run with neither a pinned release nor a directory, it takes the newest."""
    home = tmp_path / "home-default"
    home.mkdir()

    code, said = _script(staged, home)

    passing((code, said), describing="the install script with no options")
    installed = home / ".local/bin" / PROGRAM
    truth(installed.is_file(), describing=f"{installed} to be the program it installed")
    contains(_reports(installed), _version(), describing="what the installed program reports")


def test_a_pinned_release_is_the_one_installed(staged: Path, tmp_path: Path) -> None:
    """`--version` takes that release rather than the newest."""
    home = tmp_path / "home-pinned"
    home.mkdir()

    code, said = _script(staged, home, "--version", OLDER)

    passing((code, said), describing=f"the install script pinned to {OLDER}")
    installed = home / ".local/bin" / PROGRAM
    contains(_reports(installed), OLDER.removeprefix("v"), describing="the pinned program")


def test_an_install_directory_is_the_one_installed_into(staged: Path, tmp_path: Path) -> None:
    """`--to` installs there, and leaves the default directory alone."""
    home = tmp_path / "home-elsewhere"
    elsewhere = home / "opt/bin"
    home.mkdir()

    code, said = _script(staged, home, "--to", str(elsewhere))

    passing((code, said), describing="the install script with an install directory")
    truth((elsewhere / PROGRAM).is_file(), describing=f"{elsewhere / PROGRAM} to be installed")
    truth(
        not (home / ".local/bin" / PROGRAM).exists(),
        describing="the default directory to be left alone",
    )


def test_an_altered_artifact_is_refused_before_anything_reaches_a_path(
    staged: Path, tmp_path: Path
) -> None:
    """Verification happens before anything reaches a path, not after."""
    home = tmp_path / "home-altered"
    into = home / "bin"
    home.mkdir()
    altered = staged / "latest/download" / f"{PROGRAM}-{PLATFORM}.tar.gz"
    altered.write_bytes(altered.read_bytes() + b"an alteration nobody published")

    code, said = _script(staged, home, "--to", str(into))

    failing((code, said), naming="does not match the digest")
    truth(
        not (into / PROGRAM).exists(),
        describing="nothing to have been installed at the directory it was given",
    )
    contains(said, "Nothing was installed", describing="what the refusal said")


#: Every `uname` answer the platform declaration maps to a platform the install
#: path targets and this script has an arm for, read from that declaration and
#: the script's own arms rather than restated here.
TARGETED = [
    (system, machine, identifier)
    for (system, machine), identifier in HOSTS.items()
    if identifier in {platform.id for platform in install_platforms(Repo(REPO_ROOT))}
    and identifier in REACHED
]


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
def _uname(shims: Path, system: str, machine: str) -> None:
    """A `uname` on the path answering one system and one processor."""
    shims.mkdir(parents=True, exist_ok=True)
    (shims / "uname").write_text(
        f'#!/bin/sh\ncase "$1" in\n  -s) echo {system} ;;\n  -m) echo {machine} ;;\n'
        f"  *) echo {system} ;;\nesac\n",
        encoding="utf-8",
    )
    (shims / "uname").chmod(0o755)


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
@pytest.mark.parametrize(
    ("system", "machine", "identifier"),
    TARGETED,
    ids=[f"{system}-{machine}" for system, machine, _ in TARGETED],
)
@SHELL_ROUTE_JOURNEY
def test_each_targeted_platform_installs_its_own_artifact_and_names_its_own_start_command(
    tmp_path: Path, system: str, machine: str, identifier: str
) -> None:
    """Each `uname` answer is mapped to the identifier the platform list names.

    The release carries one stand-in program per targeted platform, each saying
    which platform it was published for, so what the installed program reports
    is which artifact the script chose. What it prints next is that platform's
    own start command, as the install path states it for its service manager.

    A route journey like the rest of this module, so it is skipped on a host
    the install path does not target: what it drives is the third route's own
    shell script, and the stand-in it installs is a shell script too, so a
    host that cannot take that route cannot run either — the script's `sh`
    there hands a drive-lettered release directory to `curl` as a URL.
    """
    repo = Repo(REPO_ROOT)
    base = tmp_path / "releases"
    for platform in install_platforms(repo):
        # llmlint: ignore[e2e_not_mocked] one host runs one platform's program; suppressions.toml
        _artifact(
            base / "latest" / "download",
            f'#!/bin/sh\necho "{PROGRAM} for {platform.id}"\n'.encode(),
            platform.id,
        )
    home = tmp_path / "home"
    shims = home / "shims"
    # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reasons.
    _uname(shims, system, machine)
    environment = clean_environment(
        HOME=str(home),
        PRINTOBSERVER_RELEASE_BASE=str(base),
        PATH=f"{shims}{os.pathsep}{os.environ['PATH']}",
    )

    result = shell_run(["sh", str(REPO_ROOT / SCRIPT)], cwd=home, env=environment, timeout=600)
    said = (result.stdout or "") + (result.stderr or "")

    passing((result.returncode, said), describing=f"the install script on {system}/{machine}")
    equal(
        _reports(home / ".local/bin" / PROGRAM),
        f"{PROGRAM} for {identifier}",
        describing=f"the artifact installed for {system}/{machine}",
    )
    manager = next(one for one in install_platforms(repo) if one.id == identifier).service_manager
    start = install_path.parse(repo.agents_md).commands_for(manager)[1]
    contains(said, start, describing=f"the start command it names on {system}/{machine}")


def test_a_platform_it_publishes_nothing_for_stops_with_a_next_action(
    staged: Path, tmp_path: Path
) -> None:
    """A platform that cannot be resolved is a stop rather than a guess."""
    home = tmp_path / "home-platform"
    home.mkdir()
    shims = home / "shims"
    shims.mkdir()
    (shims / "uname").write_text(
        '#!/bin/sh\ncase "$1" in\n  -s) echo Haiku ;;\n  -m) echo m68k ;;\n'
        "  *) echo Haiku ;;\nesac\n",
        encoding="utf-8",
    )
    (shims / "uname").chmod(0o755)
    environment = clean_environment(
        HOME=str(home),
        PRINTOBSERVER_RELEASE_BASE=str(staged),
        PATH=f"{shims}{os.pathsep}{os.environ['PATH']}",
    )
    result = shell_run(["sh", str(REPO_ROOT / SCRIPT)], cwd=home, env=environment, timeout=600)
    said = (result.stdout or "") + (result.stderr or "")

    failing((result.returncode, said), naming="publishes no program for")
    contains(said, "build it from source", describing="the next action it named")


def test_a_download_that_cannot_be_obtained_stops_with_a_next_action(
    staged: Path, tmp_path: Path
) -> None:
    """A release with nothing to download is a stop rather than a silent success."""
    home = tmp_path / "home-missing"
    home.mkdir()

    code, said = _script(staged, home, "--version", "v9.9.9")

    failing((code, said), naming="has no")
    contains(said, "--version", describing="the next action it named")
    truth(
        not (home / ".local/bin" / PROGRAM).exists(),
        describing="nothing to have been installed",
    )


def test_a_release_publishing_no_digests_is_refused(staged: Path, tmp_path: Path) -> None:
    """What cannot be verified is not installed."""
    home = tmp_path / "home-nodigest"
    home.mkdir()
    (staged / "latest/download" / CHECKSUMS).unlink()

    code, said = _script(staged, home)

    failing((code, said), naming="cannot be verified")
    contains(said, "Nothing was installed", describing="what the refusal said")


def test_the_script_is_committed_where_its_own_fetch_url_names() -> None:
    """The one-line command that route states fetches a path this repository has."""
    truth((REPO_ROOT / SCRIPT).is_file(), describing=f"{SCRIPT} to be committed")
    contains(
        (REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"),
        f"main/{SCRIPT} | sh",
        describing="the install path's third route",
    )


def test_the_platform_an_artifact_is_named_for_is_read_on_a_host_with_no_uname(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A Windows host has no `os.uname`, and this journey still says which platform it is."""
    monkeypatch.delattr(os, "uname", raising=False)
    monkeypatch.setattr(host_platform, "system", lambda: "Windows")
    monkeypatch.setattr(host_platform, "machine", lambda: "AMD64")

    equal(_platform(), "windows-x86_64", describing="the platform a Windows host is")
