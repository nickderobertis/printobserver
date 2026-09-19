"""The PowerShell form of the bundled install script, driven the way route 3 drives it on Windows.

`scripts/install.ps1` is the third route's script for the two Windows
platforms, and it is PowerShell — which runs here. So the committed script is
driven under `pwsh` on every host, against a release directory staged in the
shape release automation publishes, with the two answers only Windows gives —
that the host is Windows, and which processor it has — reached through the
environment the script reads them from: the `OS` and `PROCESSOR_ARCHITECTURE`
variables every Windows sets, set here by the journey. Nothing about the
script is stood in for; what is stood in for is the host.

Every decision the script makes is walked: the platform it resolves, each
refusal, the digest it verifies before anything reaches a path, the pinned
release, the install directory, and what a successful run says to do next.
What only a Windows host can do — put a `.exe` in place and run it — the
Windows cells of the gate do, over this same journey with the real program.

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
import shutil
import sys
import tarfile
import tomllib
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import REPO_ROOT, clean_environment, run
from release_artifacts.stand_in import stand_in_program
from repo_checks import platforms
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run as shell_run

#: The committed script the third route fetches and runs on Windows.
SCRIPT = "scripts/install.ps1"

#: The file a release publishes its digests in.
CHECKSUMS = "SHA256SUMS"

#: The release the staged directory's older tag carries, and what its
#: stand-in program reports.
OLDER = "v0.0.1"

#: The two Windows platforms, as the supported-platform list names them.
WINDOWS = ["windows-x86_64", "windows-aarch64"]

#: How long the program build is given the first time this tier runs.
BUILD_TIMEOUT_SECONDS = 2400

#: The directory the script puts the program in when told none: under the
#: caller's own local application data, as Windows keeps per-user programs.
DEFAULT_DIRECTORY = ("AppData", "Local", "Programs", "printobserver")

#: The temporary directory a run is given, so that what the script left there
#: is this journey's to read.
TEMPORARY = "temp"

REPO = Repo(REPO_ROOT)


def _powershell() -> str:
    """The PowerShell on this host, and a next action where there is none.

    Raises:
        AssertionError: If neither `pwsh` nor `powershell` is on PATH.
    """
    for candidate in ("pwsh", "powershell"):
        found = shutil.which(candidate)
        if found:
            return found
    message = (
        "this journey drives the PowerShell install script under PowerShell, and this host "
        "has neither `pwsh` nor `powershell` on PATH; install PowerShell 7 "
        "(https://github.com/PowerShell/PowerShell/releases) and put `pwsh` on PATH"
    )
    raise AssertionError(message)


def _machine(identifier: str) -> str:
    """What `PROCESSOR_ARCHITECTURE` reports on one Windows platform.

    Read out of the one table that maps what a Windows host reports to a
    platform identifier, rather than restated here.
    """
    return next(
        machine
        for (system, machine), named in platforms.HOSTS.items()
        if system == "Windows" and named == identifier
    )


def _version() -> str:
    """The version release automation wrote into the workspace."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["version"])


def _program(identifier: str, root: Path) -> bytes:
    """The bytes the newest release's artifact carries for one platform.

    On a Windows host of that platform, the `printobserver` program this
    repository builds — so the route is proven putting the real thing in
    place. Everywhere else a stand-in in the form this host runs one, because
    a program built for another machine cannot be run to read its version.
    """
    here = platforms.host(REPO)
    if here.id == identifier:
        built = REPO_ROOT / "target" / "release" / here.program
        if not built.is_file():
            passing(
                run(
                    ["cargo", "build", "--release", "--locked", "-p", "printobserver"],
                    REPO_ROOT,
                    timeout=BUILD_TIMEOUT_SECONDS,
                ),
                describing="building the program a release carries",
            )
        return built.read_bytes()
    return stand_in_program(
        REPO, root / "newest" / "printobserver.exe", f"printobserver {_version()}"
    ).read_bytes()


