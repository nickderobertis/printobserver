# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
"""The registry install-path proof, driven through its own recipes.

Nothing is mocked and nothing publishes. The real `just prove-registry-*`
recipes run, and what they read is a stand-in for the three registries stood up
by this repository's own `release-artifacts standin` — a Python index serving
real wheels, a JavaScript registry serving real packages, and a forge listing
real releases with their artifacts and digests. The real `pip`, the real `npm`
and the committed install script do the installing.

Every direction is falsified: a registry serving nothing for the version under
test does not pass and is reported as not served, one serving something that
cannot be run does not pass and is reported apart from that, and one serving a
working artifact passes. And the version a release's own run proves is read out
of the committed workflow rather than assumed — which is the only way to
establish it without waiting for a release to happen.
"""

from __future__ import annotations

import json
import subprocess
from collections.abc import Callable, Iterator
from pathlib import Path

import pytest
import yaml
from journey import REPO_ROOT, capture, clean_environment, output, plain, pythonpath
from release_artifacts.registries import (
    PRINTOBSERVER_PROOF_REGISTRIES,
    PRINTOBSERVER_PROOF_VERSION,
    UNREADABLE,
)
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run, start

#: The committed workflow this tier's triggers and version selection are read
#: out of, rather than restated here.
WORKFLOW = ".github/workflows/install-path.yml"

#: The recipe that proves every route, and the one per route beneath it.
TIER = "test-install-proof"
ROUTES = {
    "prove-registry-pypi": "pypi:printobserver-cli",
    "prove-registry-npm": "npm:printobserver-cli",
    "prove-registry-script": "release:printobserver",
}

#: A version the stand-in registries serve. Deliberately not the one the
#: workspace declares (`0.1.0`): what a user gets is what the registry serves,
#: and a proof that read this tree's number would pass over a registry serving
#: nothing.
SERVED = "0.3.0"

#: A version nothing serves, and no tree of this repository declares.
UNSERVED = "9.9.9"

#: The release a LATER run cut, which is the one a proof keyed on "the newest"
#: would reach for while the run it was keyed on went unproven.
LATER = "0.4.0"

#: How long one recipe is given: an install from a registry on loopback.
RECIPE_TIMEOUT_SECONDS = 600


class Standin:
    """The three registries, stood up by their own command."""

    def __init__(self, into: Path, *arguments: str) -> None:
        """Start them, and read the address they say they answer on."""
        self.process = start(
            [
                "uv",
                "run",
                "-q",
                "python",
                "-m",
                "release_artifacts",
                "standin",
                "--into",
                str(into),
                *arguments,
            ],
            cwd=REPO_ROOT,
            env=clean_environment(PYTHONPATH=pythonpath()),
        )
        said = self.process.stdout.readline() if self.process.stdout else ""
        if not said.strip():
            self.stop()
            message = f"the stand-in registries said nothing about where they answer: {said!r}"
            raise AssertionError(message)
        self.base = str(json.loads(said)["base"])

    def stop(self) -> None:
        """Close its input, which is how a consumer that is done stops it."""
        if self.process.stdin:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=60)
        except subprocess.TimeoutExpired:  # pragma: no cover - a hung bring-up
            self.process.kill()
            self.process.wait(timeout=60)


@pytest.fixture
def standing_in(tmp_path: Path) -> Iterator[Callable[..., Standin]]:
    """A factory for stand-in registries, stopped whatever the journey did."""
    started: list[Standin] = []

    def make(*arguments: str) -> Standin:
        registries = Standin(tmp_path / f"served{len(started)}", *arguments)
        started.append(registries)
        return registries

    try:
        yield make
    finally:
        for registries in started:
            registries.stop()


