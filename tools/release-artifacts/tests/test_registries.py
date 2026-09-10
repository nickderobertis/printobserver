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
from dataclasses import dataclass
from pathlib import Path

import pytest
from release_artifacts.__main__ import main
from release_artifacts.installing import InstallError
from release_artifacts.registries import (
    PRINTOBSERVER_PROOF_REGISTRIES,
    PRINTOBSERVER_PROOF_VERSION,
    RELEASE,
    UNREADABLE,
    VERSION_FIELD,
    Bases,
    Outcome,
    Proof,
    RegistryError,
    cut_at,
    ordered,
    prove,
    released,
    select,
    served,
    take,
)
from release_artifacts.standin import FORGE_PREFIX, NPM_PREFIX, PYPI_PREFIX, Registries
from release_artifacts.targets import named
from repo_checks.expect import contains, equal, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run

#: The three routes an end user gets the program by, each taken from its own
#: registry. Every one of them is driven against every outcome below.
ROUTES = ["pypi:printobserver-cli", "npm:printobserver-cli", "release:printobserver"]

#: A version no registry here serves unless a case serves it, and no tree of
#: this repository declares.
UNSERVED = "9.9.9"


# llmlint: ignore[test_tiers_split_by_project_not_by_marker] suppressions.toml has the reason.
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] The same reason as above it.
@pytest.fixture
def registries(repo: Repo, tmp_path: Path) -> Iterator[Registries]:
    """The three registries, answering on one address, serving nothing yet."""
    standing_in = Registries(repo, tmp_path / "served")
    try:
        yield standing_in
    finally:
        standing_in.stop()


@pytest.fixture
def proving(repo: Repo, registries: Registries, tmp_path: Path) -> Callable[..., Proof]:
    """Take one route from the stand-in registries and prove what they served."""

    def prove_route(identifier: str, wanted: str = "") -> Proof:
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

    return prove_route


