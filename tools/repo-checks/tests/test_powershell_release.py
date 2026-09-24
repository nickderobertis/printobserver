"""`install-powershell`: one exact PowerShell release, verified before it is unpacked.

The release is served by a stand-in over real HTTP on loopback, laid out the way
the forge lays one out — the host's archive and the release's own
`hashes.sha256` side by side, that file encoded as PowerShell encodes it — so
what is proven is the installer's own download, digest check, extraction and
linking. `pwsh` inside the archive is a script answering `--version`, and the
link the install leaves is run to prove it reaches what the archive carried.
"""

from __future__ import annotations

import hashlib
import io
import platform
import shutil
import sys
import tarfile
from collections.abc import Iterator
from pathlib import Path

import pytest
from held_toolchain import held_by_verb
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal, refused, truth
from repo_checks.model import Repo
from repo_checks.platforms import supported
from repo_checks.powershell_release import (
    FLAVOURS,
    HASHES,
    archive_for,
    install,
    listing,
    runtime_directory,
)
from repo_checks.shell import run
from repo_checks.verified_download import InstallerError
from standin_powershell import RUNTIME_FILE, archive_bytes, program
from standin_powershell import publish as _publish
from standin_release import Release, serving
from treecopy import REPO_ROOT

VERSION = next(held.version for held in held_by_verb() if held.command == "pwsh")


def _escaping_archive() -> bytes:
    """An archive whose one member names a path above where it is unpacked."""
    buffer = io.BytesIO()
    payload = b"a file nobody asked this archive to write\n"
    with tarfile.open(fileobj=buffer, mode="w:gz") as bundle:
        info = tarfile.TarInfo("../escaped")
        info.size = len(payload)
        bundle.addfile(info, io.BytesIO(payload))
    return buffer.getvalue()


