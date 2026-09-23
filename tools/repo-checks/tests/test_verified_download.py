"""The one address rule every installer downloads under, driven through all three verbs.

`--releases` is a caller's input: it exists so a suite can serve an installer a
stand-in release over loopback HTTP, and it is the one thing about an installer
a caller decides. Two of the three read the digest that vouches for an archive
out of a file served beside it, so an origin free to serve both would be an
origin free to hand an installer bytes and its own approval of them — which is
why `https` alone does not admit a host, and the producer's own forge does.

Driven through `python -m repo_checks <verb>`, which is where a caller's
`--releases` actually enters, and the verbs are asked for a release each
installer really holds so that nothing but the address decides the outcome.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal, truth
from repo_checks.gh_release import RELEASES as GH_RELEASES
from repo_checks.powershell_release import RELEASES as POWERSHELL_RELEASES
from repo_checks.release_plz_release import RELEASES as RELEASE_PLZ_RELEASES
from repo_checks.verified_download import FORGE

#: Each install verb, with a release its own installer holds digests or a
#: checksums file for, so the refusal under test is the address's alone. The
#: PowerShell verb refuses a Windows host before it reads an address at all —
#: that host already carries PowerShell — so there it would answer about the
#: host rather than about the address, and this says nothing there.
VERBS = (
    ("install-gh", "2.100.0"),
    ("install-release-plz", "0.3.167"),
    pytest.param(
        "install-powershell",
        "7.6.6",
        marks=pytest.mark.skipif(
            sys.platform == "win32", reason="the installer refuses a Windows host first"
        ),
    ),
)

#: Addresses no installer fetches, each for its own reason: a host that is not
#: the forge however well-formed its TLS is; a scheme nothing here speaks; and
#: an authority whose loopback-looking front is userinfo, so the host it names
#: is somebody else's.
REFUSED = (
    "https://example.invalid/releases/download",
    "https://github.com.example.invalid/releases/download",
    "http://example.invalid",
    "http://127.0.0.1:80@example.invalid",
    "ftp://127.0.0.1:8080",
    "file:///tmp",
    # An authority the parser itself refuses: the bracket never closes, so
    # reading the host raises rather than answering one.
    "https://[::1",
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


@pytest.mark.parametrize(
    "address", [GH_RELEASES, RELEASE_PLZ_RELEASES, POWERSHELL_RELEASES], ids=lambda a: a
)
def test_every_installers_own_address_is_one_the_rule_admits(address: str) -> None:
    """A producer's address the rule refused would be an installer that fetched nothing."""
    contains(address, f"https://{FORGE}/", describing="the address the installer downloads from")
