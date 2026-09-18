"""Proving each client against a registry, driven against registries stood up here.

The three routes are proven from their registries in `test_registries.py`;
these are the three clients a dependent takes as a dependency, each taken from
the registry that serves it — a crate registry with a real sparse index, a
Python index serving a real wheel, a JavaScript registry serving a real package
— with the same `cargo`, `pip` and `npm` a dependent runs, and proven where it
was installed by its own committed smoke check against a **real supervisor**:
the program the forge's release asset carries, taken by the install-script
route at the same version, so nothing a proof reaches is a build of the tree.

Every direction is falsified: a registry serving nothing for the version under
test does not pass and says it was not served, a forge serving no release for
the supervisor to come from does not pass and names the forge, a registry
serving a client that installs and cannot be used does not pass and says so
distinctly, and one serving the real client passes.
"""

from __future__ import annotations

from collections.abc import Callable, Iterator
from pathlib import Path

import pytest
from release_artifacts.registries import (
    PRINTOBSERVER_PROOF_REGISTRIES,
    PRINTOBSERVER_PROOF_VERSION,
    Outcome,
    Proof,
    RegistryError,
    clients,
    prove,
)
from release_artifacts.standin import Registries
from release_artifacts.targets import declared
from repo_checks.expect import contains, equal, passing
from repo_checks.model import Repo
from repo_checks.shell import run

#: The three clients a dependent takes as a dependency, each from its registry.
CLIENTS = ["crate:printobserver-sdk", "pypi:printobserver-sdk", "npm:@printobserver/sdk"]

#: How long the one program build these share is given.
BUILD_TIMEOUT_SECONDS = 2400


@pytest.fixture(scope="module")
def supervisor(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """The real program the stand-in forge's release carries, built once.

    The debug build rather than a release one: what the client's proof is
    about is the client, and the program on the other side of the socket is
    the same program either way. Stripped, in a copy, because every artifact
    the stand-in serves carries the program's bytes compressed, and a debug
    build's are mostly debug information nothing here reads.
    """
    repo = Repo(Path(__file__).resolve().parents[3])
    built = repo.root / "target" / "debug" / "printobserver"
    if not built.is_file():
        passing(
            run(
                ["cargo", "build", "--locked", "-p", "printobserver"],
                cwd=repo.root,
                timeout=BUILD_TIMEOUT_SECONDS,
            ),
            describing="building the supervisor the release stands in with",
        )
    stripped = tmp_path_factory.mktemp("supervisor") / "printobserver"
    passing(
        run(["strip", "-o", str(stripped), str(built)], cwd=repo.root, timeout=300),
        describing="stripping the supervisor of its debug information",
    )
    return stripped


# llmlint: ignore[test_tiers_split_by_project_not_by_marker] suppressions.toml has the reason.
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] The same reason as above it.
@pytest.fixture
def registries(repo: Repo, tmp_path: Path) -> Iterator[Registries]:
    """The registries, answering on one address, serving nothing yet."""
    standing_in = Registries(repo, tmp_path / "served")
    try:
        yield standing_in
    finally:
        standing_in.stop()


@pytest.fixture
def proving(repo: Repo, registries: Registries, tmp_path: Path) -> Callable[..., Proof]:
    """Take one client from the stand-in registries and prove what they served."""

    def prove_client(identifier: str, wanted: str = "") -> Proof:
        into = tmp_path / identifier.replace(":", "-").replace("/", "-") / (wanted or "newest")
        return prove(
            repo,
            identifier,
            into,
            {
                PRINTOBSERVER_PROOF_REGISTRIES: registries.base,
                PRINTOBSERVER_PROOF_VERSION: wanted,
            },
        )

    return prove_client


@pytest.mark.parametrize("identifier", CLIENTS)
def test_a_registry_serving_the_client_is_a_pass_against_the_releases_own_supervisor(
    identifier: str,
    version: str,
    supervisor: Path,
    registries: Registries,
    proving: Callable[..., Proof],
) -> None:
    """The client the registry serves is installed, and its smoke check reaches a real server.

    The server is the program the forge's release carries, taken by the
    install script; the check reads a status, materializes an image and opens
    the file the server answered — which a check that reached no server, or
    reached a build of this tree, would say nothing about.
    """
    registries.serve(version, program=supervisor)
    registries.serve_clients(identifier)

    proof = proving(identifier)

    equal(proof.outcome, Outcome.PROVEN, describing=f"the proof of `{identifier}`:\n{proof.report}")
    equal(proof.exit_status, 0, describing="the exit a pass answers with")
    contains(proof.report, "smoke: contract", describing="what the installed client said")
    contains(proof.report, version, describing="the version proven")
    contains(proof.report, "release's own supervisor", describing="what the check ran against")
    contains(
        " ".join(registries.asked),
        "/forge/releases/download/v",
        describing="the release the supervisor was taken from",
    )


