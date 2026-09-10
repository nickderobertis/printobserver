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
    RELEASE,
)
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.shell import start

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


def _selected_by_a_release_run() -> str:
    """What the committed workflow selects as the version a release's own run proves.

    Read out of the workflow rather than assumed: the whole point of that
    expression is that a release-time run proves *that release*, and a journey
    that hard-coded the answer would pass over a workflow that had stopped
    saying it.
    """
    workflow = yaml.safe_load((REPO_ROOT / WORKFLOW).read_text(encoding="utf-8"))
    stated = " ".join(str(workflow["env"][PRINTOBSERVER_PROOF_VERSION]).split())
    truth(
        "github.event_name == 'workflow_run'" in stated,
        describing=f"{WORKFLOW} to select the version by which trigger fired: {stated}",
    )
    for word in stated.replace("'", " ' ").split():
        if word == RELEASE:
            return RELEASE
    message = f"{WORKFLOW}'s version selection names no release selector: {stated}"
    raise AssertionError(message)


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
    for job in install["jobs"].values():
        contains(
            " ".join(str(job.get("if", "")).split()),
            "github.event.workflow_run.conclusion == 'success'",
            describing="the condition every job of this proof carries",
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


@pytest.mark.parametrize("recipe", list(ROUTES))
def test_an_artifact_that_cannot_be_run_is_reported_apart_from_one_nothing_serves(
    recipe: str, standing_in: Callable[..., Standin]
) -> None:
    """Served and not proven is a build to repair, and it is a different answer."""
    registries = standing_in("--broken", SERVED)

    code, said = _recipe(recipe, registries.base)

    failing((code, said), naming="SERVED AND NOT PROVEN")
    equal(code, 1, describing="the exit a served artifact that does not work answers with")
    truth("NOT SERVED\n" not in said, describing=f"the two outcomes to be told apart: {said}")


def test_a_release_run_proves_the_release_rather_than_whatever_is_newest(
    standing_in: Callable[..., Standin],
) -> None:
    """Driven with what the committed workflow selects on its release-time trigger.

    The stand-in forge lists a release no registry serves, which is what a
    release whose publish did not happen looks like from the outside — and the
    proof reports it rather than falling back on something that does install.
    """
    selector = _selected_by_a_release_run()
    registries = standing_in("--serves", SERVED, "--release", f"v{UNSERVED}")

    code, said = _recipe("prove-registry-pypi", registries.base, selector)

    failing((code, said), naming="NOT SERVED")
    contains(said, UNSERVED, describing="the release the proof was keyed on")
    contains(said, "the newest release the forge published", describing=said)


def test_a_release_run_passes_over_the_release_the_forge_published(
    standing_in: Callable[..., Standin],
) -> None:
    """And the same selector passes where that release is the one served."""
    selector = _selected_by_a_release_run()
    registries = standing_in("--serves", SERVED)

    code, said = _recipe("prove-registry-npm", registries.base, selector)

    passing((code, said), describing="the proof of the release the forge published")
    contains(said, f"printobserver {SERVED}", describing=said)


def test_the_tier_recipe_proves_every_route(standing_in: Callable[..., Standin]) -> None:
    """A run of the tier by hand takes all three routes, not one of them."""
    registries = standing_in("--serves", SERVED)

    code, said = _recipe(TIER, registries.base)

    passing((code, said), describing=f"`just {TIER}`")
    for identifier in ROUTES.values():
        contains(plain(said), f"{identifier}: SERVED AND PROVEN", describing=said)