@pytest.mark.parametrize("identifier", ROUTES)
def test_a_registry_serving_a_working_artifact_is_a_pass(
    identifier: str, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """What the registry serves is installed with no Rust toolchain, run, and read back."""
    registries.serve("0.4.0")

    proof = proving(identifier)

    equal(proof.outcome, Outcome.PROVEN, describing=f"the proof of `{identifier}`")
    equal(proof.exit_status, 0, describing="the exit a pass answers with")
    contains(proof.report, "printobserver 0.4.0", describing="what the installed program reported")
    contains(proof.report, "Rust toolchain on the install path: none", describing=proof.report)


@pytest.mark.parametrize("identifier", ROUTES)
def test_a_registry_serving_nothing_for_the_version_under_test_is_not_served(
    identifier: str, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """A publish that did not happen is reported as that rather than as a broken artifact."""
    registries.serve("0.4.0")

    proof = proving(identifier, UNSERVED)

    equal(proof.outcome, Outcome.NOT_SERVED, describing=f"the proof of `{identifier}`")
    equal(proof.exit_status, 3, describing="the exit a version nothing serves answers with")
    contains(proof.report, "NOT SERVED", describing=proof.report)
    contains(proof.report, "0.4.0", describing="what the registry does serve")
    contains(proof.report, "publish that did not happen", describing=proof.report)


@pytest.mark.parametrize("identifier", ROUTES)
def test_a_registry_serving_no_version_at_all_cannot_pass(
    identifier: str, proving: Callable[..., Proof]
) -> None:
    """The state this repository was actually in: green checks over nothing published."""
    proof = proving(identifier)

    equal(proof.outcome, Outcome.NOT_SERVED, describing=f"the proof of `{identifier}`")
    equal(proof.exit_status, 3, describing="the exit a registry serving nothing answers with")
    contains(proof.report, "no version at all", describing=proof.report)


@pytest.mark.parametrize("identifier", ROUTES)
def test_an_artifact_that_cannot_be_run_is_reported_apart_from_one_nothing_serves(
    identifier: str, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """Served and not proven is a build to repair, and it is not `NOT SERVED`."""
    registries.serve("0.5.0", broken=True)

    proof = proving(identifier)

    equal(proof.outcome, Outcome.NOT_PROVEN, describing=f"the proof of `{identifier}`")
    equal(
        proof.exit_status,
        1,
        describing="the exit a served artifact that does not work answers with",
    )
    contains(proof.report, "SERVED AND NOT PROVEN", describing=proof.report)
    contains(proof.report, "did not work here", describing=proof.report)
    truth(
        "NOT SERVED" not in proof.report.partition("\n")[0],
        describing="the two outcomes to be told apart",
    )


@pytest.mark.parametrize("reported", ["0.1.0", "10.6.0"])
def test_an_artifact_reporting_another_version_does_not_pass(
    reported: str, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """Installing is not enough: what the program says it is has to be what was proven.

    `10.6.0` is the case a substring comparison passes: it contains `0.6.0`, so
    a proof of one release would go green over the artifact of another — which
    is the failure this tier exists to catch, arriving through the check for it.
    """
    registries.serve("0.6.0", reported=reported)

    proof = proving("pypi:printobserver-cli")

    equal(proof.outcome, Outcome.NOT_PROVEN, describing="the proof of a mislabelled artifact")
    equal(proof.exit_status, 1, describing="the exit it answers with")
    contains(proof.report, "not the version under test", describing=proof.report)


def test_the_version_a_caller_names_is_the_one_proven(
    registries: Registries, proving: Callable[..., Proof]
) -> None:
    """A named version is taken rather than whatever the registry serves newest."""
    registries.serve("0.7.0")
    registries.serve("0.8.0")

    proof = proving("npm:printobserver-cli", "0.7.0")

    equal(proof.outcome, Outcome.PROVEN, describing="the proof of the named version")
    contains(proof.report, "printobserver 0.7.0", describing=proof.report)
    contains(proof.report, "named by the caller", describing=proof.report)


def test_the_release_a_run_is_keyed_on_is_taken_from_the_forge(
    registries: Registries, proving: Callable[..., Proof]
) -> None:
    """`release` takes the newest release the forge published, and not the newest served.

    The registry here serves a version the forge never released, which is what
    a publish that reached one place and not the other looks like. A proof of a
    release has to prove that release rather than whatever happens to be newest.
    """
    registries.serve("0.9.0")
    registries.serve(UNSERVED, listed=False)

    proof = proving("pypi:printobserver-cli", RELEASE)

    equal(proof.outcome, Outcome.PROVEN, describing="the proof of the released version")
    contains(proof.report, "printobserver 0.9.0", describing=proof.report)
    contains(proof.report, "the newest release the forge published", describing=proof.report)


def test_a_release_no_registry_serves_is_an_observable_failure(
    registries: Registries, proving: Callable[..., Proof]
) -> None:
    """The release-time proof cannot conceal a release nothing published for."""
    registries.serve("0.9.0")
    registries.release(f"v{UNSERVED}")

    proof = proving("pypi:printobserver-cli", RELEASE)

    equal(proof.outcome, Outcome.NOT_SERVED, describing="the proof of an unpublished release")
    equal(proof.exit_status, 3, describing="the exit it answers with")
    contains(proof.report, UNSERVED, describing=proof.report)


def test_a_forge_listing_no_release_selects_nothing(
    registries: Registries, proving: Callable[..., Proof]
) -> None:
    """A run keyed on a release, before there is one, is refused rather than guessed."""
    proof = proving("npm:printobserver-cli", RELEASE)

    equal(proof.outcome, Outcome.NOT_SERVED, describing="the proof with no release to key on")
    contains(proof.report, "it lists none", describing=proof.report)


def test_the_stated_command_of_the_route_is_what_the_report_names(
    registries: Registries, proving: Callable[..., Proof]
) -> None:
    """The install-path section is the source of the route, including in a proof of it.

    A pass names the command that did the installing, and a failure names the
    route it belongs to as well — because what a reader of a failure needs is
    everything this run knew, and what a reader of a pass needs is the answer.
    """
    registries.serve("0.4.0")

    passed = proving("pypi:printobserver-cli").report
    refused = proving("pypi:printobserver-cli", UNSERVED).report

    contains(passed, "`pip install printobserver-cli` installed", describing=passed)
    contains(refused, "pip install printobserver-cli", describing=refused)
    contains(refused, "Route 1", describing=refused)


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
    answer = registries.answer("/npm/no-such-package")

    equal(answer.status, 404, describing="what a name nothing is published under answers")
    contains(answer.body.decode(), "not served here", describing="what it said")


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
    repo: Repo,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An unreachable registry answers with an exit of its own.

    Sharing one with `SERVED AND NOT PROVEN` would send a reader to repair an
    artifact nothing here even read.
    """
    monkeypatch.setenv(PRINTOBSERVER_PROOF_REGISTRIES, "http://127.0.0.1:1")
    monkeypatch.setenv(PRINTOBSERVER_PROOF_VERSION, "")

    equal(
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
        UNREADABLE,
        describing="the exit a registry nothing could read answers with",
    )

    contains(capsys.readouterr().err, "could not be reached", describing="what it said")


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
    identifier: str, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """The other way a published artifact is broken: nothing on the path at all.

    Which is the difference between installing and being installed: an install
    that reported success and put no program anywhere is exactly what the jobs
    that only installed could not tell from a working route.
    """
    registries.serve("0.4.3", carries_program=False)

    proof = proving(identifier)

    equal(proof.outcome, Outcome.NOT_PROVEN, describing=f"the proof of `{identifier}`")
    equal(proof.exit_status, 1, describing="the exit an artifact leaving no program answers with")
    contains(proof.report, "SERVED AND NOT PROVEN", describing=proof.report)


def test_a_launcher_whose_platform_package_was_never_published_does_not_pass(
    registries: Registries, proving: Callable[..., Proof]
) -> None:
    """Route 2's quietest failure: the launcher served, and the package beside it not.

    The launcher declares one package per supported platform as an optional
    dependency, and an optional dependency nothing serves is one `npm` skips —
    so the install **reports success** and what it leaves on the path cannot
    run. Nothing that proves a local build resolves anything from a registry,
    which is why this is invisible everywhere but here.
    """
    registries.serve("0.4.4", platform_package=False)

    proof = proving("npm:printobserver-cli")

    equal(proof.outcome, Outcome.NOT_PROVEN, describing="the proof of a launcher on its own")
    equal(
        proof.exit_status, 1, describing="the exit a served route that does not work answers with"
    )
    contains(proof.report, "SERVED AND NOT PROVEN", describing=proof.report)
    contains(proof.report, "is not installed", describing="what the launcher itself said")
    truth(
        "NOT SERVED" not in proof.report.partition("\n")[0],
        describing="a launcher that was served to be told from one that was not",
    )


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


def test_a_registry_answering_a_shape_its_protocol_does_not_serve_is_refused(
    repo: Repo, registries: Registries
) -> None:
    """A malformed metadata document is not a registry serving nothing.

    Read past, it would come back as `NOT SERVED` — which sends a reader to
    repair a publish that happened, and is the one confusion this proof exists
    to remove.
    """
    registries.serve("0.4.0")
    registries.answers(f"{PYPI_PREFIX}/pypi/printobserver-cli/json", b'{"releases": "all of them"}')
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    with pytest.raises(RegistryError) as refused:
        served(bases, named(repo.root, "pypi:printobserver-cli"))

    contains(str(refused.value), "not the mapping of versions", describing="what it said")


def test_a_registry_answering_something_other_than_a_document_is_refused(
    repo: Repo, registries: Registries
) -> None:
    """A body that is JSON and is not that registry's own document is refused."""
    registries.answers(f"{NPM_PREFIX}/printobserver-cli", b"[1, 2, 3]")
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    with pytest.raises(RegistryError) as refused:
        served(bases, named(repo.root, "npm:printobserver-cli"))

    contains(str(refused.value), "metadata document", describing="what it said")


def test_a_release_listing_carrying_something_that_is_not_a_release_is_refused(
    repo: Repo, registries: Registries
) -> None:
    """Dropped silently, the newest release could be the one that went missing."""
    registries.answers(FORGE_PREFIX, b'[{"tag_name": "v0.4.0"}, {"name": "no tag at all"}]')
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    with pytest.raises(RegistryError) as refused:
        released(bases)

    contains(str(refused.value), "not a release with a tag", describing="what it said")


def test_the_stand_in_refuses_a_version_that_is_not_one_to_serve(
    repo: Repo, tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    """Everything a caller names here reaches a manifest, a file name and a path."""
    monkeypatch.setattr(sys, "stdin", io.StringIO(""))

    equal(
        main(
            [
                "standin",
                "--serves",
                "../../etc",
                "--into",
                str(tmp_path / "refused"),
                "--root",
                str(repo.root),
            ]
        ),
        1,
        describing="the exit a version nothing can serve answers with",
    )

    contains(capsys.readouterr().err, "is not a version to serve", describing="what it said")


def test_the_stand_in_refuses_a_mislabelling_that_names_one_version(
    repo: Repo, tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    """`--mislabelled` names the version served and the one the program reports."""
    monkeypatch.setattr(sys, "stdin", io.StringIO(""))

    equal(
        main(
            [
                "standin",
                "--mislabelled",
                "0.1.0",
                "--into",
                str(tmp_path / "refused-pair"),
                "--root",
                str(repo.root),
            ]
        ),
        1,
        describing="the exit a mislabelling naming one version answers with",
    )

    contains(capsys.readouterr().err, "names no reported version", describing="what it said")


def test_a_failing_proof_reports_where_a_reader_of_a_failure_looks(
    repo: Repo,
    registries: Registries,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A pass goes to standard output, and the two failures to standard error."""
    registries.serve("0.4.0")
    monkeypatch.setenv(PRINTOBSERVER_PROOF_REGISTRIES, registries.base)
    monkeypatch.setenv(PRINTOBSERVER_PROOF_VERSION, UNSERVED)

    equal(
        main(
            [
                "prove",
                "--registry",
                "--target",
                "npm:printobserver-cli",
                "--into",
                str(tmp_path / "cli-not-served"),
                "--root",
                str(repo.root),
            ]
        ),
        3,
        describing="the exit a version nothing serves answers with",
    )

    said = capsys.readouterr()
    contains(said.err, "NOT SERVED", describing="what a reader of a failure is shown")
    equal(said.out, "", describing="what a failing proof writes to standard output")


def test_a_version_that_is_no_version_to_prove_is_refused(
    repo: Repo, registries: Registries, tmp_path: Path
) -> None:
    """What a caller names reaches a package manager, so it is validated here."""
    registries.serve("0.4.0")

    with pytest.raises(RegistryError) as refused:
        prove(
            repo,
            "pypi:printobserver-cli",
            tmp_path / "nonsense",
            {
                PRINTOBSERVER_PROOF_REGISTRIES: registries.base,
                PRINTOBSERVER_PROOF_VERSION: "the latest one, please",
            },
        )

    contains(str(refused.value), "is no version to prove", describing="what it said")


def test_a_pre_release_a_registry_serves_is_never_the_newest(
    repo: Repo, registries: Registries, proving: Callable[..., Proof]
) -> None:
    """A registry serving a pre-release beside the real ones is ordinary.

    And it is neither what "the newest" means here nor something to hand a
    package manager, so it is not a version this proof will select.
    """
    registries.serve("0.4.0")
    registries.answers(
        f"{PYPI_PREFIX}/pypi/printobserver-cli/json",
        b'{"releases": {"0.4.0": [], "0.5.0rc1": []}}',
    )
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    equal(
        served(bases, named(repo.root, "pypi:printobserver-cli")),
        ("0.4.0",),
        describing="the versions this proof can select",
    )
    equal(proving("pypi:printobserver-cli").outcome, Outcome.PROVEN, describing="the proof")


def test_a_tag_naming_no_version_is_not_a_release_a_run_is_keyed_on(
    repo: Repo, registries: Registries
) -> None:
    """A forge lists whatever somebody tagged; a release is three numbers."""
    registries.serve("0.4.0")
    registries.answers(FORGE_PREFIX, b'[{"tag_name": "v0.4.0"}, {"tag_name": "nightly"}]')
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    equal(released(bases), ("0.4.0",), describing="the releases a run can be keyed on")


def test_a_pre_release_the_forge_lists_is_not_a_release_a_run_is_keyed_on(
    repo: Repo, registries: Registries
) -> None:
    """The forge marks a draft and a pre-release, and a run is keyed on neither.

    What a release-time run proves is what an ordinary user's own install would
    resolve to, and neither of those is that.
    """
    registries.serve("0.4.0")
    registries.answers(
        FORGE_PREFIX,
        b'[{"tag_name": "v0.9.0", "prerelease": true, "draft": false},'
        b' {"tag_name": "v0.8.0", "prerelease": false, "draft": true},'
        b' {"tag_name": "v0.4.0", "prerelease": false, "draft": false}]',
    )
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    equal(released(bases), ("0.4.0",), describing="the releases a run can be keyed on")
    equal(
        select(bases, named(repo.root, "pypi:printobserver-cli"), RELEASE).version,
        "0.4.0",
        describing="the version a release-time run proves",
    )


def test_a_release_flag_that_is_not_a_boolean_is_refused(
    repo: Repo, registries: Registries
) -> None:
    """Read by truthiness, a `"false"` somebody answered with is a release skipped."""
    registries.serve("0.4.0")
    registries.answers(FORGE_PREFIX, b'[{"tag_name": "v0.4.0", "prerelease": "false"}]')
    bases = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: registries.base})

    with pytest.raises(RegistryError) as refused:
        released(bases)

    contains(str(refused.value), "not the boolean its protocol serves", describing="what it said")


#: Two release-time runs, in the order they finished. Each cut the release its
#: own tag names, and the second one's release is another run's as far as the
#: first is concerned — which is the whole of what binding a proof to a run is
#: about.
CUT = ("0.5.0", "0.6.0")


@dataclass(frozen=True, slots=True)
class Checkout:
    """A real repository carrying the tag each release-time run left behind."""

    path: Path
    #: The commit each run ran at, by the version it cut.
    at: dict[str, str]


@pytest.fixture
def checkout(tmp_path: Path) -> Checkout:
    """Two release-time runs, in a real git repository.

    A real one rather than a stand-in: what binds a release to the run that cut
    it is the tag release automation left at that run's own commit, and git is
    the only thing that can be asked about that.
    """
    root = tmp_path / "checkout"
    root.mkdir(parents=True, exist_ok=True)

    def git(*argv: str) -> str:
        done = run(["git", *argv], cwd=root, timeout=60)
        truth(done.returncode == 0, describing=f"`git {' '.join(argv)}`:\n{done.stderr}")
        return done.stdout.strip()

    git("init", "--initial-branch", "main")
    git("config", "user.email", "release@example.invalid")
    git("config", "user.name", "release automation")
    at: dict[str, str] = {}
    for version in CUT:
        git("commit", "--allow-empty", "-m", f"chore: release v{version}")
        git("tag", f"v{version}")
        at[version] = git("rev-parse", "HEAD")
    return Checkout(root, at)


def resolved(checkout: Checkout, version: str, capsys: pytest.CaptureFixture[str]) -> str:
    """The version the release-time run that cut `version` answers, through its own command.

    Driven as the workflow drives it — the committed command line, whose one
    line a job publishes an output from — rather than by calling in past it.
    """
    equal(
        main(["released", "--commit", checkout.at[version], "--root", str(checkout.path)]),
        0,
        describing="the exit resolving a release-time run's own release answers with",
    )
    field, separator, said = capsys.readouterr().out.strip().partition("=")
    equal(field, VERSION_FIELD, describing="the field a job reads its output from")
    truth(bool(separator), describing="the output to be a field and a value")
    return said


def test_a_release_time_run_proves_the_release_it_cut_and_not_the_newest(
    repo: Repo,
    checkout: Checkout,
    registries: Registries,
    proving: Callable[..., Proof],
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Two runs finish, and the newer release is the other one's.

    Keyed on the newest the forge lists, the earlier run reports green over an
    artifact it never looked at while its own release goes unproven — and
    `release_always` makes two runs in flight ordinary rather than rare. So the
    release is bound to the run by the tag that run left at its own commit.
    """
    for version in CUT:
        registries.serve(version)

    version = resolved(checkout, "0.5.0", capsys)

    equal(version, "0.5.0", describing="the release the run this proof is keyed on cut")
    proof = proving("pypi:printobserver-cli", version)
    equal(proof.outcome, Outcome.PROVEN, describing="the proof of that run's own release")
    contains(proof.report, "printobserver 0.5.0", describing=proof.report)
    truth("0.6.0" not in proof.report, describing="another run's release to go unproven here")


def test_a_run_whose_release_was_never_published_is_an_observable_failure(
    repo: Repo,
    checkout: Checkout,
    registries: Registries,
    proving: Callable[..., Proof],
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The release is cut and the publish after it fails: the state this tier exists for.

    Gated on the triggering run's conclusion, that run is skipped and nothing
    reports the missing publish at all. Gated on the release it cut, the run is
    proven and the registries' silence is named — as a publish that did not
    happen rather than as an artifact that does not work.
    """
    registries.serve("0.5.0")
    registries.release("v0.6.0")

    version = resolved(checkout, "0.6.0", capsys)

    equal(version, "0.6.0", describing="the release the failed run had already cut")
    proof = proving("npm:printobserver-cli", version)
    equal(proof.outcome, Outcome.NOT_SERVED, describing="the proof of an unpublished release")
    equal(proof.exit_status, 3, describing="the exit it answers with")
    contains(proof.report, "publish that did not happen", describing=proof.report)
    contains(proof.report, "0.6.0", describing=proof.report)


def test_a_run_that_cut_no_release_answers_none(
    checkout: Checkout, capsys: pytest.CaptureFixture[str]
) -> None:
    """Every push finishes a release run and all but the release ones cut nothing.

    The empty field is what the jobs proving a route are gated on, so an
    ordinary push proves nothing rather than proving whatever was newest.
    """
    ordinary = run(
        ["git", "commit", "--allow-empty", "-m", "fix: a change that released nothing"],
        cwd=checkout.path,
        timeout=60,
    )
    truth(ordinary.returncode == 0, describing=ordinary.stderr)
    head = run(["git", "rev-parse", "HEAD"], cwd=checkout.path, timeout=60).stdout.strip()

    equal(
        main(["released", "--commit", head, "--root", str(checkout.path)]),
        0,
        describing="the exit a run that cut no release answers with",
    )
    equal(
        capsys.readouterr().out.strip(),
        f"{VERSION_FIELD}=",
        describing="the empty field a run that cut nothing publishes",
    )


def test_a_checkout_that_does_not_carry_the_commit_is_refused(checkout: Checkout) -> None:
    """A shallow clone answers `no release` for every commit, which passes over every publish."""
    with pytest.raises(RegistryError) as refused:
        cut_at(checkout.path, "0" * 40)

    contains(str(refused.value), "does not carry the commit", describing="what it said")


def test_a_commit_that_is_no_object_name_is_refused(checkout: Checkout) -> None:
    """What arrives from an event payload reaches `git` as an argument."""
    with pytest.raises(RegistryError) as refused:
        cut_at(checkout.path, "the one that broke")

    contains(str(refused.value), "no commit to key", describing="what it said")


def test_two_releases_at_one_commit_are_refused(checkout: Checkout) -> None:
    """Which release that run cut is then not something a tag can answer."""
    tagged = run(["git", "tag", "v0.7.0", checkout.at["0.6.0"]], cwd=checkout.path, timeout=60)
    truth(tagged.returncode == 0, describing=tagged.stderr)

    with pytest.raises(RegistryError) as refused:
        cut_at(checkout.path, checkout.at["0.6.0"])

    contains(str(refused.value), "not something a tag can answer", describing="what it said")


def test_resolving_a_release_without_naming_a_commit_is_refused(
    repo: Repo, capsys: pytest.CaptureFixture[str]
) -> None:
    """The commit is the whole binding, so the command asking for one has to have it."""
    equal(
        main(["released", "--root", str(repo.root)]),
        2,
        describing="the exit naming no commit answers with",
    )

    contains(capsys.readouterr().err, "takes --commit", describing="what it said")
