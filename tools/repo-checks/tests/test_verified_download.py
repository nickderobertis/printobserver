"""The one address rule every installer downloads under, driven through all three verbs.

`--releases` is a caller's input: it exists so a suite can serve an installer a
stand-in release over loopback HTTP, and it is the one thing about an installer
a caller decides. Two of the three read the digest that vouches for an archive
out of a file served beside it, so an origin free to serve both would be an
origin free to hand an installer bytes and its own approval of them — which is
why `https` alone does not admit a host, nor the forge alone a repository: a
caller may name the producer's own release address, or a stand-in on loopback.

Driven through `python -m repo_checks <verb>`, which is where a caller's
`--releases` actually enters, and the verbs are asked for a release each
installer really holds so that nothing but the address decides the outcome.
"""

from __future__ import annotations

import contextlib
import platform
import socket
import sys
import threading
from collections.abc import Iterator
from pathlib import Path

import pytest
from held_toolchain import held_by_verb
from repo_checks.__main__ import main
from repo_checks.expect import absent, contains, equal, truth
from repo_checks.gh_release import RELEASES as GH_RELEASES
from repo_checks.gh_release import archive_for as gh_archive_for
from repo_checks.powershell_release import RELEASES as POWERSHELL_RELEASES
from repo_checks.powershell_release import archive_for, install
from repo_checks.release_plz_release import RELEASES as RELEASE_PLZ_RELEASES
from repo_checks.release_plz_release import archive_for as release_plz_archive_for
from repo_checks.shell import run
from repo_checks.verified_download import InstallerError, permitted
from standin_powershell import publish
from standin_release import Release, serving

#: Each installer's own reading of a host, by the verb that drives it.
ARCHIVE_FOR = {
    "install-gh": gh_archive_for,
    "install-powershell": archive_for,
    "install-release-plz": release_plz_archive_for,
}


def _serves_this_host(verb: str, version: str) -> bool:
    """Whether `verb`'s installer takes this host, asked of that installer itself."""
    try:
        ARCHIVE_FOR[verb](version, sys.platform, platform.machine())
    except InstallerError:
        return False
    return True


#: Each install verb with the release the committed toolchain holds it at, read
#: off that toolchain so the refusal under test is the address's alone and a
#: bump carries these with it. An installer refuses a host it takes no archive
#: for — gh's and PowerShell's both refuse Windows — before it reads an address
#: at all, so there it would answer about the host rather than about the
#: address, and this says nothing there.
VERBS = tuple(
    pytest.param(
        held.verb,
        held.version,
        marks=pytest.mark.skipif(
            not _serves_this_host(held.verb, held.version),
            reason="the installer refuses this host before it reads an address",
        ),
    )
    for held in held_by_verb()
)


#: Addresses no installer fetches, each for its own reason: a host that is not
#: the forge however well-formed its TLS is; a repository on the forge that is
#: not the producer's, whose release could carry an archive and the checksums
#: approving it together; a scheme nothing here speaks; and an authority whose
#: loopback-looking front is userinfo, so the host it names is somebody else's.
REFUSED = (
    "https://example.invalid/releases/download",
    "https://github.com.example.invalid/releases/download",
    "https://github.com/someone-else/fork/releases/download",
    "http://example.invalid",
    "http://127.0.0.1:80@example.invalid",
    "ftp://127.0.0.1:8080",
    "file:///tmp",
    # Authorities the parser itself refuses: the bracket never closes, so
    # reading the host raises rather than answering one; and the forge's own
    # host with a port that is no number, which only reading the port refuses.
    "https://[::1",
    "https://github.com:bad/releases/download",
)


