"""Proving each route against a registry, driven against registries stood up here.

Nothing is mocked and nothing publishes. `standin.py` stands the three
registries up for real — a Python index serving real wheels, a JavaScript
registry serving real packages, a forge listing real releases with real
artifacts and digests — and the proof under test resolves, installs and runs
against them with the same `pip`, `npm` and committed install script an end user
runs.

Every direction is falsified rather than argued: a registry serving nothing for
the version under test does not pass and says it was not served, one serving
something that cannot be run does not pass and says so distinctly, one serving
something mislabelled does not pass, and one serving a working artifact passes.
"""

from __future__ import annotations

import io
import sys
from collections.abc import Callable, Iterator
from pathlib import Path

import pytest
from release_artifacts.__main__ import main
from release_artifacts.installing import InstallError
from release_artifacts.registries import (
    PRINTOBSERVER_PROOF_REGISTRIES,
    PRINTOBSERVER_PROOF_VERSION,
    RELEASE,
    Bases,
    Outcome,
    RegistryError,
    ordered,
    prove,
    released,
    select,
    served,
    take,
)
from release_artifacts.standin import FORGE_PREFIX, PYPI_PREFIX, Registries
from release_artifacts.targets import named
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo

#: The three routes an end user gets the program by, each taken from its own
#: registry. Every one of them is driven against every outcome below.
ROUTES = ["pypi:printobserver-cli", "npm:printobserver-cli", "release:printobserver"]

#: A version no registry here serves unless a case serves it, and no tree of
#: this repository declares.
UNSERVED = "9.9.9"


@pytest.fixture
def registries(repo: Repo, tmp_path: Path) -> Iterator[Registries]:
    """The three registries, answering on one address, serving nothing yet."""
    standing_in = Registries(repo, tmp_path / "served")
    try:
        yield standing_in
    finally:
        standing_in.stop()


@pytest.fixture
def proving(
    repo: Repo, registries: Registries, tmp_path: Path
) -> Callable[..., tuple[Outcome, str, int]]:
    """Take one route from the stand-in registries and prove what they served."""

    def prove_route(identifier: str, wanted: str = "") -> tuple[Outcome, str, int]:
        into = tmp_path / identifier.replace(":", "-").replace("/", "-") / (wanted or "newest")
        proof = prove(
            repo,
            identifier,
            into,
            {
                PRINTOBSERVER_PROOF_REGISTRIES: registries.base,
                PRINTOBSERVER_PROOF_VERSION: wanted,
            },
        )
        return proof.outcome, proof.report, proof.exit_status

    return prove_route