@pytest.fixture
def serving_release() -> Iterator[tuple[str, Release]]:
    """A loopback server answering the release's files under `/v<version>/`."""
    release = Release()
    with serving(f"/v{VERSION}/", release) as base:
        yield base, release


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_installs_a_verified_release_and_the_link_reaches_its_program(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """`python -m repo_checks install-powershell` against a stand-in, end to end."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)

    runtime = runtime_directory(into, VERSION)
    truth((runtime / RUNTIME_FILE).is_file(), describing="the whole archive unpacked")
    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    equal(
        release.asked,
        [f"/v{VERSION}/{HASHES}", f"/v{VERSION}/{archive.name}"],
        describing="what the installer downloaded, the hashes file first",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_refuses_a_tampered_archive_before_anything_is_unpacked(
    serving_release: tuple[str, Release], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """An archive the release's own hashes do not vouch for is never unpacked."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive, tampered=True)
    into = tmp_path / "bin"

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 1)

    said = capsys.readouterr().err
    contains(said, "install-powershell: ")
    contains(said, "nothing was installed")
    truth(not into.exists(), describing="nothing written where pwsh goes")
    truth(
        not runtime_directory(into, VERSION).exists(),
        describing="nothing unpacked where the runtime goes",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_an_archive_carrying_no_program_leaves_the_runtime_a_good_install_left(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """The archive is unpacked beside the runtime and moved onto it, never into it."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    install(archive, into, releases=base)

    broken = archive_bytes(carrying=None)
    release.files[archive.name] = broken
    release.files[HASHES] = (f"{hashlib.sha256(broken).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )
    with pytest.raises(InstallerError):
        install(archive, into, releases=base)

    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_an_unclearable_staging_directory_cannot_supply_a_missing_program(
    serving_release: tuple[str, Release],
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """An old staging program cannot validate a newly downloaded archive without one."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)
    capsys.readouterr()
    runtime = runtime_directory(into, VERSION)
    staged = runtime.with_name(f".{runtime.name}.part")
    staged.mkdir()
    (staged / "pwsh").write_bytes(program("stale"))
    staged.chmod(0o500)
    broken = archive_bytes(carrying=None)
    release.files[archive.name] = broken
    release.files[HASHES] = (f"{hashlib.sha256(broken).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )

    try:
        equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 1)
        contains(capsys.readouterr().err, "staging directory could not be cleared")
        truth((staged / "pwsh").is_file(), describing="the old program was never trusted")
        contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    finally:
        staged.chmod(0o700)


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_replacement_the_filesystem_refuses_leaves_the_runtime_that_was_there(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """A commit that cannot be made is a refusal naming where the working runtime is.

    The name the installer puts a superseded runtime aside under is occupied
    here by a regular file, which is a directory rename the filesystem refuses
    — the one way a commit fails that a test can produce rather than pretend.
    What it proves is that the failure leaves the runtime the first install put
    there untouched and still runnable, and comes back as a refusal a caller
    reads rather than as a traceback out of the middle of a replacement.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    install(archive, into, releases=base)
    runtime = runtime_directory(into, VERSION)
    runtime.with_name(f".{runtime.name}.superseded").write_text("in the way", encoding="utf-8")

    with pytest.raises(InstallerError) as raised:
        install(archive, into, releases=base)

    contains(str(raised.value), f"{runtime} could not be replaced")
    contains(str(raised.value), "Nothing was deleted: the runtime this was replacing is at")
    contains(str(raised.value), f"`pwsh` is linked to {runtime}")
    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    truth(
        not runtime.with_name(f".{runtime.name}.part").exists(),
        describing="the staged runtime, cleared by the refusal",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_link_the_filesystem_refuses_after_the_runtime_was_replaced_is_a_refusal(
    serving_release: tuple[str, Release],
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The runtime is in place and the link is not: the verb says which, and where.

    The name the link is staged under is occupied here by a directory, which a
    file unlink refuses — so the second install replaces the runtime the first
    left and then cannot commit the link onto it. That comes back as the verb's
    own refusal naming the runtime it did install, rather than as a traceback
    after a replacement, and the link the first install left still runs.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    install(archive, into, releases=base)
    (into / ".pwsh.part").mkdir()
    runtime = runtime_directory(into, VERSION)

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 1)

    said = capsys.readouterr().err
    contains(said, f"install-powershell: PowerShell {VERSION} is installed at {runtime}")
    contains(said, f"`pwsh` could not be linked into {into}")
    contains(said, f"Link {runtime / 'pwsh'} there yourself")
    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_pointed_at_a_relative_directory_leaves_a_link_that_runs(
    serving_release: tuple[str, Release], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """`--into bin` is a directory a caller names from where they stand, and it works.

    The runtime goes BESIDE the directory the program is linked into, so a
    relative directory taken as written would leave `bin/pwsh` pointing at
    `share/powershell-<release>/pwsh` resolved from `bin/` — a link under
    itself, where nothing is, and a verb that exits zero having installed a
    program nobody can run.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    monkeypatch.chdir(tmp_path)

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", "bin"]), 0)

    linked = tmp_path / "bin" / "pwsh"
    truth(linked.is_symlink(), describing="the link the install left")
    truth(linked.resolve().is_file(), describing=f"what it points at: {linked.readlink()}")
    contains(run([str(linked), "--version"], check=True).stdout, f"PowerShell {VERSION}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_the_verb_says_what_it_installed_and_that_its_directory_is_off_path(
    serving_release: tuple[str, Release],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A directory off PATH is an install that worked and a program nobody can run."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"
    monkeypatch.setenv("PATH", str(tmp_path / "elsewhere"))

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)

    said = capsys.readouterr().err
    contains(said, f"installed PowerShell {VERSION} at {into / 'pwsh'}")
    contains(said, f"{into} is not on PATH")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_second_install_replaces_the_runtime_and_the_link_the_first_one_left(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """Installing again over a runtime already there leaves one runtime and one link."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    install(archive, into, releases=base)

    replacing = archive_bytes(carrying=program("7.6.6 (the second install)"), runtime=b"newer\n")
    release.files[archive.name] = replacing
    release.files[HASHES] = (f"{hashlib.sha256(replacing).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )
    linked = install(archive, into, releases=base)

    runtime = runtime_directory(into, VERSION)
    contains(run([str(linked), "--version"], check=True).stdout, "the second install")
    equal((runtime / RUNTIME_FILE).read_bytes(), b"newer\n", describing="the unpacked runtime")
    equal(
        sorted(path.name for path in runtime.parent.iterdir()),
        [runtime.name],
        describing="what the two installs left beside the runtime",
    )
    equal(sorted(path.name for path in into.iterdir()), ["pwsh"], describing="the link directory")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_an_archive_whose_member_leaves_the_directory_is_unpacked_nowhere(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """A digest vouches for bytes; the archive inside them is still read as untrusted.

    The member here names a path above the directory it is unpacked into, which
    is how an archive writes over a file nobody asked it to touch.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    escaping = _escaping_archive()
    release.files[archive.name] = escaping
    release.files[HASHES] = (f"{hashlib.sha256(escaping).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )
    into = tmp_path / "bin"

    with pytest.raises(InstallerError) as raised:
        install(archive, into, releases=base)

    above = runtime_directory(into, VERSION).parent
    contains(str(raised.value), "could not be unpacked")
    truth(not (above / "escaped").exists(), describing="nothing written above the directory")
    truth(not into.exists(), describing="nothing written where pwsh goes")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_release_listing_no_checksum_for_the_archive_is_refused(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """An archive the hashes file does not name is one nothing vouches for."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    release.files[HASHES] = f"{'0' * 64} *something-else.tar.gz\n".encode("utf-16")

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"the release's {HASHES} lists no checksum for {archive.name}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_release_the_forge_does_not_serve_is_refused(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """A download that fails says which address it was."""
    base, _ = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), f"{HASHES} could not be downloaded")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
@pytest.mark.parametrize(
    ("published", "said"),
    [
        (b"not a tar.gz at all", "could not be unpacked"),
        (archive_bytes(carrying=None), "carries no pwsh"),
    ],
    ids=["unreadable", "no-program"],
)
def test_an_archive_that_is_not_a_powershell_is_refused_naming_what_it_lacks(
    serving_release: tuple[str, Release], tmp_path: Path, published: bytes, said: str
) -> None:
    """A digest vouches for bytes, not for what they are: the unpacking says the rest."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    release.files[archive.name] = published
    release.files[HASHES] = (f"{hashlib.sha256(published).hexdigest()} *{archive.name}\n").encode(
        "utf-16"
    )

    with pytest.raises(InstallerError) as raised:
        install(archive, tmp_path / "bin", releases=base)

    contains(str(raised.value), said)
    truth(not (tmp_path / "bin").exists(), describing="nothing written where pwsh goes")
    truth(
        not runtime_directory(tmp_path / "bin", VERSION).exists(),
        describing="no runtime left where one would go",
    )
    equal(
        sorted(path.name for path in (tmp_path / "share").iterdir())
        if (tmp_path / "share").exists()
        else [],
        [],
        describing="what the refused install left beside the runtime",
    )


def test_a_windows_host_is_told_it_carries_powershell_already() -> None:
    """There is nothing to install where `powershell` is part of the operating system."""
    with pytest.raises(InstallerError) as raised:
        archive_for(VERSION, "win32", "AMD64")

    refused([str(raised.value)], "a Windows host carries PowerShell already")


@pytest.mark.parametrize("machine", ["AMD64", "ARM64"])
def test_the_verb_on_a_windows_host_installs_nothing_and_says_why(
    machine: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Pointed at the producer's own release, a Windows host is refused before any download.

    The address is the one the verb takes by default, so the refusal is the
    host's: nothing is fetched, and nothing reaches the directory it was given.
    """
    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(platform, "machine", lambda: machine)
    into = tmp_path / "bin"

    equal(main(["install-powershell", VERSION, "--into", str(into)]), 1)

    contains(capsys.readouterr().err, "install-powershell: a Windows host carries PowerShell")
    truth(not into.exists(), describing="nothing written where pwsh goes")


@pytest.mark.parametrize(("system", "machine"), [("linux", "riscv64"), ("freebsd14", "amd64")])
def test_a_host_the_installer_does_not_know_is_refused_by_name(system: str, machine: str) -> None:
    """Rather than guess at an archive, the installer names the host it cannot serve."""
    with pytest.raises(InstallerError) as raised:
        archive_for(VERSION, system, machine)

    refused([str(raised.value)], f"this host is {system} on {machine}")


def test_a_version_that_is_not_a_release_is_refused() -> None:
    """The release reaches a URL, so it is read as a release or not at all."""
    with pytest.raises(InstallerError) as raised:
        archive_for("lts", "linux", "x86_64")

    contains(str(raised.value), "`lts` is not a release")


@pytest.mark.parametrize(
    ("system", "machine", "named"),
    [
        ("linux", "x86_64", "linux-x64"),
        ("linux", "aarch64", "linux-arm64"),
        ("darwin", "arm64", "osx-arm64"),
    ],
)
def test_a_host_it_takes_is_answered_the_archive_its_release_names(
    system: str, machine: str, named: str
) -> None:
    """The archive's name is the one `hashes.sha256` lists and the release serves."""
    equal(archive_for(VERSION, system, machine).name, f"powershell-{VERSION}-{named}.tar.gz")


def test_the_runtime_sits_beside_the_directory_its_program_is_linked_into(tmp_path: Path) -> None:
    """`~/.local/bin/pwsh` reaches `~/.local/share/powershell-<release>/pwsh`."""
    equal(
        runtime_directory(tmp_path / "bin", VERSION),
        tmp_path / "share" / f"powershell-{VERSION}",
    )


def test_the_hashes_file_is_read_as_its_producer_encoded_it() -> None:
    """Read as UTF-8, PowerShell's UTF-16 hashes file lists nothing at all."""
    line = f"{'a' * 64} *powershell-{VERSION}-linux-x64.tar.gz"

    equal(listing(line.encode("utf-16")).strip(), line)
    equal(listing(line.encode("utf-8")).strip(), line, describing="a producer that writes UTF-8")


def test_every_supported_platform_but_windows_has_a_flavour() -> None:
    """A platform with no flavour is a development host the PowerShell journeys skip on.

    `AGENTS.md`'s supported-platform list is the one source of which platforms
    this repository has, and this installer restates the producer's name for
    each of them. A platform the list gains and this does not would leave that
    host with no `pwsh` and the three PowerShell journeys silently skipped
    there, so every one the list names has a flavour here — except the Windows
    ones, which are absent by design, carry PowerShell already, and are what
    `archive_for` refuses by name. That exception is asserted too, so a Windows
    entry appearing here is refused rather than quietly installed over the
    host's own PowerShell.

    A flavour for a host the list does NOT name is deliberately left alone:
    `repo-policy.toml` retired `macos-x86_64` as a CI platform for runner cost
    rather than because nobody develops there, and a developer on one still
    runs these journeys.
    """
    committed = Repo(REPO_ROOT)
    named = {entry.id for entry in supported(committed) if not entry.id.startswith("windows-")}
    flavoured = {
        f"{'macos' if system == 'darwin' else system}-{machine}" for system, machine in FLAVOURS
    }

    equal(
        sorted(flavoured & named),
        sorted(named),
        describing="the supported platforms this installer takes PowerShell for",
    )
    equal(
        sorted(entry.id for entry in supported(committed) if entry.id.startswith("windows-")),
        sorted(entry.id for entry in supported(committed) if entry.id not in flavoured),
        describing="the platforms with no flavour: Windows, which carries PowerShell already",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_an_install_interrupted_mid_commit_is_recovered_by_the_next_one(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """The state a commit stopped part-way leaves, run forward rather than argued about.

    Between the two renames a replacement is made of, the runtime already
    installed is at the aside name and the name `pwsh` is linked to holds
    nothing — a machine whose install was killed there has a broken link and a
    runtime under a dotted name. No test can make a rename between two entries
    of one directory fail, so that state is not produced here by breaking one;
    it is put on disk directly, exactly as an interrupted install leaves it, and
    what is proven is the thing an operator needs: the next install comes back
    over it, leaving one runtime, one link and nothing dotted beside it.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    into = tmp_path / "bin"
    _publish(release, archive)
    install(archive, into, releases=base)
    runtime = runtime_directory(into, VERSION)
    runtime.replace(runtime.with_name(f".{runtime.name}.superseded"))
    truth(not (into / "pwsh").resolve().exists(), describing="the link an interruption leaves")

    install(archive, into, releases=base)

    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    equal(
        sorted(path.name for path in runtime.parent.iterdir()),
        [runtime.name],
        describing="what the recovering install left beside the runtime",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_blocked_recovery_keeps_the_saved_runtime(
    serving_release: tuple[str, Release], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A saved runtime survives when another entry blocks its original name."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"
    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)
    capsys.readouterr()
    runtime = runtime_directory(into, VERSION)
    saved = runtime.with_name(f".{runtime.name}.superseded")
    runtime.replace(saved)
    runtime.write_text("another entry", encoding="utf-8")

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 1)

    contains(capsys.readouterr().err, str(saved))
    contains(run([str(saved / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    equal(runtime.read_text(encoding="utf-8"), "another entry")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_saved_runtime_blocked_by_a_dangling_link_at_its_name_stays_saved_and_named(
    serving_release: tuple[str, Release], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A link to nothing at the runtime's name reads as vacant, and still blocks the move back.

    An interrupted install leaves the runtime under its aside name; if the name
    it came from is then a dangling link — somebody's hand-made link to a
    runtime they since removed — `exists` answers false for it, so the next
    install tries to move the saved runtime back. `rename(2)` moves a directory
    only onto a name that is absent or an empty directory, and refuses any other
    entry there with `ENOTDIR`, a link included whether or not it resolves. So
    the refusal carries that error and names where the saved runtime still is,
    and that runtime still runs.
    """
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"
    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)
    capsys.readouterr()
    runtime = runtime_directory(into, VERSION)
    saved = runtime.with_name(f".{runtime.name}.superseded")
    runtime.replace(saved)
    runtime.symlink_to(tmp_path / "a runtime somebody removed")

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 1)

    said = capsys.readouterr().err
    contains(said, f"the saved PowerShell runtime at {saved} could not be restored to {runtime}")
    contains(said, "Not a directory", describing="the refusal `rename(2)` gives, ENOTDIR")
    truth(runtime.is_symlink(), describing="the dangling link, left where it was")
    contains(run([str(saved / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    truth(
        not runtime.with_name(f".{runtime.name}.part").exists(),
        describing="the staged runtime, cleared",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_saved_runtime_beside_a_working_one_is_cleared_after_the_next_install(
    serving_release: tuple[str, Release], tmp_path: Path
) -> None:
    """A completed swap interrupted before cleanup leaves an old saved copy."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"
    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)
    runtime = runtime_directory(into, VERSION)
    saved = runtime.with_name(f".{runtime.name}.superseded")
    shutil.copytree(runtime, saved)

    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)

    truth(not saved.exists(), describing="the stale saved runtime is cleared")
    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_an_unclearable_saved_runtime_refuses_without_replacing_the_working_one(
    serving_release: tuple[str, Release], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A filesystem refusal while clearing an old saved copy keeps pwsh runnable."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"
    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)
    capsys.readouterr()
    runtime = runtime_directory(into, VERSION)
    saved = runtime.with_name(f".{runtime.name}.superseded")
    shutil.copytree(runtime, saved)
    saved.chmod(0o500)

    try:
        equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 1)
        contains(capsys.readouterr().err, "saved PowerShell runtime could not be cleared")
        contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
        truth(saved.exists(), describing="the saved copy remains for manual recovery")
        truth(
            not runtime.with_name(f".{runtime.name}.part").exists(),
            describing="the uncommitted replacement is cleared",
        )
    finally:
        saved.chmod(0o700)


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_saved_runtime_that_cannot_be_removed_after_swap_is_reported(
    serving_release: tuple[str, Release], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A completed swap can leave a protected old copy without losing the new pwsh."""
    base, release = serving_release
    archive = archive_for(VERSION, sys.platform, platform.machine())
    _publish(release, archive)
    into = tmp_path / "bin"
    equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)
    capsys.readouterr()
    runtime = runtime_directory(into, VERSION)
    # Protect a directory inside the runtime rather than the runtime itself: macOS
    # refuses to rename a directory its owner cannot write, which would stop the
    # swap before the removal this test is about. Linux allows that rename.
    protected = runtime / "protected"
    protected.mkdir()
    (protected / "held").write_text("held\n", encoding="utf-8")
    protected.chmod(0o500)
    saved = runtime.with_name(f".{runtime.name}.superseded")

    try:
        equal(main(["install-powershell", VERSION, "--releases", base, "--into", str(into)]), 0)
        contains(capsys.readouterr().err, "old PowerShell runtime could not be cleared")
        truth(saved.exists(), describing="the protected old copy remains")
        contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {VERSION}")
    finally:
        for held in (protected, saved / "protected"):
            if held.exists():
                held.chmod(0o700)
