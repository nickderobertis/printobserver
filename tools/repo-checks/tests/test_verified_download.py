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
import io
import os
import platform
import shutil
import socket
import ssl
import sys
import tarfile
import threading
import zipfile
from collections.abc import Iterator
from pathlib import Path

import pytest
from held_toolchain import held_by_verb
from repo_checks.__main__ import main
from repo_checks.expect import absent, contains, equal, truth
from repo_checks.gh_release import RELEASES as GH_RELEASES
from repo_checks.powershell_release import RELEASES as POWERSHELL_RELEASES
from repo_checks.powershell_release import archive_for, install
from repo_checks.release_plz_release import RELEASES as RELEASE_PLZ_RELEASES
from repo_checks.shell import run
from repo_checks.verified_download import InstallerError, download, member_of, permitted, unpack
from standin_powershell import publish
from standin_release import Release, serving

#: Each install verb with the release the committed toolchain holds it at, read
#: off that toolchain so the refusal under test is the address's alone and a
#: bump carries these with it. Every installer holds `--releases` to its
#: producer before it asks which archive this host takes, so a host an
#: installer takes no archive for — gh's and PowerShell's both refuse Windows —
#: is answered about the address exactly as every other host is, and each verb
#: is driven on every host.
VERBS = tuple((held.verb, held.version) for held in held_by_verb())


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