@pytest.mark.parametrize(("verb", "version"), VERBS)
@pytest.mark.parametrize("releases", REFUSED)
def test_a_verb_pointed_at_an_address_no_installer_fetches_installs_nothing(
    verb: str,
    version: str,
    releases: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The refusal names the address, and nothing reaches the directory it was given."""
    into = tmp_path / verb / "bin"

    equal(main([verb, version, "--releases", releases, "--into", str(into)]), 1)

    contains(capsys.readouterr().err, "is not an address this installer downloads from")
    truth(not into.exists(), describing=f"nothing written where {verb} puts a program")


def test_gh_refuses_an_untrusted_address_even_on_a_host_without_an_archive(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The public verb checks its caller's address before its host's archive support."""
    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(platform, "machine", lambda: "AMD64")
    into = tmp_path / "bin"

    equal(
        main(
            ["install-gh", "2.100.0", "--releases", "https://example.invalid", "--into", str(into)]
        ),
        1,
    )

    contains(capsys.readouterr().err, "is not an address this installer downloads from")
    truth(not into.exists(), describing="nothing written where gh goes")


@pytest.mark.parametrize("verb", [held.verb for held in held_by_verb()])
@pytest.mark.parametrize("option", ["--releases", "--into"])
def test_explicit_empty_install_options_are_refused_before_any_install(
    verb: str,
    option: str,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """An empty caller value must not turn into an implicit trusted default."""
    with pytest.raises(SystemExit) as exited:
        main([verb, "latest", option, ""])

    equal(exited.value.code, 2)
    contains(capsys.readouterr().err, f"{option} needs a nonempty value")


@pytest.mark.parametrize(
    "address", [GH_RELEASES, RELEASE_PLZ_RELEASES, POWERSHELL_RELEASES], ids=lambda a: a
)
def test_every_installers_own_address_is_one_the_rule_admits(address: str) -> None:
    """A producer's address the rule refused would be an installer that fetched nothing."""
    truth(permitted(address), describing=f"the address rule admitting {address}")


#: The release the PowerShell installer is driven at here. Its stand-in is the
#: one every suite shares, which is why the redirect is proven through that
#: installer: what a redirect happens to, `download` decides for all three.
PWSH = next(held.version for held in held_by_verb() if held.command == "pwsh")

#: How long, in seconds, a suite waits on a loopback listener an installer is
#: redirected to: far longer than a connect over loopback takes.
DOWNLOAD_WAIT = 30


@pytest.fixture
def serving_powershell() -> Iterator[tuple[str, Release]]:
    """A loopback release laid out as the PowerShell installer reads one."""
    release = Release()
    with serving(f"/v{PWSH}/", release) as base:
        yield base, release


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_release_redirecting_its_archive_elsewhere_on_loopback_is_still_installed(
    serving_powershell: tuple[str, Release], tmp_path: Path
) -> None:
    """A redirect is how a real release asset is answered, so one must be followed.

    The forge answers a release asset with a `302` to its own asset store, and
    a download refusing every redirect would fetch nothing at all from a real
    release. Here the stand-in answers the archive that way, naming itself
    under another name, and what is installed is the program that name carries.
    """
    base, release = serving_powershell
    archive = archive_for(PWSH, sys.platform, platform.machine())
    publish(release, archive)
    release.files["moved.tar.gz"] = release.files[archive.name]
    release.redirects[archive.name] = f"{base}/v{PWSH}/moved.tar.gz"
    into = tmp_path / "bin"

    install(archive, into, releases=base)

    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {PWSH}")
    contains(
        release.asked,
        f"/v{PWSH}/moved.tar.gz",
        describing="the address the redirect was followed to",
    )


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_release_redirecting_to_tls_is_followed_to_the_address_it_names(
    serving_powershell: tuple[str, Release], tmp_path: Path
) -> None:
    """A redirect to `https` is taken whatever host it names, as the forge's are.

    The forge answers a release asset with a redirect to its own asset store on
    another host, so the host of a TLS redirect cannot be held to the forge.
    Here the redirect names `https` on a loopback listener this suite owns — an
    address `--releases` itself would refuse, since over `https` it is not the
    forge — and that listener being reached is
    what says the redirect was followed rather than refused. It speaks no TLS,
    so the download then fails, and the install is refused as a failed download
    with nothing written.
    """
    base, release = serving_powershell
    archive = archive_for(PWSH, sys.platform, platform.machine())
    publish(release, archive)
    reached = threading.Event()
    with socket.create_server(("127.0.0.1", 0)) as listener:
        listener.settimeout(DOWNLOAD_WAIT)

        def accept_one() -> None:
            with contextlib.suppress(TimeoutError), listener.accept()[0]:
                reached.set()

        accepting = threading.Thread(target=accept_one, daemon=True)
        accepting.start()
        port = listener.getsockname()[1]
        release.redirects[archive.name] = f"https://127.0.0.1:{port}/moved.tar.gz"
        into = tmp_path / "bin"

        with pytest.raises(InstallerError) as raised:
            install(archive, into, releases=base)
        accepting.join(DOWNLOAD_WAIT)

    truth(reached.is_set(), describing="the TLS address the redirect named, reached")
    contains(str(raised.value), "could not be downloaded")
    absent(str(raised.value), "not an address this installer follows")
    truth(not into.exists(), describing="nothing written where the program goes")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
@pytest.mark.parametrize(
    "redirected",
    ["http://example.invalid/moved.tar.gz", "https://127.0.0.1:bad/moved.tar.gz"],
)
def test_a_release_redirecting_off_tls_or_to_no_address_is_refused_and_installs_nothing(
    serving_powershell: tuple[str, Release], tmp_path: Path, redirected: str
) -> None:
    """A download that began over TLS is not walked off it by whatever answered.

    Both the checksums file and the archive are fetched from the address a
    caller named, so an answer free to redirect anywhere would be an answer
    free to move either onto a scheme and a host this rule already refused.
    A redirect naming a port that is no number names no address at all, and is
    refused the same way rather than failing somewhere inside the download.
    """
    base, release = serving_powershell
    archive = archive_for(PWSH, sys.platform, platform.machine())
    publish(release, archive)
    release.redirects[archive.name] = redirected
    into = tmp_path / "bin"

    with pytest.raises(InstallerError) as raised:
        install(archive, into, releases=base)

    contains(str(raised.value), "which is not an address this installer follows")
    truth(not into.exists(), describing="nothing written where the program goes")