def _artifact(into: Path, program: bytes, identifier: str) -> Path:
    """One release artifact, in the shape release automation publishes it."""
    platform = platforms.descriptor(REPO, identifier)
    into.mkdir(parents=True, exist_ok=True)
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        info = tarfile.TarInfo(platform.program)
        info.size = len(program)
        info.mode = 0o755
        archive.addfile(info, io.BytesIO(program))
    target = into / platform.asset
    target.write_bytes(gzip.compress(raw.getvalue(), mtime=0))
    with (into / CHECKSUMS).open("a", encoding="utf-8") as digests:
        digests.write(f"{hashlib.sha256(target.read_bytes()).hexdigest()}  {target.name}\n")
    return target


@pytest.fixture
def staged(tmp_path: Path) -> Callable[[str], Path]:
    """A release directory holding two releases for one platform, as the forge serves them.

    `latest/download` is what the forge serves for the newest release and
    `download/<tag>` for a pinned one, so a script proven against this is
    proven against the layout it will meet.
    """

    def stage(identifier: str) -> Path:
        base = tmp_path / "releases"
        newest = _program(identifier, tmp_path)
        older = stand_in_program(
            REPO,
            tmp_path / "older" / "printobserver.exe",
            f"printobserver {OLDER.removeprefix('v')}",
        ).read_bytes()
        _artifact(base / "download" / OLDER, older, identifier)
        _artifact(base / "download" / f"v{_version()}", newest, identifier)
        _artifact(base / "latest" / "download", newest, identifier)
        return base

    return stage


def _environment(staged: Path, home: Path, identifier: str | None) -> dict[str, str]:
    """The environment one run is given: a Windows host of `identifier`, or this host's own.

    The two answers the script reads about the host are the variables Windows
    itself sets, and here they are set by the journey — which is how the
    Windows answers are reached on a host that is not one. `None` leaves the
    host's own answers in place.
    """
    temporary = home / TEMPORARY
    temporary.mkdir(parents=True, exist_ok=True)
    environment = clean_environment(
        PRINTOBSERVER_RELEASE_BASE=str(staged),
        LOCALAPPDATA=str(home / "AppData" / "Local"),
        TMPDIR=str(temporary),
        TMP=str(temporary),
        TEMP=str(temporary),
    )
    environment.pop("PROCESSOR_ARCHITEW6432", None)
    if identifier is not None:
        environment["OS"] = "Windows_NT"
        environment["PROCESSOR_ARCHITECTURE"] = _machine(identifier)
    return environment


def _script(staged: Path, home: Path, identifier: str | None, *arguments: str) -> tuple[int, str]:
    """Run the committed script under PowerShell, as a host of `identifier`."""
    result = shell_run(
        [_powershell(), "-NoProfile", "-File", str(REPO_ROOT / SCRIPT), *arguments],
        cwd=home,
        env=_environment(staged, home, identifier),
        timeout=600,
    )
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def _reports(program: Path) -> str:
    """What one installed program says its own version is."""
    result = run([str(program), "--version"], program.parent, timeout=120)
    passing(result, describing=f"{program} reporting its own version")
    return (result.stdout + result.stderr).strip()


def _default(home: Path) -> Path:
    """Where the script installs when told no directory."""
    return home.joinpath(*DEFAULT_DIRECTORY) / "printobserver.exe"


def _home(tmp_path: Path, name: str) -> Path:
    home = tmp_path / name
    home.mkdir()
    return home