class Runs:
    """Two release-time runs, in a real repository carrying the tag each left."""

    def __init__(self, path: Path) -> None:
        """Make one commit per run, and tag it as release automation would."""
        self.path = path
        self.path.mkdir(parents=True, exist_ok=True)
        self._git("init", "--initial-branch", "main")
        self._git("config", "user.email", "release@example.invalid")
        self._git("config", "user.name", "release automation")
        #: The commit each run ran at, by the version it cut.
        self.at: dict[str, str] = {}
        for version in (SERVED, LATER):
            self._git("commit", "--allow-empty", "-m", f"chore: release v{version}")
            self._git("tag", f"v{version}")
            self.at[version] = self._git("rev-parse", "HEAD")

    def _git(self, *argv: str) -> str:
        """Run one git command here, or fail the journey saying what it said."""
        done = run(["git", *argv], cwd=self.path, timeout=60)
        truth(done.returncode == 0, describing=f"`git {' '.join(argv)}`:\n{done.stderr}")
        return done.stdout.strip()


@pytest.fixture
def runs(tmp_path: Path) -> Runs:
    """Two release-time runs, in a real git repository.

    A real one rather than a stand-in: what binds a release to the run that cut
    it is the tag release automation left at that run's own commit, and git is
    the only thing that can be asked about that.
    """
    return Runs(tmp_path / "checkout")


def _recipe(name: str, base: str, version: str = "") -> tuple[int, str]:
    """Run one recipe of this repository's own command surface against a stand-in."""
    result = capture(
        ["just", name],
        REPO_ROOT,
        timeout=RECIPE_TIMEOUT_SECONDS,
        env=clean_environment(
            **{
                PRINTOBSERVER_PROOF_REGISTRIES: base,
                PRINTOBSERVER_PROOF_VERSION: version,
            }
        ),
    )
    return result.returncode, output(result)


#: What `repo-policy.toml` declares about the release-time binding: which job
#: resolves the release its run cut, the output it publishes that under, and
#: the recipe it answers it with. Read rather than restated, because those three
#: are what `just check-repo` holds the workflow to.
BINDING = Repo(REPO_ROOT).policy["install_proof"]


def _release_time_version(checkout: Path, commit: str) -> str:
    """The version a release-time run proves, answered by the recipe that workflow runs.

    Read out of the committed workflow rather than assumed — which job resolves
    it, and that every job proving a route takes THAT job's answer — and then
    answered by driving the real recipe over a real repository. A journey that
    hard-coded the version would pass over a workflow that had stopped binding
    the proof to the run at all.
    """
    workflow = yaml.safe_load((REPO_ROOT / WORKFLOW).read_text(encoding="utf-8"))
    job = str(BINDING["release_job"])
    recipe = str(BINDING["release_recipe"])
    resolved = f"needs.{job}.outputs.{BINDING['release_output']}"
    ran = " ".join(str(step.get("run", "")) for step in workflow["jobs"][job]["steps"])
    contains(ran, f"just {recipe}", describing=f"what {WORKFLOW}'s `{job}` job resolves it with")
    for name, declared in workflow["jobs"].items():
        if any(
            f"just {proof}" in str(step.get("run", ""))
            for proof in ROUTES
            for step in declared["steps"]
        ):
            contains(
                " ".join(str((declared.get("env") or {})[PRINTOBSERVER_PROOF_VERSION]).split()),
                resolved,
                describing=f"the version job `{name}` proves",
            )

    result = capture(
        ["just", recipe, commit, str(checkout)],
        REPO_ROOT,
        timeout=RECIPE_TIMEOUT_SECONDS,
        env=clean_environment(PYTHONPATH=pythonpath()),
    )
    passing((result.returncode, output(result)), describing=f"`just {recipe}` over {checkout}")
    field = str(BINDING["release_output"])
    answered = [line for line in output(result).splitlines() if line.startswith(f"{field}=")]
    equal(len(answered), 1, describing=f"the one `{field}=` line a job reads its output from")
    # And nothing beside it, on either stream: a tool that worked says its one
    # line and stops, and the job appends that stream to its output file.
    equal(output(result).strip(), answered[0], describing=f"what a passing `just {recipe}` said")
    return answered[0].partition("=")[2].strip()


# llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
def test_the_release_time_trigger_cannot_fire_before_the_artifacts_are_published() -> None:
    """The release's own proof is keyed on the release workflow having finished.

    That workflow cuts the release in its `release` job and builds and publishes
    the artifacts in the two jobs after it, so a proof keyed on the release
    being published measures the version before it.
    """
    install = yaml.safe_load((REPO_ROOT / WORKFLOW).read_text(encoding="utf-8"))
    release = yaml.safe_load(
        (REPO_ROOT / ".github/workflows/release-plz.yml").read_text(encoding="utf-8")
    )

    # `on` is the YAML boolean, which is what a reader of this file trips over
    # once and then remembers: the key is spelled `on:` and parses as `True`.
    triggers = install[True]
    contains(
        triggers["workflow_run"]["workflows"],
        str(release["name"]),
        describing="the workflows this proof waits on",
    )
    truth("release" not in triggers, describing=f"{WORKFLOW} not to fire on a release")
    equal(release["jobs"]["publish"]["needs"], "artifacts", describing="what the publish awaits")
    equal(release["jobs"]["artifacts"]["needs"], "release", describing="what the build awaits")
    gate = f"needs.{BINDING['release_job']}.outputs.{BINDING['release_output']}"
    for name, job in install["jobs"].items():
        if name == BINDING["release_job"]:
            continue
        condition = " ".join(str(job.get("if", "")).split())
        contains(condition, gate, describing=f"what job `{name}` is gated on")
        # And not on the triggering run's conclusion, which would skip the one
        # run this tier exists for: the one whose release was cut and whose
        # publish then failed.
        truth(
            "workflow_run.conclusion" not in condition,
            describing=f"job `{name}` not to be gated on the run's conclusion: {condition}",
        )


@pytest.mark.parametrize("recipe", list(ROUTES))
def test_each_route_is_proven_against_what_its_registry_serves(
    recipe: str, standing_in: Callable[..., Standin]
) -> None:
    """The recipe installs what the registry served and runs what it installed."""
    registries = standing_in("--serves", SERVED)

    code, said = _recipe(recipe, registries.base)

    passing((code, said), describing=f"`just {recipe}` against a registry serving {SERVED}")
    contains(said, "SERVED AND PROVEN", describing=said)
    contains(said, f"printobserver {SERVED}", describing="what the installed program reported")
    contains(said, "Rust toolchain on the install path: none", describing=said)
    # A tool that worked says so and stops: one line on either stream, and no
    # echoed command line beside it. The whole report is what a FAILING proof
    # prints, which the journeys below read.
    equal(len(said.strip().splitlines()), 1, describing=f"what a passing `just {recipe}` said")


@pytest.mark.parametrize("recipe", list(ROUTES))
def test_a_registry_serving_nothing_for_the_version_under_test_does_not_pass(
    recipe: str, standing_in: Callable[..., Standin]
) -> None:
    """A publish that did not happen is reported as that, and it cannot pass."""
    registries = standing_in("--serves", SERVED)

    code, said = _recipe(recipe, registries.base, UNSERVED)

    failing((code, said), naming="NOT SERVED")
    equal(code, 3, describing="the exit a version nothing serves answers with")
    contains(said, "publish that did not happen", describing=said)
    contains(said, SERVED, describing="what the registry does serve")


def test_an_artifact_that_cannot_be_run_is_reported_apart_from_one_nothing_serves(
    standing_in: Callable[..., Standin],
) -> None:
    """Served and not proven is a build to repair, and it is a different answer.

    One route rather than three: what differs between them is the install, and
    this is about the answer a recipe gives once an install has happened —
    which `tools/release-artifacts/tests/test_registries.py` drives for all
    three without paying for three more installs here.
    """
    registries = standing_in("--broken", SERVED)

    code, said = _recipe("prove-registry-npm", registries.base)

    failing((code, said), naming="SERVED AND NOT PROVEN")
    equal(code, 1, describing="the exit a served artifact that does not work answers with")
    truth("NOT SERVED\n" not in said, describing=f"the two outcomes to be told apart: {said}")