@pytest.mark.parametrize("identifier", ROUTES)
def test_a_registry_serving_a_working_artifact_is_a_pass(
    identifier: str, registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """What the registry serves is installed with no Rust toolchain, run, and read back."""
    registries.serve("0.4.0")

    outcome, report, status = proving(identifier)

    equal(outcome, Outcome.PROVEN, describing=f"the proof of `{identifier}`")
    equal(status, 0, describing="the exit a pass answers with")
    contains(report, "printobserver 0.4.0", describing="what the installed program reported")
    contains(report, "Rust toolchain on the install path: none", describing=report)


@pytest.mark.parametrize("identifier", ROUTES)
def test_a_registry_serving_nothing_for_the_version_under_test_is_not_served(
    identifier: str, registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """A publish that did not happen is reported as that rather than as a broken artifact."""
    registries.serve("0.4.0")

    outcome, report, status = proving(identifier, UNSERVED)

    equal(outcome, Outcome.NOT_SERVED, describing=f"the proof of `{identifier}`")
    equal(status, 3, describing="the exit a version nothing serves answers with")
    contains(report, "NOT SERVED", describing=report)
    contains(report, "0.4.0", describing="what the registry does serve")
    contains(report, "publish that did not happen", describing=report)


@pytest.mark.parametrize("identifier", ROUTES)
def test_a_registry_serving_no_version_at_all_cannot_pass(
    identifier: str, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """The state this repository was actually in: green checks over nothing published."""
    outcome, report, status = proving(identifier)

    equal(outcome, Outcome.NOT_SERVED, describing=f"the proof of `{identifier}`")
    equal(status, 3, describing="the exit a registry serving nothing answers with")
    contains(report, "no version at all", describing=report)


@pytest.mark.parametrize("identifier", ROUTES)
def test_an_artifact_that_cannot_be_run_is_reported_apart_from_one_nothing_serves(
    identifier: str, registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """Served and not proven is a build to repair, and it is not `NOT SERVED`."""
    registries.serve("0.5.0", broken=True)

    outcome, report, status = proving(identifier)

    equal(outcome, Outcome.NOT_PROVEN, describing=f"the proof of `{identifier}`")
    equal(status, 1, describing="the exit a served artifact that does not work answers with")
    contains(report, "SERVED AND NOT PROVEN", describing=report)
    contains(report, "did not work here", describing=report)
    truth("NOT SERVED" not in report.partition("\n")[0], describing="the two to be told apart")


def test_an_artifact_reporting_another_version_does_not_pass(
    registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """Installing is not enough: what the program says it is has to be what was proven."""
    registries.serve("0.6.0", reported="0.1.0")

    outcome, report, status = proving("pypi:printobserver-cli")

    equal(outcome, Outcome.NOT_PROVEN, describing="the proof of a mislabelled artifact")
    equal(status, 1, describing="the exit it answers with")
    contains(report, "not the version under test", describing=report)


def test_the_version_a_caller_names_is_the_one_proven(
    registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """A named version is taken rather than whatever the registry serves newest."""
    registries.serve("0.7.0")
    registries.serve("0.8.0")

    outcome, report, _ = proving("npm:printobserver-cli", "0.7.0")

    equal(outcome, Outcome.PROVEN, describing="the proof of the named version")
    contains(report, "printobserver 0.7.0", describing=report)
    contains(report, "named by the caller", describing=report)


def test_the_release_a_run_is_keyed_on_is_taken_from_the_forge(
    registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """`release` takes the newest release the forge published, and not the newest served.

    The registry here serves a version the forge never released, which is what
    a publish that reached one place and not the other looks like. A proof of a
    release has to prove that release rather than whatever happens to be newest.
    """
    registries.serve("0.9.0")
    registries.serve(UNSERVED, listed=False)

    outcome, report, _ = proving("pypi:printobserver-cli", RELEASE)

    equal(outcome, Outcome.PROVEN, describing="the proof of the released version")
    contains(report, "printobserver 0.9.0", describing=report)
    contains(report, "the newest release the forge published", describing=report)


def test_a_release_no_registry_serves_is_an_observable_failure(
    registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """The release-time proof cannot conceal a release nothing published for."""
    registries.serve("0.9.0")
    registries.release(f"v{UNSERVED}")

    outcome, report, status = proving("pypi:printobserver-cli", RELEASE)

    equal(outcome, Outcome.NOT_SERVED, describing="the proof of an unpublished release")
    equal(status, 3, describing="the exit it answers with")
    contains(report, UNSERVED, describing=report)


def test_a_forge_listing_no_release_selects_nothing(
    registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """A run keyed on a release, before there is one, is refused rather than guessed."""
    outcome, report, _ = proving("npm:printobserver-cli", RELEASE)

    equal(outcome, Outcome.NOT_SERVED, describing="the proof with no release to key on")
    contains(report, "it lists none", describing=report)


def test_the_stated_command_of_the_route_is_what_the_report_names(
    registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """The install-path section is the source of the route, including in a proof of it."""
    registries.serve("0.4.0")

    _, report, _ = proving("pypi:printobserver-cli")

    contains(report, "pip install printobserver-cli", describing=report)
    contains(report, "Route 1", describing=report)


def test_the_real_registries_are_where_a_run_with_no_stand_in_reads(repo: Repo) -> None:
    """Nothing points at a stand-in unless a caller says so."""
    bases = Bases.read(repo, {})

    equal(bases.pypi, "https://pypi.org", describing="where route 1 is taken from")
    equal(bases.npm, "https://registry.npmjs.org", describing="where route 2 is taken from")
    contains(bases.listing, "api.github.com", describing="where releases are listed")
    contains(bases.releases, "/releases", describing="where a release is downloaded from")
    equal(bases.of("crate"), "", describing="a registry no route is taken from")


def test_a_stand_in_address_points_every_registry_at_it(repo: Repo) -> None:
    """One address covers all three: a proof reading one of each would prove neither."""
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: "http://127.0.0.1:9/"})

    for where in (bases.pypi, bases.npm, bases.listing, bases.releases):
        contains(where, "http://127.0.0.1:9/", describing="where a registry is read from")


def test_a_registry_that_cannot_be_reached_is_neither_outcome(repo: Repo, tmp_path: Path) -> None:
    """An unreachable registry is a network, not a missing publish or a bad build."""
    with pytest.raises(RegistryError) as refused:
        prove(
            repo,
            "pypi:printobserver-cli",
            tmp_path / "unreachable",
            {PRINTOBSERVER_PROOF_REGISTRIES: "http://127.0.0.1:1"},
        )

    contains(str(refused.value), "could not be reached", describing="what it said")


def test_an_address_no_registry_is_asked_over_is_refused(repo: Repo) -> None:
    """A registry base that is not http is refused before anything is asked of it."""
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: "file:///etc"})

    with pytest.raises(RegistryError) as refused:
        served(bases, named(repo.root, "pypi:printobserver-cli"))

    contains(str(refused.value), "not an address", describing="what it said")


def test_a_registry_answering_something_other_than_its_protocol_is_refused(
    repo: Repo, registries: Registries
) -> None:
    """A body that is not the JSON a registry serves is a stop rather than a guess."""
    registries.serve("0.4.0")
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})
    registries.answers(f"{PYPI_PREFIX}/pypi/printobserver-cli/json", b"not json")

    with pytest.raises(RegistryError) as refused:
        served(bases, named(repo.root, "pypi:printobserver-cli"))

    contains(str(refused.value), "other than the JSON", describing="what it said")


def test_nothing_here_asks_a_registry_no_route_is_taken_from(repo: Repo) -> None:
    """A crate is not a route, and a proof of one is refused rather than invented."""
    bases = Bases.read(repo, {})

    with pytest.raises(RegistryError) as refused:
        served(bases, named(repo.root, "crate:printobserver-sdk"))

    contains(str(refused.value), "knows how to ask", describing="what it said")


def test_nothing_here_takes_a_route_that_is_not_one_of_the_three(
    repo: Repo, tmp_path: Path
) -> None:
    """A target with no route is refused rather than taken some default way."""
    with pytest.raises(InstallError) as refused:
        take(
            repo,
            named(repo.root, "crate:printobserver-sdk"),
            "0.1.0",
            tmp_path / "not-a-route",
            Bases.read(repo, {}),
        )

    contains(str(refused.value), "the way an end user takes it", describing="what it said")


def test_a_version_that_is_not_three_numbers_is_never_the_newest() -> None:
    """A pre-release nobody meant to install is not what "the newest" means."""
    truth(ordered("0.1.0-rc.1") < ordered("0.0.1"), describing="a pre-release to sort first")
    equal(ordered("v1.2.3"), (1, 2, 3), describing="a tag as it sorts")


def test_the_selection_of_a_version_names_where_it_came_from(
    repo: Repo, registries: Registries
) -> None:
    """Every answer a proof gives says which of the three ways it chose."""
    registries.serve("0.4.0")
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})
    target = named(repo.root, "npm:printobserver-cli")

    contains(select(bases, target, "0.2.0").whence, "named by the caller", describing="a name")
    contains(select(bases, target, RELEASE).whence, "release", describing="a release")
    contains(select(bases, target, "").whence, "the newest", describing="the newest served")


def test_the_stand_in_answers_nothing_for_a_path_it_does_not_serve(
    registries: Registries,
) -> None:
    """A registry serving no such name answers as a registry does, and not with bytes."""
    status, _, body = registries.answer("/npm/no-such-package")

    equal(status, 404, describing="what a name nothing is published under answers")
    contains(body.decode(), "not served here", describing="what it said")


def test_the_recipe_this_tier_runs_reports_the_outcome_as_its_exit(
    repo: Repo, registries: Registries, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The tool's own command line answers the three outcomes with three exits."""
    registries.serve("0.4.0")
    monkeypatch.setenv(PRINTOBSERVER_PROOF_REGISTRIES, registries.base)
    monkeypatch.setenv(PRINTOBSERVER_PROOF_VERSION, "")

    passing(
        (
            main(
                [
                    "prove",
                    "--registry",
                    "--target",
                    "npm:printobserver-cli",
                    "--into",
                    str(tmp_path / "cli-proven"),
                    "--root",
                    str(repo.root),
                ]
            ),
            "",
        ),
        describing="the registry proof of a served route",
    )

    monkeypatch.setenv(PRINTOBSERVER_PROOF_VERSION, UNSERVED)
    equal(
        main(
            [
                "prove",
                "--registry",
                "--target",
                "npm:printobserver-cli",
                "--into",
                str(tmp_path / "cli-unserved"),
                "--root",
                str(repo.root),
            ]
        ),
        3,
        describing="the exit a version nothing serves answers with",
    )


def test_the_command_line_refuses_a_registry_it_cannot_reach(
    repo: Repo, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An unreachable registry comes back as a refusal rather than as an outcome."""
    monkeypatch.setenv(PRINTOBSERVER_PROOF_REGISTRIES, "http://127.0.0.1:1")
    monkeypatch.setenv(PRINTOBSERVER_PROOF_VERSION, "")

    failing(
        (
            main(
                [
                    "prove",
                    "--registry",
                    "--target",
                    "pypi:printobserver-cli",
                    "--into",
                    str(tmp_path / "cli-unreachable"),
                    "--root",
                    str(repo.root),
                ]
            ),
            "",
        ),
        naming="",
    )


def test_the_stand_in_registries_are_started_by_their_own_command(
    repo: Repo,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """One command stands all three up, says where they answer, and holds them up.

    It is held up until whoever started it closes its input, which is what a
    consumer that is done does — so an input already at its end is a bring-up
    and an immediate bring-down.
    """
    monkeypatch.setattr(sys, "stdin", io.StringIO(""))

    equal(
        main(
            [
                "standin",
                "--serves",
                "0.4.0",
                "--broken",
                "0.4.1",
                "--mislabelled",
                "0.4.2=0.1.0",
                "--release",
                f"v{UNSERVED}",
                "--into",
                str(tmp_path / "standing-up"),
                "--root",
                str(repo.root),
            ]
        ),
        0,
        describing="standing the registries up",
    )

    contains(capsys.readouterr().out, "127.0.0.1", describing="where they said they answer")


@pytest.mark.parametrize("identifier", ROUTES)
def test_an_artifact_that_installs_and_leaves_no_program_does_not_pass(
    identifier: str, registries: Registries, proving: Callable[..., tuple[Outcome, str, int]]
) -> None:
    """The other way a published artifact is broken: nothing on the path at all.

    Which is the difference between installing and being installed: an install
    that reported success and put no program anywhere is exactly what the jobs
    that only installed could not tell from a working route.
    """
    registries.serve("0.4.3", carries_program=False)

    outcome, report, status = proving(identifier)

    equal(outcome, Outcome.NOT_PROVEN, describing=f"the proof of `{identifier}`")
    equal(status, 1, describing="the exit an artifact leaving no program answers with")
    contains(report, "SERVED AND NOT PROVEN", describing=report)


def test_a_registry_refusing_the_read_is_a_stop_naming_it(
    repo: Repo, registries: Registries
) -> None:
    """A refused read is not a publish that did not happen."""
    registries.serve("0.4.0")
    registries.answers(
        f"{PYPI_PREFIX}/pypi/printobserver-cli/json", b"upstream is unwell", status=503
    )
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    with pytest.raises(RegistryError) as refused:
        served(bases, named(repo.root, "pypi:printobserver-cli"))

    contains(str(refused.value), "refused the read", describing="what it said")


def test_a_forge_answering_something_other_than_releases_is_a_stop(
    repo: Repo, registries: Registries
) -> None:
    """A listing that is not a list of releases is refused rather than read past."""
    registries.answers(FORGE_PREFIX, b'{"message": "not a list of releases"}')
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    with pytest.raises(RegistryError) as refused:
        released(bases)

    contains(str(refused.value), "other than a list of releases", describing="what it said")


def test_a_tree_declaring_no_repository_cannot_say_where_its_releases_are(
    tmp_path: Path,
) -> None:
    """The forge's own paths are composed from the declaration rather than guessed."""
    (tmp_path / "repo-policy.toml").write_text("schema_version = 1\n", encoding="utf-8")

    with pytest.raises(RegistryError) as refused:
        Bases.read(Repo(tmp_path), {})

    contains(str(refused.value), "repository.owner", describing="what it said")