@pytest.mark.parametrize("identifier", WINDOWS)
def test_it_installs_the_newest_release_into_the_default_directory_and_says_what_is_next(
    identifier: str, staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """Run with neither a pinned release nor a directory, it takes the newest.

    And a successful run says where the program went, that its directory is
    now on the path, and the two commands the operator runs after it.
    """
    home = _home(tmp_path, f"home-default-{identifier}")

    code, said = _script(staged(identifier), home, identifier)

    passing((code, said), describing=f"the install script with no options on {identifier}")
    installed = _default(home)
    truth(installed.is_file(), describing=f"{installed} to be the program it installed")
    contains(_reports(installed), _version(), describing="what the installed program reports")
    contains(said, f"installed {installed}", describing="what the run said")
    contains(said, "was added to your PATH", describing="what the run said about the path")
    contains(said, "next, in an elevated PowerShell", describing="the next step it named")
    contains(said, "scripts/install-service.ps1 | iex", describing="the installer it named")
    contains(said, "Set-Service -Name printobserver", describing="the start it named")
    equal(
        sorted((home / TEMPORARY).iterdir()),
        [],
        describing="the directory of the script's own, removed when it was done",
    )


def test_a_pinned_release_is_the_one_installed(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """`-Version` takes that release rather than the newest."""
    home = _home(tmp_path, "home-pinned")

    code, said = _script(staged(WINDOWS[0]), home, WINDOWS[0], "-Version", OLDER)

    passing((code, said), describing=f"the install script pinned to {OLDER}")
    contains(_reports(_default(home)), OLDER.removeprefix("v"), describing="the pinned program")
    contains(said, f"from release {OLDER}", describing="which release the run said it took")


def test_an_install_directory_is_the_one_installed_into(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """`-To` installs there, and leaves the default directory alone."""
    home = _home(tmp_path, "home-elsewhere")
    elsewhere = home / "tools" / "printobserver"

    code, said = _script(staged(WINDOWS[1]), home, WINDOWS[1], "-To", str(elsewhere))

    passing((code, said), describing="the install script with an install directory")
    installed = elsewhere / "printobserver.exe"
    truth(installed.is_file(), describing=f"{installed} to be installed")
    truth(not _default(home).exists(), describing="the default directory to be left alone")
    contains(_reports(installed), _version(), describing="what the installed program reports")


def test_an_altered_artifact_is_refused_before_anything_reaches_a_path(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """Verification happens before anything reaches a path, not after."""
    home = _home(tmp_path, "home-altered")
    into = home / "bin"
    base = staged(WINDOWS[0])
    altered = base / "latest" / "download" / platforms.descriptor(REPO, WINDOWS[0]).asset
    altered.write_bytes(altered.read_bytes() + b"an alteration nobody published")

    code, said = _script(base, home, WINDOWS[0], "-To", str(into))

    failing((code, said), naming="does not match the digest")
    truth(not into.exists(), describing="nothing to have reached the directory it was given")
    contains(said, "Nothing was installed", describing="what the refusal said")
    contains(said, "try again", describing="the next action it named")
    equal(sorted((home / TEMPORARY).iterdir()), [], describing="its own directory, removed")


def test_a_processor_it_publishes_nothing_for_stops_with_a_next_action(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A Windows host of a processor no release builds for is a stop rather than a guess."""
    home = _home(tmp_path, "home-processor")
    environment = _environment(staged(WINDOWS[0]), home, WINDOWS[0])
    environment["PROCESSOR_ARCHITECTURE"] = "x86"
    result = shell_run(
        [_powershell(), "-NoProfile", "-File", str(REPO_ROOT / SCRIPT)],
        cwd=home,
        env=environment,
        timeout=600,
    )
    said = (result.stdout or "") + (result.stderr or "")

    failing((result.returncode, said), naming="publishes no program for")
    contains(said, "windows/x86", describing="the host it named")
    for identifier in WINDOWS:
        contains(said, identifier, describing="the platforms it does publish for")
    contains(said, "build it from source", describing="the next action it named")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_a_host_that_is_not_windows_is_sent_to_the_shell_form(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """The PowerShell form is Windows's; on anything else it names the script that is not."""
    home = _home(tmp_path, "home-elsewhere-os")
    environment = _environment(staged(WINDOWS[0]), home, None)
    environment["OS"] = "Haiku"

    result = shell_run(
        [_powershell(), "-NoProfile", "-File", str(REPO_ROOT / SCRIPT)],
        cwd=home,
        env=environment,
        timeout=600,
    )
    said = (result.stdout or "") + (result.stderr or "")

    failing((result.returncode, said), naming="the Windows form of the install script")
    contains(said, "scripts/install.sh | sh", describing="the shell form it sent the caller to")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_a_download_that_cannot_be_obtained_stops_with_a_next_action(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A release with nothing to download is a stop rather than a silent success."""
    home = _home(tmp_path, "home-missing")

    code, said = _script(staged(WINDOWS[0]), home, WINDOWS[0], "-Version", "v9.9.9")

    failing((code, said), naming="has no")
    contains(said, "-Version", describing="the next action it named")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_a_release_publishing_no_digests_is_refused(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """What cannot be verified is not installed."""
    home = _home(tmp_path, "home-nodigest")
    base = staged(WINDOWS[1])
    (base / "latest" / "download" / CHECKSUMS).unlink()

    code, said = _script(base, home, WINDOWS[1])

    failing((code, said), naming="cannot be verified")
    contains(said, "Nothing was installed", describing="what the refusal said")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_a_digest_file_naming_no_digest_for_the_artifact_is_refused(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A checksum file that lists other artifacts and not this one verifies nothing."""
    home = _home(tmp_path, "home-unlisted")
    base = staged(WINDOWS[0])
    digests = base / "latest" / "download" / CHECKSUMS
    digests.write_text(f"{'0' * 64}  printobserver-elsewhere.tar.gz\n", encoding="utf-8")

    code, said = _script(base, home, WINDOWS[0])

    failing((code, said), naming="names no digest for")
    contains(said, "Nothing was installed", describing="what the refusal said")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_an_artifact_carrying_no_program_is_refused(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A verified artifact with the wrong thing in it is still nothing to install."""
    home = _home(tmp_path, "home-empty")
    base = staged(WINDOWS[0])
    into = base / "latest" / "download"
    asset = into / platforms.descriptor(REPO, WINDOWS[0]).asset
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        info = tarfile.TarInfo("README")
        info.size = 0
        archive.addfile(info, io.BytesIO(b""))
    asset.write_bytes(gzip.compress(raw.getvalue(), mtime=0))
    (into / CHECKSUMS).write_text(
        f"{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n", encoding="utf-8"
    )

    code, said = _script(base, home, WINDOWS[0])

    failing((code, said), naming="carries no printobserver.exe")
    contains(said, "Nothing was installed", describing="what the refusal said")


def test_the_script_is_committed_where_its_own_fetch_url_names() -> None:
    """The one-line command that route states fetches a path this repository has."""
    truth((REPO_ROOT / SCRIPT).is_file(), describing=f"{SCRIPT} to be committed")
    contains(
        (REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"),
        f"main/{SCRIPT} | iex",
        describing="the install path's third route, in its PowerShell form",
    )


@pytest.mark.skipif(
    sys.platform != "win32", reason="the host's own answers are Windows's only there"
)
def test_this_windows_host_resolves_itself_with_nothing_stood_in(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """On a Windows host the script reads the host, and installs the real program."""
    here = platforms.host(REPO)
    home = _home(tmp_path, "home-here")
    environment = _environment(staged(here.id), home, None)
    for name in ("OS", "PROCESSOR_ARCHITECTURE"):
        environment[name] = os.environ[name]

    result = shell_run(
        [_powershell(), "-NoProfile", "-File", str(REPO_ROOT / SCRIPT)],
        cwd=home,
        env=environment,
        timeout=600,
    )
    said = (result.stdout or "") + (result.stderr or "")

    passing((result.returncode, said), describing=f"the install script on {here.id} itself")
    contains(_reports(_default(home)), _version(), describing="what the real program reports")
