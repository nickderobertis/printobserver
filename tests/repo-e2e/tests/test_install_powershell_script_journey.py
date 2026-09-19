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
import sys
import tarfile
import tomllib
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import REPO_ROOT, clean_environment, run
from release_artifacts.installing import powershell
from release_artifacts.stand_in import stand_in_program
from repo_checks import platforms
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run as shell_run

#: The committed script the third route fetches and runs on Windows.
SCRIPT = "scripts/install.ps1"

#: The file a release publishes its digests in.
CHECKSUMS = "SHA256SUMS"

#: The release the staged directory's older tag carries, and what its
#: stand-in program reports.
OLDER = "v0.0.1"

REPO = Repo(REPO_ROOT)

#: The Windows platforms, read off the supported-platform list by the service
#: manager their column names rather than restated: the ones this script is
#: the route for.
WINDOWS = [
    platform.id
    for platform in platforms.supported(REPO)
    if platform.service_manager == platforms.ServiceManager.WINDOWS_SERVICE
]

#: How long the program build is given the first time this tier runs.
BUILD_TIMEOUT_SECONDS = 2400

#: The directory the script puts the program in when told none: under the
#: caller's own local application data, as Windows keeps per-user programs.
DEFAULT_DIRECTORY = ("AppData", "Local", "Programs", "printobserver")

#: The temporary directory a run is given, so that what the script left there
#: is this journey's to read.
TEMPORARY = "temp"


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


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def _built_here(platform: platforms.Platform) -> bytes:
    """The `printobserver` program this repository builds for this host, built once."""
    built = REPO_ROOT / "target" / "release" / platform.program
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


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
def _program(identifier: str, root: Path) -> bytes:
    """The bytes the newest release's artifact carries for one platform.

    On a Windows host of that platform, the `printobserver` program this
    repository builds — so the route is proven putting the real thing in
    place. Everywhere else a stand-in in the form this host runs one, because
    a program built for another machine cannot be run to read its version.
    """
    here = platforms.host(REPO)
    if here.id == identifier:
        return _built_here(here)
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


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
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


# llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reasons.
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


def _run(home: Path, environment: dict[str, str], *arguments: str) -> tuple[int, str]:
    """Run the committed script as a file under this host's PowerShell."""
    result = shell_run(
        [powershell(), "-NoProfile", "-File", str(REPO_ROOT / SCRIPT), *arguments],
        cwd=home,
        env=environment,
        timeout=600,
    )
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def _script(staged: Path, home: Path, identifier: str | None, *arguments: str) -> tuple[int, str]:
    """Run the committed script under PowerShell, as a host of `identifier`."""
    return _run(home, _environment(staged, home, identifier), *arguments)