@pytest.mark.parametrize("identifier", CLIENTS)
def test_a_registry_serving_nothing_for_the_client_is_not_served(
    identifier: str, version: str, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """A publish that did not happen is reported as that rather than as a broken client.

    The release is served with the stand-in's own program: the client's
    registry is asked before anything is taken, and it is that answer which
    stops the run.
    """
    registries.serve(version)

    proof = proving(identifier, version)

    equal(proof.outcome, Outcome.NOT_SERVED, describing=f"the proof of `{identifier}`")
    equal(proof.exit_status, 3, describing="the exit a version nothing serves answers with")
    contains(proof.report, "publish that did not happen", describing=proof.report)
    contains(proof.report, f"client: {identifier}", describing=proof.report)


@pytest.mark.parametrize("identifier", CLIENTS)
def test_a_forge_serving_no_release_for_the_supervisor_names_the_forge(
    identifier: str, version: str, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """The client is there and the program it would run against is not: the forge is named.

    Nothing is installed, because the repair is the release's publish rather
    than the client — and a proof that took the client first would report a
    client that works as one that does not.
    """
    registries.serve_clients(identifier)

    proof = proving(identifier, version)

    equal(proof.outcome, Outcome.NOT_SERVED, describing=f"the proof of `{identifier}`")
    contains(proof.report, "release:printobserver", describing="the target nothing serves")
    contains(proof.report, "supervisor:", describing=proof.report)
    contains(proof.report, "/forge/releases", describing="the forge, named")


@pytest.mark.parametrize("identifier", CLIENTS)
def test_a_client_that_installs_and_cannot_be_used_does_not_pass(
    identifier: str,
    version: str,
    supervisor: Path,
    registries: Registries,
    proving: Callable[..., Proof],
) -> None:
    """A registry serving the name with nothing usable in it is a build to repair."""
    registries.serve(version, program=supervisor)
    registries.serve_clients(identifier, broken=True)

    proof = proving(identifier, version)

    equal(proof.outcome, Outcome.NOT_PROVEN, describing=f"the proof of `{identifier}`")
    equal(proof.exit_status, 1, describing="the exit a broken artifact answers with")
    contains(proof.report, "did not work here", describing=proof.report)
    contains(proof.report, "build to repair", describing=proof.report)


def test_the_clients_taken_are_exactly_the_clients_declared(repo: Repo) -> None:
    """The committed tree's own two copies agree."""
    equal(
        sorted(clients(repo)),
        sorted(
            target.id
            for target in declared(repo.root)
            if not target.route and target.built_by == "release-artifacts"
        ),
        describing="the clients this proof takes",
    )


def test_a_client_declared_that_nothing_here_takes_is_refused(repo: Repo, tmp_path: Path) -> None:
    """A fourth client would be one this tier reports nothing at all about."""
    tree = tmp_path / "fourth"
    tree.mkdir()
    declaration = (repo.root / "release-targets.toml").read_text(encoding="utf-8")
    (tree / "release-targets.toml").write_text(
        declaration + '\n[[target]]\nid = "gem:printobserver-sdk"\n'
        'description = "A fourth client."\nbuilt_by = "release-artifacts"\n',
        encoding="utf-8",
    )

    with pytest.raises(RegistryError) as refused:
        clients(Repo(tree))

    contains(str(refused.value), "gem:printobserver-sdk", describing="the client nothing takes")
    contains(str(refused.value), "nothing here takes it", describing="what it said")


def test_a_client_taken_here_that_nothing_declares_is_refused(repo: Repo, tmp_path: Path) -> None:
    """And the other direction: an entry beside a client nobody publishes is dead."""
    tree = tmp_path / "undeclared"
    tree.mkdir()
    declaration = (repo.root / "release-targets.toml").read_text(encoding="utf-8")
    start = declaration.index('[[target]]\nid = "npm:@printobserver/sdk"')
    end = declaration.index("[[target]]", start + 1)
    (tree / "release-targets.toml").write_text(
        declaration[:start] + declaration[end:], encoding="utf-8"
    )

    with pytest.raises(RegistryError) as refused:
        clients(Repo(tree))

    contains(str(refused.value), "npm:@printobserver/sdk", describing="the client nothing declares")
    contains(str(refused.value), "declared as no client", describing="what it said")