@pytest.mark.parametrize(("verb", "version"), VERBS)
@pytest.mark.parametrize("machine", ["AMD64", "ARM64"])
@pytest.mark.parametrize("releases", REFUSED)
def test_a_verb_refuses_an_untrusted_address_before_host_archive_selection(
    verb: str,
    version: str,
    machine: str,
    releases: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A Windows caller gets the same address refusal, even without a host archive.

    gh's and PowerShell's installers take no archive for Windows, and they once
    asked which archive the host takes before reading the address — so on a
    Windows gate the refusal named the host instead of the address. This puts
    any host in the shape a Windows one reports and holds it to the address
    refusal every other host gives.
    """
    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(platform, "machine", lambda: machine)
    into = tmp_path / "bin"

    equal(main([verb, version, "--releases", releases, "--into", str(into)]), 1)

    contains(capsys.readouterr().err, "is not an address this installer downloads from")
    truth(not into.exists(), describing=f"nothing written where {verb} goes")


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


@pytest.mark.parametrize("verb", [held.verb for held in held_by_verb()])
def test_a_verb_named_no_release_is_refused_naming_what_it_needs(
    verb: str, capsys: pytest.CaptureFixture[str]
) -> None:
    """An install with no release would be one of whatever release came to hand."""
    with pytest.raises(SystemExit) as exited:
        main([verb])

    equal(exited.value.code, 2)
    contains(capsys.readouterr().err, f"{verb} needs the release to install")


@pytest.mark.parametrize(
    "address", [GH_RELEASES, RELEASE_PLZ_RELEASES, POWERSHELL_RELEASES], ids=lambda a: a
)
def test_every_installers_own_address_is_one_the_rule_admits(address: str) -> None:
    """A producer's address the rule refused would be an installer that fetched nothing."""
    truth(permitted(address), describing=f"the address rule admitting {address}")


#: The release the PowerShell installer is driven at here. A redirect an
#: install follows is proven through that installer, and one the address rule
#: refuses through `download` itself — which decides it for all three, and runs
#: on every host where PowerShell's installer refuses Windows.
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


@pytest.fixture
def local_tls(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> ssl.SSLContext:
    """A trusted local TLS listener for a release's asset-store redirect."""
    openssl = shutil.which("openssl")
    if openssl is None:
        pytest.skip("OpenSSL is needed to serve the local TLS redirect")
    config = tmp_path / "openssl.cnf"
    config.write_text(
        "[req]\nprompt=no\ndistinguished_name=dn\nx509_extensions=v3\n"
        "[dn]\nCN=127.0.0.1\n[v3]\nsubjectAltName=IP:127.0.0.1\n",
        encoding="utf-8",
    )
    cert = tmp_path / "cert.pem"
    key = tmp_path / "key.pem"
    run(
        [
            openssl,
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(key),
            "-out",
            str(cert),
            "-days",
            "1",
            "-config",
            str(config),
        ],
        check=True,
    )
    monkeypatch.setenv("SSL_CERT_FILE", str(cert))
    monkeypatch.setenv("NO_PROXY", "127.0.0.1")
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain(cert, key)
    return tls


def test_a_tls_redirect_cannot_downgrade_to_loopback_http(local_tls: ssl.SSLContext) -> None:
    """A real HTTPS response cannot send a download back to plain HTTP."""
    release = Release(files={"moved.tar.gz": b"an archive"})
    middle = Release()
    with (
        serving("/v1/", release) as base,
        serving("/v1/", middle, tls=local_tls) as secure,
    ):
        release.redirects["archive.tar.gz"] = f"{secure}/v1/archive.tar.gz"
        middle.redirects["archive.tar.gz"] = f"{base}/v1/moved.tar.gz"

        with pytest.raises(InstallerError) as raised:
            download(f"{base}/v1/archive.tar.gz")

    contains(str(raised.value), "not an address this installer follows")
    equal(middle.asked, ["/v1/archive.tar.gz"])
    absent(release.asked, "/v1/moved.tar.gz")


@pytest.mark.skipif(sys.platform == "win32", reason="the installer refuses a Windows host")
def test_a_tls_asset_redirect_installs_the_verified_program(
    serving_powershell: tuple[str, Release], tmp_path: Path, local_tls: ssl.SSLContext
) -> None:
    """A release may serve its checksum over HTTP and its verified asset over TLS."""
    base, release = serving_powershell
    archive = archive_for(PWSH, sys.platform, platform.machine())
    publish(release, archive)
    asset_store = Release(files={archive.name: release.files[archive.name]})
    with serving(f"/v{PWSH}/", asset_store, tls=local_tls) as secure:
        release.redirects[archive.name] = f"{secure}/v{PWSH}/{archive.name}"
        into = tmp_path / "bin"

        equal(main(["install-powershell", PWSH, "--releases", base, "--into", str(into)]), 0)

    contains(run([str(into / "pwsh"), "--version"], check=True).stdout, f"PowerShell {PWSH}")
    equal(asset_store.asked, [f"/v{PWSH}/{archive.name}"])


def test_a_release_redirecting_to_tls_is_followed_to_the_address_it_names() -> None:
    """A redirect to `https` is taken whatever host it names, as the forge's are.

    The forge answers a release asset with a redirect to its own asset store on
    another host, so the host of a TLS redirect cannot be held to the forge.
    Here the redirect names `https` on a loopback listener this suite owns — an
    address `--releases` itself would refuse, since over `https` it is not the
    forge — and that listener being reached is
    what says the redirect was followed rather than refused. It speaks no TLS,
    so the download then fails, and is refused as a failed download.
    """
    release = Release()
    reached = threading.Event()
    with socket.create_server(("127.0.0.1", 0)) as listener, serving("/v1/", release) as base:
        listener.settimeout(DOWNLOAD_WAIT)

        def accept_one() -> None:
            with contextlib.suppress(TimeoutError), listener.accept()[0]:
                reached.set()

        accepting = threading.Thread(target=accept_one, daemon=True)
        accepting.start()
        port = listener.getsockname()[1]
        release.redirects["archive.tar.gz"] = f"https://127.0.0.1:{port}/moved.tar.gz"

        with pytest.raises(InstallerError) as raised:
            download(f"{base}/v1/archive.tar.gz")
        accepting.join(DOWNLOAD_WAIT)

    truth(reached.is_set(), describing="the TLS address the redirect named, reached")
    contains(str(raised.value), "could not be downloaded")
    absent(str(raised.value), "not an address this installer follows")


@pytest.mark.parametrize(
    "redirected",
    ["http://example.invalid/moved.tar.gz", "https://127.0.0.1:bad/moved.tar.gz"],
)
def test_a_loopback_release_redirecting_to_untrusted_or_malformed_address_is_refused(
    redirected: str,
) -> None:
    """A loopback release cannot redirect to an untrusted or malformed address.

    Both the checksums file and the archive are fetched from the address a
    caller named, so an answer free to redirect anywhere would be an answer
    free to move either onto a scheme and a host this rule already refused.
    A redirect naming a port that is no number names no address at all, and is
    refused the same way rather than failing somewhere inside the download.
    """
    release = Release()
    with serving("/v1/", release) as base:
        release.redirects["archive.tar.gz"] = redirected

        with pytest.raises(InstallerError) as raised:
            download(f"{base}/v1/archive.tar.gz")

    contains(str(raised.value), "which is not an address this installer follows")


@pytest.mark.parametrize(
    "address",
    [
        "http://example.invalid/archive.tar.gz",
        "ftp://127.0.0.1:8080/archive.tar.gz",
        "file:///tmp/archive.tar.gz",
        "https://[::1/archive.tar.gz",
    ],
)
def test_a_download_of_an_address_the_rule_refuses_asks_nothing_of_it(address: str) -> None:
    """The rule holds `download` itself, not only the installers that call it."""
    with pytest.raises(InstallerError) as raised:
        download(address)

    contains(str(raised.value), f"{address} is not an address this installer downloads from")


def _tar(entries: dict[str, bytes | None]) -> bytes:
    """A `.tar.gz` of named files, a `None` entry being a directory of that name."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as bundle:
        for name, content in entries.items():
            info = tarfile.TarInfo(name)
            if content is None:
                info.type = tarfile.DIRTYPE
                bundle.addfile(info)
            else:
                info.size = len(content)
                bundle.addfile(info, io.BytesIO(content))
    return buffer.getvalue()


def _truncated() -> bytes:
    """A `.tar.gz` cut off inside its compressed stream, as an interrupted download is.

    The member is incompressible, so the cut lands in the deflate data rather
    than in the trailer gzip would forgive.
    """
    whole = _tar({"tool/bin/tool": os.urandom(200_000)})
    return whole[: len(whole) // 2]


def _zip(entries: dict[str, bytes]) -> bytes:
    """A `.zip` of named files."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as bundle:
        for name, content in entries.items():
            bundle.writestr(name, content)
    return buffer.getvalue()


@pytest.mark.parametrize(
    ("name", "payload"),
    [
        ("tool.tar.gz", _tar({"tool/bin/tool": b"the program"})),
        ("tool.zip", _zip({"tool/bin/tool": b"the program"})),
    ],
)
def test_a_member_is_read_out_of_either_archive_shape(name: str, payload: bytes) -> None:
    """A release publishes `.tar.gz` or `.zip`, and its name says which is read."""
    equal(member_of(payload, name=name, member="tool/bin/tool"), b"the program")


@pytest.mark.parametrize(
    ("name", "payload"),
    [
        ("tool.tar.gz", _tar({"tool/bin/tool": None})),
        ("tool.tar.gz", _tar({"README": b"no program"})),
        ("tool.zip", _zip({"README": b"no program"})),
        ("tool.zip", b"not an archive at all"),
        ("tool.tar.gz", _truncated()),
    ],
    ids=["a directory by that name", "tar without it", "zip without it", "not a zip", "truncated"],
)
def test_an_archive_without_the_member_as_a_file_is_refused_naming_it(
    name: str, payload: bytes
) -> None:
    """A directory where the program should be is no program, and says so."""
    with pytest.raises(InstallerError) as raised:
        member_of(payload, name=name, member="tool/bin/tool")

    contains(str(raised.value), f"{name} carries no tool/bin/tool")


def test_an_archive_is_unpacked_whole_into_its_own_directory(tmp_path: Path) -> None:
    """A runtime is every file its archive carries, where the archive put each."""
    into = tmp_path / "runtime"

    unpack(_tar({"pwsh": b"the program", "lib/a.dll": b"a library"}), name="rt.tar.gz", into=into)

    equal((into / "pwsh").read_bytes(), b"the program")
    equal((into / "lib" / "a.dll").read_bytes(), b"a library")


@pytest.mark.parametrize(
    "payload",
    [_tar({"../escaped": b"outside"}), b"not an archive at all", _truncated()],
    ids=["a member leaving the directory", "not a tar", "truncated"],
)
def test_an_archive_the_data_filter_or_reader_refuses_writes_nothing_above_its_directory(
    tmp_path: Path, payload: bytes
) -> None:
    """Nothing is written above the directory an archive was unpacked into."""
    into = tmp_path / "runtime"

    with pytest.raises(InstallerError) as raised:
        unpack(payload, name="rt.tar.gz", into=into)

    contains(str(raised.value), f"rt.tar.gz could not be unpacked into {into}")
    truth(not (tmp_path / "escaped").exists(), describing="nothing written above the runtime")