def _piped(home: Path, environment: dict[str, str], *arguments: str) -> tuple[int, str]:
    """Run the script the way the route's own command runs it: its text piped into `iex`.

    With arguments, the way the pinned form passes them — a script block made
    of the same text. The session goes on after it and says what status the
    script left it, which is what a caller at the prompt reads.
    """
    text = str(REPO_ROOT / SCRIPT).replace("'", "''")
    invoked = (
        f"& ([scriptblock]::Create((Get-Content -LiteralPath '{text}' -Raw))) {' '.join(arguments)}"
        if arguments
        else f"Get-Content -LiteralPath '{text}' -Raw | iex"
    )
    result = shell_run(
        [
            powershell(),
            "-NoProfile",
            "-Command",
            f'{invoked}; Write-Output "the session goes on, LASTEXITCODE=$LASTEXITCODE"',
        ],
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

    code, said = _run(home, environment)

    failing((code, said), naming="publishes no program for")
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

    code, said = _run(home, environment)

    failing((code, said), naming="the Windows form of the install script")
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


def test_the_piped_form_installs_and_leaves_the_session_with_a_zero_status(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """`irm ... | iex` is how the route states it, and a success leaves `$LASTEXITCODE` zero."""
    home = _home(tmp_path, "home-piped")

    code, said = _piped(home, _environment(staged(WINDOWS[0]), home, WINDOWS[0]))

    passing((code, said), describing="the session the script was piped into")
    contains(said, "the session goes on, LASTEXITCODE=0", describing="what the session said after")
    contains(_reports(_default(home)), _version(), describing="what the installed program reports")


def test_the_piped_form_that_stopped_keeps_the_session_and_leaves_a_failing_status(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A stop under `iex` must not close the caller's window, and must still read as one.

    An `exit` there would take the window and both sentences with it; what a
    caller — or a CI step's PowerShell — reads instead is `$LASTEXITCODE`.
    """
    home = _home(tmp_path, "home-piped-stop")
    environment = _environment(staged(WINDOWS[0]), home, WINDOWS[0])
    environment["PROCESSOR_ARCHITECTURE"] = "x86"

    code, said = _piped(home, environment)

    equal(code, 0, describing=f"the session's own status, which a stop must not end: {said}")
    contains(said, "publishes no program for", describing="the stop's own sentence")
    contains(said, "the session goes on, LASTEXITCODE=1", describing="what the session read")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_the_pinned_form_passes_both_options_through_a_script_block(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """The install path's pinned form is a script block made of the fetched text, with options."""
    home = _home(tmp_path, "home-piped-pinned")
    elsewhere = home / "pinned"

    code, said = _piped(
        home,
        _environment(staged(WINDOWS[1]), home, WINDOWS[1]),
        "-Version",
        OLDER,
        "-To",
        f"'{elsewhere}'",
    )

    passing((code, said), describing="the pinned form")
    contains(said, "the session goes on, LASTEXITCODE=0", describing="what the session said after")
    contains(
        _reports(elsewhere / "printobserver.exe"),
        OLDER.removeprefix("v"),
        describing="the pinned program, where it was asked for",
    )


def test_help_says_the_two_options_and_installs_nothing(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """`-Help` is the one run that is not an install."""
    home = _home(tmp_path, "home-help")

    code, said = _script(staged(WINDOWS[0]), home, WINDOWS[0], "-Help")

    passing((code, said), describing="the script asked for help")
    contains(said, "-Version", describing="the options it names")
    contains(said, "-To", describing="the options it names")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_an_emulated_session_installs_for_the_machine_rather_than_the_process(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A 64-bit x86 PowerShell emulated on an ARM64 machine still installs on an ARM64 machine.

    Windows tells an emulated process the machine's own processor in
    `PROCESSOR_ARCHITEW6432`, and that is what the script reads first.
    """
    home = _home(tmp_path, "home-emulated")
    arm = next(identifier for identifier in WINDOWS if _machine(identifier) == "ARM64")
    x64 = next(identifier for identifier in WINDOWS if _machine(identifier) == "AMD64")
    environment = _environment(staged(arm), home, x64)
    environment["PROCESSOR_ARCHITEW6432"] = _machine(arm)

    code, said = _run(home, environment)

    passing((code, said), describing="the script under an emulated PowerShell")
    contains(_reports(_default(home)), _version(), describing="the program for the machine")


def test_a_release_tag_that_is_not_one_is_refused_before_anything_is_fetched(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """`-Version` reaches a path and a URL, so it is held to the shape of a tag first."""
    home = _home(tmp_path, "home-not-a-tag")

    code, said = _script(staged(WINDOWS[0]), home, WINDOWS[0], "-Version", "../latest")

    failing((code, said), naming="is not a release tag")
    contains(said, "v0.1.0", describing="the shape it asked for")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_a_session_with_no_local_application_data_is_told_to_name_a_directory(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """With no default to install into, the script asks for `-To` rather than guessing."""
    home = _home(tmp_path, "home-no-localappdata")
    environment = _environment(staged(WINDOWS[0]), home, WINDOWS[0])
    del environment["LOCALAPPDATA"]

    code, said = _run(home, environment)

    failing((code, said), naming="no LOCALAPPDATA")
    contains(said, "Pass -To", describing="the next action it named")


def test_a_file_url_names_a_release_directory_too(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A mirror named as `file://` is the same directory as one named by its path."""
    home = _home(tmp_path, "home-file-url")
    environment = _environment(staged(WINDOWS[0]), home, WINDOWS[0])
    environment["PRINTOBSERVER_RELEASE_BASE"] = (
        "file://" + environment["PRINTOBSERVER_RELEASE_BASE"]
    )

    code, said = _run(home, environment)

    passing((code, said), describing="the script against a file:// release base")
    contains(_reports(_default(home)), _version(), describing="what the installed program reports")


def test_a_machine_with_no_tar_is_told_where_windows_keeps_one(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """Unpacking needs `tar`, and a session whose path has none is told so before unpacking."""
    home = _home(tmp_path, "home-no-tar")
    empty = home / "empty-path"
    empty.mkdir()
    environment = _environment(staged(WINDOWS[0]), home, WINDOWS[0])
    environment["PATH"] = str(empty)

    code, said = _run(home, environment)

    failing((code, said), naming="has no tar")
    contains(said, "tar.exe", describing="where it said Windows keeps one")
    contains(said, "Nothing was installed", describing="what the refusal said")


def _served_as_the_newest(base: Path, identifier: str, content: bytes) -> None:
    """Replace the newest release's artifact with `content`, digest and all."""
    into = base / "latest" / "download"
    asset = into / platforms.descriptor(REPO, identifier).asset
    asset.write_bytes(content)
    (into / CHECKSUMS).write_text(
        f"{hashlib.sha256(content).hexdigest()}  {asset.name}\n", encoding="utf-8"
    )


def test_an_artifact_that_is_not_an_archive_is_refused_after_its_digest_matched(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A verified download that will not unpack is still nothing to install."""
    home = _home(tmp_path, "home-not-an-archive")
    base = staged(WINDOWS[0])
    _served_as_the_newest(base, WINDOWS[0], b"not a gzip archive at all")

    code, said = _script(base, home, WINDOWS[0])

    failing((code, said), naming="could not be unpacked")
    contains(said, "Nothing was installed", describing="what the refusal said")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_an_archive_naming_a_place_outside_its_own_directory_is_refused(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A member that would land outside the script's directory is read before any member lands."""
    home = _home(tmp_path, "home-escaping")
    base = staged(WINDOWS[0])
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        info = tarfile.TarInfo("../escaped.exe")
        info.size = 0
        archive.addfile(info, io.BytesIO(b""))
    _served_as_the_newest(base, WINDOWS[0], gzip.compress(raw.getvalue(), mtime=0))

    code, said = _script(base, home, WINDOWS[0])

    failing((code, said), naming="names a place outside where it is unpacked")
    truth(not (home / "escaped.exe").exists(), describing="nothing to have escaped")
    truth(not (home / TEMPORARY / "escaped.exe").exists(), describing="nothing to have escaped")


def test_an_archive_carrying_a_link_is_refused_before_anything_is_unpacked(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A link lands wherever it points, so a member that is not a plain file is refused unread."""
    home = _home(tmp_path, "home-linked")
    base = staged(WINDOWS[0])
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        link = tarfile.TarInfo("printobserver.exe")
        link.type = tarfile.SYMTYPE
        link.linkname = "../../somewhere-else.exe"
        archive.addfile(link)
    _served_as_the_newest(base, WINDOWS[0], gzip.compress(raw.getvalue(), mtime=0))

    code, said = _script(base, home, WINDOWS[0])

    failing((code, said), naming="is not a plain file")
    contains(said, "Nothing was installed", describing="what the refusal said")
    truth(not _default(home).exists(), describing="nothing to have been installed")


def test_a_destination_nothing_can_write_is_refused_naming_the_option(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A destination under a file is one nothing can make or move into; pass another.

    Which of the two steps refuses it is the host's: one PowerShell refuses to
    make the directory, another makes nothing and refuses the move. Either
    way the caller is told the same next action, and nothing was installed.
    """
    home = _home(tmp_path, "home-uncreatable")
    blocking = home / "a-file"
    blocking.write_text("", encoding="utf-8")

    code, said = _script(staged(WINDOWS[0]), home, WINDOWS[0], "-To", str(blocking / "bin"))

    failing((code, said), naming="Pass -To a directory you can write to")
    contains(said, "could not be", describing="what the refusal said")
    contains(said, "Nothing was installed", describing="what the refusal said")
    truth(blocking.is_file(), describing="the file the destination was under, left as it was")


def test_a_directory_already_on_the_path_is_left_as_it_is(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """Nothing is added to a path already carrying the directory, and the run says nothing of it."""
    home = _home(tmp_path, "home-on-path")
    environment = _environment(staged(WINDOWS[0]), home, WINDOWS[0])
    # `Path` is the variable Windows keeps its path under, and the one the
    # script reads; on a POSIX host it is a variable of the journey's own.
    environment["Path"] = os.pathsep.join([str(_default(home).parent), environment["PATH"]])

    code, said = _run(home, environment)

    passing((code, said), describing="the script installing onto a path already carrying it")
    absent(said, "was added to your PATH", describing="what the run said about the path")
    contains(_reports(_default(home)), _version(), describing="what the installed program reports")


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

    code, said = _run(home, environment)

    passing((code, said), describing=f"the install script on {here.id} itself")
    contains(_reports(_default(home)), _version(), describing="what the real program reports")


@pytest.mark.skipif(sys.platform != "win32", reason="only Windows keeps a user's own PATH")
def test_the_directory_is_on_the_users_own_path_for_the_next_window(
    staged: Callable[[str], Path], tmp_path: Path
) -> None:
    """A fresh PowerShell — the next window — finds the directory on the user's PATH.

    Read back through a new process rather than through the run's prose, and
    taken off again afterwards so the host is left as it was found.
    """
    home = _home(tmp_path, "home-persisted")
    into = home / "persisted"
    environment = _environment(staged(WINDOWS[0]), home, WINDOWS[0])
    read = "[System.Environment]::GetEnvironmentVariable('Path', 'User')"

    code, said = _run(home, environment, "-To", str(into))

    passing((code, said), describing="the install whose directory is persisted")
    try:
        fresh = shell_run([powershell(), "-NoProfile", "-Command", read], cwd=home, timeout=120)
        contains(
            (fresh.stdout or "").split(os.pathsep),
            str(into),
            describing="the user's own PATH, read by a fresh PowerShell",
        )
    finally:
        restore = (
            f"$kept = ({read} -split ';') | Where-Object {{ $_ -ne '{into}' }}; "
            "[System.Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')"
        )
        shell_run([powershell(), "-NoProfile", "-Command", restore], cwd=home, timeout=120)
