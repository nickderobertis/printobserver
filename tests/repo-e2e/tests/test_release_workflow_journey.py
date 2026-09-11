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
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import json
import shutil
from collections.abc import Callable
from pathlib import Path

import pytest
from actions import Result, Runner, WorkflowRun
from journey import REPO_ROOT, GateCopy, clean_environment
from release_artifacts.registries import RELEASE_ANSWER_SAMPLE
from repo_checks.expect import absent, contains, equal, truth

WORKFLOW = ".github/workflows/release-plz.yml"

#: The jobs, by the key the committed workflow gives each.
DRAFTING = "release-pr"
PUBLISHING = "release"
ARTIFACTS = "artifacts"
PUBLISH = "publish"

#: The recipes the two downstream jobs run, recorded rather than run.
RECORDED = ("bootstrap", "build-artifacts", "publish-artifacts")

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

RELEASE_PLZ_STANDIN = """#!/bin/sh
# A stand-in for the release program: what it would say, and none of what it
# would reach. `release-pr` fails the way the wedged one did; `release` prints
# the answer the journey put in RELEASE_PLZ_STANDIN_ANSWER, and only when asked
# for it with `--output json`, as the real one prints nothing otherwise.
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

JUST_STANDIN = """#!/bin/sh
# The real `just`, except for the recipes RELEASE_STANDIN_RECORDED names, which
# are written to RELEASE_STANDIN_RECORD instead of run.
for recorded in $RELEASE_STANDIN_RECORDED; do
  if [ "$1" = "$recorded" ]; then
    printf '%s\\n' "$1" >> "$RELEASE_STANDIN_RECORD"
    exit 0
  fi
done
exec "$RELEASE_STANDIN_REAL_JUST" "$@"
"""


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


def released(
    gate_copy: Callable[..., GateCopy],
    tmp_path: Path,
    answered: str,
    *edits: tuple[str, str],
) -> tuple[WorkflowRun, list[str]]:
    """Run the workflow over a copy carrying `edits`, the release program answering `answered`.

    Returns the run, and the downstream recipes that were reached, in order.
    """
    copy = gate_copy()
    for old, new in edits:
        copy.edit(WORKFLOW, old, new)

    stand_ins = tmp_path / "stand-ins"
    stand_ins.mkdir()
    for name, text in (("release-plz", RELEASE_PLZ_STANDIN), ("just", JUST_STANDIN)):
        program = stand_ins / name
        program.write_text(text, encoding="utf-8")
        program.chmod(0o755)
    answer_file = tmp_path / "answer.json"
    answer_file.write_text(answered, encoding="utf-8")
    record = tmp_path / "reached"
    record.touch()
    real_just = shutil.which("just")
    truth(real_just is not None, describing="`just` to be on the PATH")

    runner = Runner(
        copy.root / WORKFLOW,
        copy.root,
        path_first=stand_ins,
        env=clean_environment(
            UV_PROJECT_ENVIRONMENT=str(copy.shared_venv),
            RELEASE_PLZ_STANDIN_ANSWER=str(answer_file),
            RELEASE_STANDIN_RECORD=str(record),
            RELEASE_STANDIN_RECORDED=" ".join(RECORDED),
            RELEASE_STANDIN_REAL_JUST=str(real_just),
        ),
    )
    run = runner.run()
    return run, record.read_text(encoding="utf-8").split()


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
