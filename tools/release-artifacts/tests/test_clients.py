"""Each client is installed the way its own consumers install it, and proved there.

The three routes are proved beside these in `test_taking.py`; these are the
three clients a dependent takes as a dependency. Each is built from the
committed tree, installed into a throwaway environment holding no copy of these
sources — a virtual environment, a package directory, an unpacked crate — and
then proved there by its own committed smoke check against a **real
supervisor**: the program this repository builds, over a stand-in machine, with
a print and an image opened through the supervisor's own ingress.

The supervisor these run against is the debug build rather than the one a
release carries, and deliberately: what a client's smoke check proves is the
client, and the program on the other side of the socket is the same program
either way. What proves the *published* program is the route journeys, which
carry it.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
from release_artifacts.installing import prove
from repo_checks.expect import contains, passing
from repo_checks.model import Repo
from repo_checks.shell import run

#: The three clients a dependent takes as a dependency.
CLIENTS = ["crate:printobserver-sdk", "pypi:printobserver-sdk", "npm:@printobserver/sdk"]

#: How long the one program build these share is given.
BUILD_TIMEOUT_SECONDS = 2400


@pytest.fixture(scope="module")
def supervisor(request: pytest.FixtureRequest) -> Path:
    """The program a client's smoke check is proved against, built once."""
    repo = Repo(Path(__file__).resolve().parents[3])
    built = repo.root / "target" / "debug" / "printobserver"
    if not built.is_file():
        passing(
            run(
                ["cargo", "build", "--locked", "-p", "printobserver"],
                cwd=repo.root,
                timeout=BUILD_TIMEOUT_SECONDS,
            ),
            describing="building the supervisor these clients are proved against",
        )
    return built


@pytest.mark.parametrize("identifier", CLIENTS)
def test_each_client_is_installed_and_proved_against_a_real_supervisor(
    identifier: str,
    repo: Repo,
    supervisor: Path,
    into: Callable[[str], Path],
) -> None:
    """Its own smoke check runs where it was installed, and reaches a real server.

    The check reads a status, materializes an image, opens the file at the path
    the server answered and checks its bytes against the digest the record
    declares. A check that reached no server would say nothing about the
    artifact.
    """
    said = prove(repo, identifier, into(identifier.replace(":", "-").replace("@", "")), supervisor)

    contains(said, "smoke: contract", describing=f"what `{identifier}` said where it was put")
    contains(said, "image ", describing=f"what `{identifier}` said where it was put")


def test_a_client_with_no_smoke_check_is_refused(
    repo: Repo, supervisor: Path, into: Callable[[str], Path], monkeypatch: pytest.MonkeyPatch
) -> None:
    """An artifact nothing proves where it was installed is a stop, not a pass."""
    from release_artifacts import installing

    monkeypatch.delitem(installing.SMOKE, "pypi:printobserver-sdk")

    with pytest.raises(installing.InstallError, match="has no committed smoke check"):
        prove(repo, "pypi:printobserver-sdk", into("no-smoke-check"), supervisor)
