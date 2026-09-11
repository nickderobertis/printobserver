"""The committed release workflow, run: what its wiring schedules and what it skips.

`test_release_gating_journey.py` proves the answer reader and the check that
reads the workflow. Neither proves the workflow's own scheduling — that a
drafting failure leaves the publishing job free to run, that an empty answer
skips the artifact build and the artifact publish and a non-empty one does not
— because that is decided by the forge's rules over the workflow's text. So
these journeys run the committed workflow under those rules (`actions.py`),
in a copy of the tree, substituting only what reaches outside this host:

  * `release-plz`, which would reach a forge and a registry, is a stand-in
    whose `release-pr` fails the way the wedged one did and whose `release`
    answers whatever the journey says was released — and only when asked
    with `--output json`, as the real one does;
  * `just bootstrap`, `just build-artifacts` and `just publish-artifacts` are
    recorded rather than run, because the question is whether the build and
    the publish are REACHED, and building the workspace in release mode
    twice over to answer it would prove nothing more. Every other recipe —
    `just release-answer` above all — is the real one over the copy.

Each defect the gating rule refuses is then run rather than read: the
publishing job chained behind drafting publishes nothing whatever was cut, and
a downstream job with its gate removed runs on a push that cut nothing.

The workflow's second shape is run the same way. The copy is a git repository
already, and its commit is tagged `v<X>` where `X` is its own workspace version,
so `just release-dispatched` runs for real against a real tag: a dispatch
naming it builds and publishes, one naming a tag the copy does not carry or a
tag over the wrong tree fails at `release` with nothing after it reached, and
the record a dispatch uploads is what the install-path workflow — run under a
`workflow_run` event over the same artifact store — resolves the version from.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import json
import shutil
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import pytest
from actions import ArtifactStore, Event, Result, Runner, WorkflowRun
from journey import REPO_ROOT, GateCopy, clean_environment
from release_artifacts import targets
from release_artifacts.publishing import PRINTOBSERVER_PUBLISH_VERSION
from release_artifacts.registries import (
    PRINTOBSERVER_PROOF_VERSION,
    RELEASE_ANSWER_SAMPLE,
    RELEASED_FIELD,
    VERSION_FIELD,
)
from repo_checks.expect import absent, contains, equal, truth
from repo_checks.model import Repo
from repo_checks.shell import run as shell_run

WORKFLOW = ".github/workflows/release-plz.yml"
PROOF_WORKFLOW = ".github/workflows/install-path.yml"

#: The jobs, by the key the committed workflow gives each.
DRAFTING = "release-pr"
PUBLISHING = "release"
ARTIFACTS = "artifacts"
PUBLISH = "publish"

#: The install-path workflow's jobs a journey drives: the one resolving the
#: version, and the three proving a route with it. The three installing a route
#: are left out by name, because their steps reach the real registries.
RESOLVE = "resolve"
PROVING = ("prove-registry-pypi", "prove-registry-npm", "prove-registry-script")

#: The recipes the two downstream jobs run, recorded rather than run — and the
#: three route proofs, which would otherwise read the real registries.
RECORDED = ("bootstrap", "build-artifacts", "publish-artifacts", *PROVING)

#: What `repo-policy.toml` declares the record crosses between the two
#: workflows as. Read rather than restated: it is what `just check-repo` holds
#: both workflows to.
RECORD_ARTIFACT = str(Repo(REPO_ROOT).policy["release"]["record_artifact"])

#: A version no tree of this repository declares, tagged at the copy's commit
#: for the dispatch that names an existing tag over the wrong tree.
MISMATCHED = "9.9.9"

#: The two boundaries a journey reads the inputs of.
CHECKOUT = "actions/checkout@"
DOWNLOAD = "actions/download-artifact@"

#: The edits that put each defect the `release-gating` rule refuses back.
CHAINED_BEHIND_DRAFTING = (
    "  release:\n    name: release\n",
    "  release:\n    name: release\n    needs: release-pr\n",
)
NO_ANSWER_ASKED_FOR = ("--output json > ", "> ")
ARTIFACTS_UNGATED = (
    "    if: needs.release.outputs.released != ''\n    strategy:\n",
    "    strategy:\n",
)
PUBLISH_UNGATED = (
    "    needs: [release, artifacts]\n    if: needs.release.outputs.released != ''\n",
    "    needs: [release, artifacts]\n",
)

# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
RELEASE_PLZ_STANDIN = """#!/bin/sh
# A stand-in for the release program: what it would say, and none of what it
# would reach. `release-pr` fails the way the wedged one did; `release` prints
# the answer the journey put in RELEASE_PLZ_STANDIN_ANSWER, and only when asked
# for it with `--output json`, as the real one prints nothing otherwise. Every
# invocation is written to RELEASE_PLZ_STANDIN_RECORD, so a journey can say
# the program was never reached.
printf '%s\\n' "$*" >> "$RELEASE_PLZ_STANDIN_RECORD"
case "$1" in
  release-pr)
    echo "ERROR failed to determine next versions: package \\`printobserver-sdk\\` not found" \\
      "in the registry, but the git tag v0.1.0 exists" >&2
    exit 1
    ;;
  release)
    previous=""
    for argument in "$@"; do
      if [ "$previous" = "--output" ] || [ "$previous" = "-o" ]; then
        if [ "$argument" = "json" ]; then cat "$RELEASE_PLZ_STANDIN_ANSWER"; fi
      fi
      previous="$argument"
    done
    exit 0
    ;;
  *)
    echo "stand-in release-plz: not asked for: $*" >&2
    exit 64
    ;;