def test_a_release_run_proves_the_release_it_cut_and_not_whatever_is_newest(
    runs: Runs, standing_in: Callable[..., Standin]
) -> None:
    """Two release-time runs, and the newer release is the other one's.

    Every push to the base branch finishes a `release-plz` run and all but the
    release ones cut nothing, so two runs in flight is ordinary. Keyed on the
    newest the forge lists, the earlier run reports green over an artifact it
    never looked at; keyed on the tag it left at its own commit, it proves its
    own release.
    """
    version = _release_time_version(runs.path, runs.at[SERVED])
    registries = standing_in("--serves", SERVED, "--serves", LATER)

    equal(version, SERVED, describing="the release the run this proof is keyed on cut")
    code, said = _recipe("prove-registry-npm", registries.base, version)

    passing((code, said), describing="the proof of that run's own release")
    contains(said, f"printobserver {SERVED}", describing=said)
    truth(LATER not in said, describing=f"another run's release to go unproven here: {said}")


def test_a_release_run_whose_publish_failed_is_an_observable_failure(
    runs: Runs, standing_in: Callable[..., Standin]
) -> None:
    """The release is cut and the publish after it fails: the state this tier is for.

    Gated on the triggering run's conclusion, that run is skipped and the
    missing publish is reported by nothing at all. Keyed on the release it cut,
    the registries' silence is named — as a publish that did not happen rather
    than as an artifact that does not work.
    """
    version = _release_time_version(runs.path, runs.at[LATER])
    registries = standing_in("--serves", SERVED, "--release", f"v{LATER}")

    equal(version, LATER, describing="the release the failed run had already cut")
    code, said = _recipe("prove-registry-pypi", registries.base, version)

    failing((code, said), naming="NOT SERVED")
    contains(said, LATER, describing="the release the proof was keyed on")
    contains(said, "publish that did not happen", describing=said)


def test_the_tier_recipe_proves_every_route(standing_in: Callable[..., Standin]) -> None:
    """A run of the tier by hand takes all three routes, not one of them."""
    registries = standing_in("--serves", SERVED)

    code, said = _recipe(TIER, registries.base)

    passing((code, said), describing=f"`just {TIER}`")
    for identifier in ROUTES.values():
        contains(plain(said), f"{identifier}: SERVED AND PROVEN", describing=said)
    # Three routes, three lines, and nothing else: a tier of proofs that all
    # passed is as quiet as the three recipes it is made of.
    equal(len(said.strip().splitlines()), len(ROUTES), describing=f"what `just {TIER}` said")


def test_an_artifact_reporting_another_version_does_not_pass(
    standing_in: Callable[..., Standin],
) -> None:
    """Installing is not enough: what the program says it is has to be what was proven.

    The registry here serves the version under test and the package carries a
    program that reports another, which is what a distribution built from the
    wrong commit looks like from the outside.
    """
    registries = standing_in("--mislabelled", f"{SERVED}=0.1.0")

    code, said = _recipe("prove-registry-pypi", registries.base)

    failing((code, said), naming="not the version under test")
    equal(code, 1, describing="the exit an artifact that is not what it says answers with")


def test_a_registry_that_cannot_be_reached_is_neither_outcome() -> None:
    """An unreachable registry is a network, not a missing publish or a bad build.

    Reported as `NOT SERVED` it would send a reader to repair a release that is
    fine, so the recipe stops naming the registry instead.
    """
    code, said = _recipe("prove-registry-npm", "http://127.0.0.1:1")

    failing((code, said), naming="could not be reached")
    equal(code, UNREADABLE, describing="the exit a registry nothing could read answers with")
    truth("NOT SERVED" not in said, describing=f"neither outcome to be reported: {said}")


def test_the_stand_in_refuses_a_version_it_cannot_serve(tmp_path: Path) -> None:
    """Everything a caller names it reaches a manifest, a file name and a path."""
    result = capture(
        [
            "uv",
            "run",
            "-q",
            "python",
            "-m",
            "release_artifacts",
            "standin",
            "--serves",
            "the newest one",
            "--into",
            str(tmp_path / "refused"),
        ],
        REPO_ROOT,
        timeout=RECIPE_TIMEOUT_SECONDS,
        env=clean_environment(PYTHONPATH=pythonpath()),
    )

    failing((result.returncode, output(result)), naming="is not a version to serve")