esac
"""

# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
JUST_STANDIN = """#!/bin/sh
# The real `just`, except for the recipes RELEASE_STANDIN_RECORDED names, which
# are written to RELEASE_STANDIN_RECORD instead of run — each with the version
# the publish recipe and the route proofs were handed, so a journey can say
# which version a run would have published or proven.
for recorded in $RELEASE_STANDIN_RECORDED; do
  if [ "$1" = "$recorded" ]; then
    printf '%s\\t%s\\t%s\\n' "$1" "${PRINTOBSERVER_PUBLISH_VERSION:-}" \\
      "${PRINTOBSERVER_PROOF_VERSION:-}" >> "$RELEASE_STANDIN_RECORD"
    exit 0
  fi
done
exec "$RELEASE_STANDIN_REAL_JUST" "$@"
"""


@dataclass(frozen=True, slots=True)
class Recorded:
    """One recorded recipe: its name, and the versions it was handed."""

    recipe: str
    publish_version: str
    proof_version: str


@dataclass(frozen=True, slots=True)
class Driven:
    """One workflow run over a copy, and everything the stand-ins recorded of it."""

    run: WorkflowRun
    copy: GateCopy
    #: The store the run's artifacts went to, keyed by `run_id`.
    store: ArtifactStore
    run_id: str
    #: Every recorded recipe, in the order reached.
    recorded: list[Recorded]
    #: Every invocation of the release program, as its argument list.
    invoked: list[str]

    @property
    def reached(self) -> list[str]:
        """The recorded recipes' names, in order."""
        return [entry.recipe for entry in self.recorded]

    def handed(self, recipe: str) -> list[Recorded]:
        """Every record of one recipe."""
        return [entry for entry in self.recorded if entry.recipe == recipe]


def answer(*versions: str) -> str:
    """What `release-plz release --output json` answers having released `versions`."""
    recorded = json.loads((REPO_ROOT / RELEASE_ANSWER_SAMPLE).read_text(encoding="utf-8"))
    entry = recorded["releases"][0]
    return json.dumps(
        {
            "releases": [
                {**entry, "package_name": f"printobserver-{index}", "tag": f"v{v}", "version": v}
                for index, v in enumerate(versions)
            ]
        }
    )


def tagged(copy: GateCopy, *, mismatched: bool = True) -> str:
    """Tag the copy's commit as release automation tagged its own version's release.

    Answers that version, `X`: the copy's workspace's own, so that the tag
    names the tree it is at. `v9.9.9` is tagged beside it unless `mismatched`
    is off, for the dispatch that names an existing tag whose tree is another
    release's — and a push-shaped run reading the version off the tag at its
    commit needs that commit under one tag alone.
    """
    version = targets.workspace(copy.root)["version"]
    tags = [f"v{version}", f"v{MISMATCHED}"] if mismatched else [f"v{version}"]
    for tag in tags:
        shell_run(["git", "tag", tag], cwd=copy.root, check=True)
    return version


def head(copy: GateCopy) -> str:
    """The commit the copy is at."""
    return shell_run(["git", "rev-parse", "HEAD"], cwd=copy.root, check=True).stdout.strip()


# llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
def driven(
    copy: GateCopy,
    tmp_path: Path,
    workflow: str,
    *,
    answered: str = "",
    event: Event | None = None,
    store: ArtifactStore | None = None,
    run_id: str = "1",
    only: set[str] | None = None,
) -> Driven:
    """Run one committed workflow over `copy` under `event`, the stand-ins first on the PATH.

    `answered` is what the release program answers it released, where the run
    asks it; `store` is the artifact store the run shares with another, or one
    of its own.
    """
    stand_ins = tmp_path / f"stand-ins-{run_id}"
    stand_ins.mkdir()
    for name, text in (("release-plz", RELEASE_PLZ_STANDIN), ("just", JUST_STANDIN)):
        program = stand_ins / name
        program.write_text(text, encoding="utf-8")
        program.chmod(0o755)
    answer_file = stand_ins / "answer.json"
    answer_file.write_text(answered, encoding="utf-8")
    record = stand_ins / "reached"
    record.touch()
    invocations = stand_ins / "invoked"
    invocations.touch()
    real_just = shutil.which("just")
    truth(real_just is not None, describing="`just` to be on the PATH")
    store = store or ArtifactStore(tmp_path / "store")

    runner = Runner(
        copy.root / workflow,
        copy.root,
        path_first=stand_ins,
        env=clean_environment(
            UV_PROJECT_ENVIRONMENT=str(copy.shared_venv),
            RELEASE_PLZ_STANDIN_ANSWER=str(answer_file),
            RELEASE_PLZ_STANDIN_RECORD=str(invocations),
            RELEASE_STANDIN_RECORD=str(record),
            RELEASE_STANDIN_RECORDED=" ".join(RECORDED),
            RELEASE_STANDIN_REAL_JUST=str(real_just),
        ),
        event=event,
        artifacts=store,
        run_id=run_id,
    )
    run = runner.run(only)
    recorded = [
        Recorded(*line.split("\t"))
        for line in record.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    invoked = [line for line in invocations.read_text(encoding="utf-8").splitlines() if line]
    return Driven(run, copy, store, run_id, recorded, invoked)


def released(
    gate_copy: Callable[..., GateCopy],
    tmp_path: Path,
    answered: str,
    *edits: tuple[str, str],
) -> tuple[WorkflowRun, list[str]]:
    """Run the release workflow on a push over a copy carrying `edits`.

    Returns the run, and the downstream recipes that were reached, in order.
    """
    copy = gate_copy()
    for old, new in edits:
        copy.edit(WORKFLOW, old, new)
    done = driven(copy, tmp_path, WORKFLOW, answered=answered)
    return done.run, done.reached


def dispatched(
    gate_copy: Callable[..., GateCopy], tmp_path: Path, tag: str = "", *, run_id: str = "1"
) -> tuple[Driven, str]:
    """Run the release workflow dispatched with `tag`, over a copy tagged at its own version.

    `tag` names the copy's own tag, `v<X>`, when empty. Returns the run and
    the copy's version, `X`.
    """
    copy = gate_copy()
    version = tagged(copy)
    event = Event("workflow_dispatch", inputs={"tag": tag or f"v{version}"})
    return driven(copy, tmp_path, WORKFLOW, event=event, run_id=run_id), version


def proof_after(
    done: Driven, tmp_path: Path, *, event: str, run_id: str, head_sha: str = ""
) -> Driven:
    """Run the install-path workflow as the forge runs it after `done` finished.

    Under a `workflow_run` event whose `event` is what fired `done`, whose
    `id` is `done`'s, and over the same artifact store — which is the seam the
    record crosses. The route-installing jobs are left out by name.
    """
    triggered = Event(
        "workflow_run",
        workflow_run={"event": event, "id": done.run_id, "head_sha": head_sha or head(done.copy)},
    )
    return driven(
        done.copy,
        tmp_path,
        PROOF_WORKFLOW,
        event=triggered,
        store=done.store,
        run_id=run_id,
        only={RESOLVE, *PROVING},
    )


def results(run: WorkflowRun) -> dict[str, str]:
    """Every job's result, as the forge would report the run."""
    return {name: str(job.result) for name, job in run.jobs.items()}


def test_a_drafting_failure_does_not_stop_a_cut_release_from_reaching_its_artifacts(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """Drafting fails, publishing runs, and a non-empty answer reaches the build and the publish."""
    run, reached = released(gate_copy, tmp_path, answer("0.4.0", "0.4.0"))

    equal(
        results(run),
        {
            DRAFTING: Result.FAILURE,
            PUBLISHING: Result.SUCCESS,
            ARTIFACTS: Result.SUCCESS,
            PUBLISH: Result.SUCCESS,
        },
        describing="what the forge would report each job as",
    )
    equal(
        run.jobs[PUBLISHING].outputs, {"released": "v0.4.0"}, describing="what `release` published"
    )
    contains(
        run.commands(PUBLISHING),
        'just release-answer "$RUNNER_TEMP/released.json" >> "$GITHUB_OUTPUT"',
    )
    equal(reached.count("publish-artifacts"), 1, describing=f"the publishes reached: {reached}")
    truth(reached.count("build-artifacts") >= 1, describing=f"the builds reached: {reached}")


def test_a_push_runs_nothing_of_the_dispatched_shape(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The dispatched shape adds no step to a push: no verification, no record, no version."""
    copy = gate_copy()
    tagged(copy)

    done = driven(copy, tmp_path, WORKFLOW, answered=answer("0.4.0"))

    equal(done.run.result(PUBLISH), Result.SUCCESS, describing="the publish on a push")
    absent(
        done.run.commands(PUBLISHING),
        'just release-dispatched "$TAG" . "$RUNNER_TEMP/dispatched-release/version" '
        '>> "$GITHUB_OUTPUT"',
        describing="the verifying step, which a push must not run",
    )
    equal(done.store.names(done.run_id), [], describing="what a push uploaded to the store")
    equal(
        [entry.publish_version for entry in done.handed("publish-artifacts")],
        [""],
        describing=f"the {PRINTOBSERVER_PUBLISH_VERSION} a push hands the publisher",
    )
    equal(
        done.run.jobs[ARTIFACTS].boundary(CHECKOUT).given.get("ref"),
        "",
        describing="the ref a push's build checks out: the run's own commit",
    )
    truth(
        any(line.startswith("release ") for line in done.invoked),
        describing=f"the release program to have been invoked on a push: {done.invoked}",
    )


def test_a_push_that_cut_nothing_builds_and_publishes_nothing(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """Publishing succeeds having released nothing, and the two jobs after it are skipped."""
    run, reached = released(gate_copy, tmp_path, answer())

    equal(
        results(run),
        {
            DRAFTING: Result.FAILURE,
            PUBLISHING: Result.SUCCESS,
            ARTIFACTS: Result.SKIPPED,
            PUBLISH: Result.SKIPPED,
        },
        describing="what the forge would report each job as",
    )
    equal(run.jobs[PUBLISHING].outputs, {"released": ""}, describing="what `release` published")
    equal(reached, [], describing="the downstream recipes reached on a push that cut nothing")


def test_a_release_step_that_is_not_asked_for_its_answer_fails_rather_than_skipping(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """Without `--output json` the answer file is empty, and the reader fails the job by name."""
    run, reached = released(gate_copy, tmp_path, answer("0.4.0"), NO_ANSWER_ASKED_FOR)

    equal(run.result(PUBLISHING), Result.FAILURE, describing="the publishing job's result")
    contains(run.jobs[PUBLISHING].steps[-1].output, "not JSON", describing="what failed it")
    equal(run.result(ARTIFACTS), Result.SKIPPED, describing="the build's result")
    equal(run.result(PUBLISH), Result.SKIPPED, describing="the publish's result")
    equal(reached, [], describing="the downstream recipes reached")


def test_a_publishing_job_chained_behind_drafting_publishes_nothing_whatever_was_cut(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The state this repair undid, run: drafting fails, and nothing after it ever runs."""
    run, reached = released(gate_copy, tmp_path, answer("0.4.0"), CHAINED_BEHIND_DRAFTING)

    equal(
        results(run),
        {
            DRAFTING: Result.FAILURE,
            PUBLISHING: Result.SKIPPED,
            ARTIFACTS: Result.SKIPPED,
            PUBLISH: Result.SKIPPED,
        },
        describing="what the forge would report each job as",
    )
    equal(reached, [], describing="the downstream recipes reached")


@pytest.mark.parametrize(
    ("edits", "expected", "reaches_publish"),
    [
        # The build's gate removed: the build runs on a push that cut nothing,
        # and the publish's own gate is what still holds.
        (
            (ARTIFACTS_UNGATED,),
            {ARTIFACTS: Result.SUCCESS, PUBLISH: Result.SKIPPED},
            False,
        ),
        # The publish's gate removed alone: the build's gate still skips the
        # build, and a skipped dependency skips the publish with it.
        (
            (PUBLISH_UNGATED,),
            {ARTIFACTS: Result.SKIPPED, PUBLISH: Result.SKIPPED},
            False,
        ),
        # Both removed: the publish runs on a push that cut nothing, which is
        # the push the registries refuse and the base branch goes red on.
        (
            (ARTIFACTS_UNGATED, PUBLISH_UNGATED),
            {ARTIFACTS: Result.SUCCESS, PUBLISH: Result.SUCCESS},
            True,
        ),
    ],
)
def test_a_downstream_job_without_its_gate_runs_on_a_push_that_cut_nothing(
    edits: tuple[tuple[str, str], ...],
    expected: dict[str, Result],
    reaches_publish: bool,
    gate_copy: Callable[..., GateCopy],
    tmp_path: Path,
) -> None:
    """Each gate removed in turn, over an empty answer: what then runs is what the gate stopped."""
    run, reached = released(gate_copy, tmp_path, answer(), *edits)

    equal(run.result(PUBLISHING), Result.SUCCESS, describing="the publishing job's result")
    equal(
        {job: str(run.result(job)) for job in expected},
        {job: str(result) for job, result in expected.items()},
        describing="what the two downstream jobs do without the gate",
    )
    if reaches_publish:
        contains(reached, "publish-artifacts", describing=f"the recipes reached: {reached}")
    else:
        absent(reached, "publish-artifacts", describing=f"the recipes reached: {reached}")


def test_a_dispatch_naming_an_existing_tag_builds_its_tree_and_publishes_its_version(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The second shape, run: verified at `release`, built from the tag, published as the tag.

    The release program is never invoked; the build checks out the tag; the
    publisher at the dispatched ref is handed the tag's version; and the
    record the install-path proof reads is in the store, under the name both
    workflows are held to.
    """
    done, version = dispatched(gate_copy, tmp_path, run_id="21")
    tag = f"v{version}"

    equal(
        results(done.run),
        {
            DRAFTING: Result.SKIPPED,
            PUBLISHING: Result.SUCCESS,
            ARTIFACTS: Result.SUCCESS,
            PUBLISH: Result.SUCCESS,
        },
        describing="what the forge would report each job as",
    )
    equal(
        done.run.jobs[PUBLISHING].outputs,
        {RELEASED_FIELD: tag},
        describing="what `release` answered the run publishes",
    )
    equal(done.invoked, [], describing="the release program's invocations on a dispatch")
    equal(
        done.run.jobs[PUBLISHING].boundary(CHECKOUT).given.get("fetch-depth"),
        "0",
        describing="the verifying job's checkout: the whole history and its tags",
    )
    equal(
        done.run.jobs[ARTIFACTS].boundary(CHECKOUT).given.get("ref"),
        tag,
        describing="the tree the build checks out",
    )
    equal(
        done.run.jobs[PUBLISH].boundary(CHECKOUT).given.get("ref"),
        None,
        describing="the publisher's checkout: the dispatched ref, with no `ref` given",
    )
    truth(done.reached.count("build-artifacts") >= 1, describing=f"the builds: {done.reached}")
    equal(
        [entry.publish_version for entry in done.handed("publish-artifacts")],
        [tag],
        describing=f"the {PRINTOBSERVER_PUBLISH_VERSION} the one publish was handed",
    )
    equal(done.store.names(done.run_id), [RECORD_ARTIFACT], describing="what the run uploaded")
    equal(
        done.store.read(done.run_id, RECORD_ARTIFACT, "version"),
        f"{VERSION_FIELD}={version}\n",
        describing="the record the install-path proof reads",
    )


def test_a_dispatch_naming_a_tag_the_checkout_does_not_carry_is_refused_at_release(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """Nothing is built or published for a tag nothing tagged, and no record is uploaded."""
    done, _ = dispatched(gate_copy, tmp_path, "v0.0.1", run_id="22")

    equal(
        results(done.run),
        {
            DRAFTING: Result.SKIPPED,
            PUBLISHING: Result.FAILURE,
            ARTIFACTS: Result.SKIPPED,
            PUBLISH: Result.SKIPPED,
        },
        describing="what the forge would report each job as",
    )
    refused = done.run.jobs[PUBLISHING].steps[-1].output
    contains(refused, "v0.0.1", describing=f"the tag named: {refused}")
    contains(refused, "existing release tag", describing="what a dispatch names")
    equal(done.reached, [], describing="the downstream recipes reached")
    equal(done.invoked, [], describing="the release program's invocations")
    equal(done.store.names(done.run_id), [], describing="what the run uploaded")


def test_a_dispatch_naming_a_tag_over_another_releases_tree_is_refused_at_release(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """A tag that exists over a tree declaring another version names both and stops there."""
    done, version = dispatched(gate_copy, tmp_path, f"v{MISMATCHED}", run_id="23")

    equal(done.run.result(PUBLISHING), Result.FAILURE, describing="the verifying job's result")
    refused = done.run.jobs[PUBLISHING].steps[-1].output
    contains(refused, MISMATCHED, describing=f"the version the tag names: {refused}")
    contains(refused, version, describing="the version the tree declares")
    equal(done.run.result(ARTIFACTS), Result.SKIPPED, describing="the build's result")
    equal(done.run.result(PUBLISH), Result.SKIPPED, describing="the publish's result")
    equal(done.reached, [], describing="the downstream recipes reached")
    equal(done.store.names(done.run_id), [], describing="what the run uploaded")


def test_the_proof_after_a_dispatched_run_proves_the_version_that_run_recorded(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The seam between the two workflows: the record uploaded is the version proven."""
    done, version = dispatched(gate_copy, tmp_path, run_id="24")

    proof = proof_after(done, tmp_path, event="workflow_dispatch", run_id="25")

    equal(proof.run.result(RESOLVE), Result.SUCCESS, describing="the resolving job")
    equal(
        proof.run.jobs[RESOLVE].outputs,
        {VERSION_FIELD: version},
        describing="the version `resolve` answered off the record",
    )
    equal(
        proof.run.jobs[RESOLVE].boundary(DOWNLOAD).given.get("name"),
        RECORD_ARTIFACT,
        describing="the artifact `resolve` downloaded",
    )
    equal(
        proof.run.jobs[RESOLVE].boundary(DOWNLOAD).given.get("run-id"),
        done.run_id,
        describing="the run `resolve` downloaded it from",
    )
    absent(
        proof.run.commands(RESOLVE),
        'just release-version "$COMMIT" . >> "$GITHUB_OUTPUT"',
        describing="the push-shaped resolution, which a dispatched run does not take",
    )
    for job in PROVING:
        equal(proof.run.result(job), Result.SUCCESS, describing=f"the `{job}` job")
        equal(
            [entry.proof_version for entry in proof.handed(job)],
            [version, version],
            describing=f"the {PRINTOBSERVER_PROOF_VERSION} each cell of `{job}` proves",
        )


def test_the_proof_after_a_push_shaped_run_reads_the_version_off_the_tag_as_before(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """After a push-triggered run at a tagged commit, `resolve` answers off the tag."""
    copy = gate_copy()
    version = tagged(copy, mismatched=False)
    done = driven(copy, tmp_path, WORKFLOW, answered=answer(version), run_id="26")

    proof = proof_after(done, tmp_path, event="push", run_id="27", head_sha=head(copy))

    equal(proof.run.result(RESOLVE), Result.SUCCESS, describing="the resolving job")
    equal(
        proof.run.jobs[RESOLVE].outputs,
        {VERSION_FIELD: version},
        describing="the version `resolve` read off the tag",
    )
    truth(
        not any(b.uses.startswith(DOWNLOAD) for b in proof.run.jobs[RESOLVE].boundaries),
        describing="no record downloaded after a push",
    )
    for job in PROVING:
        equal(
            [entry.proof_version for entry in proof.handed(job)],
            [version, version],
            describing=f"the {PRINTOBSERVER_PROOF_VERSION} each cell of `{job}` proves",
        )


def test_the_proof_after_a_refused_dispatch_fails_naming_the_missing_record(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """A dispatch refused at `release` leaves no record: one red run beside the red dispatch."""
    done, _ = dispatched(gate_copy, tmp_path, "v0.0.1", run_id="28")

    proof = proof_after(done, tmp_path, event="workflow_dispatch", run_id="29")

    equal(proof.run.result(RESOLVE), Result.FAILURE, describing="the resolving job")
    failed = proof.run.jobs[RESOLVE].steps[-1]
    contains(failed.output, RECORD_ARTIFACT, describing=f"the missing artifact named: {failed}")
    equal(proof.run.jobs[RESOLVE].outputs, {}, describing="no version for the proofs to take")
    for job in PROVING:
        equal(proof.run.result(job), Result.SKIPPED, describing=f"the `{job}` job")
    equal(proof.reached, [], describing="the route proofs reached")
