"""The rules `actions.py` runs a workflow under are the forge's, and nothing past them is guessed.

The release-workflow journey rests on this runner, so what it claims about the
forge is pinned here over a workflow small enough to read: a job waits on what
it `needs` and is skipped when that failed or was skipped; a job's `if:` reads
another job's outputs and skips it when false; a step's `if:` skips the step
and its `id` then answers nothing; a step's `GITHUB_OUTPUT` lines become the
job's outputs; the event a run is under is what `github` and `inputs` answer;
a `uses:` boundary is recorded with the inputs it was given; the two artifact
actions move files through a store keyed by run id, and a download of what
nothing uploaded fails the step; a caller's set of jobs to run skips the rest
by name; and a construct outside the modelled set is refused by name rather
than run as something else.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from pathlib import Path

import pytest
from actions import (
    ArtifactError,
    ArtifactStore,
    Boundary,
    Contexts,
    Event,
    Needed,
    Result,
    Runner,
    UnsupportedError,
    evaluate,
)
from journey import clean_environment
from repo_checks.expect import contains, equal, truth

SMALL = """
name: small
on: push
jobs:
  answers:
    runs-on: ubuntu-24.04
    env:
      WORD: ${{ secrets.WORD }}
      LOUD: quietly
    outputs:
      word: ${{ steps.say.outputs.word }}
      loud: ${{ steps.say.outputs.loud }}
    steps:
      - uses: actions/checkout@v5
      - id: say
        run: |
          echo "word=$WORD" >> "$GITHUB_OUTPUT"
          echo "loud=$LOUD" >> "$GITHUB_OUTPUT"
        env:
          LOUD: loudly
  gated-open:
    needs: answers
    if: needs.answers.outputs.word != ''
    runs-on: ubuntu-24.04
    steps:
      - run: echo "shared=$RUNNER_TEMP" >> "$GITHUB_OUTPUT"
  gated-shut:
    needs: answers
    if: needs.answers.outputs.word == 'something else'
    runs-on: ubuntu-24.04
    steps:
      - run: exit 3
  after-shut:
    needs: gated-shut
    runs-on: ubuntu-24.04
    steps:
      - run: exit 3
  falls:
    runs-on: ubuntu-24.04
    steps:
      - run: exit 3
      - run: echo never
  after-falls:
    needs: [falls, answers]
    runs-on: ubuntu-24.04
    steps:
      - run: echo never
  cells:
    needs: answers
    runs-on: ${{ matrix.platform.runner }}
    strategy:
      matrix:
        platform:
          - id: one
            runner: ubuntu-24.04
          - id: two
            runner: ubuntu-24.04-arm
    steps:
      - run: echo "$CELL"
        env:
          CELL: ${{ matrix.platform.id }}
"""


# llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
def run_small(tmp_path: Path, **secrets: str) -> Runner:
    """A runner over the small workflow, in an empty checkout."""
    workflow = tmp_path / "small.yml"
    workflow.write_text(SMALL, encoding="utf-8")
    checkout = tmp_path / "checkout"
    checkout.mkdir()
    (tmp_path / "stand-ins").mkdir()
    return Runner(
        workflow,
        checkout,
        path_first=tmp_path / "stand-ins",
        env=clean_environment(),
        secrets=secrets,
    )


def test_a_job_follows_what_it_needs_and_what_its_condition_reads(tmp_path: Path) -> None:
    """Outputs flow along `needs`, a false condition skips, and a skip or a failure propagates."""
    run = run_small(tmp_path, WORD="spoken").run()

    equal(
        {name: str(job.result) for name, job in run.jobs.items()},
        {
            "answers": "success",
            "gated-open": "success",
            "gated-shut": "skipped",
            "after-shut": "skipped",
            "falls": "failure",
            "after-falls": "skipped",
            "cells": "success",
        },
        describing="what the forge would report each job as",
    )
    equal(
        run.jobs["answers"].outputs,
        {"word": "spoken", "loud": "loudly"},
        describing="the outputs a step wrote, under the job's env and then its own",
    )
    equal(
        run.jobs["answers"].boundaries,
        [Boundary("actions/checkout@v5")],
        describing="the boundaries",
    )
    equal(len(run.jobs["falls"].steps), 1, describing="the steps run before a failure stops a job")
    equal(
        [step.command for step in run.jobs["cells"].steps],
        ['echo "$CELL"'] * 2,
        describing="one run per matrix cell",
    )
    equal(
        [step.output.strip() for step in run.jobs["cells"].steps],
        ["one", "two"],
        describing="each cell's own values",
    )


def test_an_empty_output_shuts_the_gate(tmp_path: Path) -> None:
    """The same workflow over an empty answer: the open gate closes."""
    run = run_small(tmp_path, WORD="").run()

    equal(run.result("gated-open"), Result.SKIPPED, describing="a gate on an empty output")
    equal(run.result("cells"), Result.SUCCESS, describing="an ungated job after a success")


@pytest.mark.parametrize(
    ("expression", "value"),
    [
        ("needs.a.outputs.x != ''", True),
        ("needs.a.outputs.x == 'V0.2.0'", True),
        ("needs.a.outputs.missing != ''", False),
        ("needs.a.outputs.missing == ''", True),
        ("!(needs.a.outputs.x == 'v0.2.0') || needs.a.outputs.x != ''", True),
        ("needs.a.outputs.x == 'other' && needs.a.outputs.x != ''", False),
    ],
)
def test_the_expression_grammar_is_the_forges(expression: str, value: bool) -> None:
    """Case-insensitive strings, `null` equal to the empty string, short-circuit logic."""
    contexts = Contexts(needs={"a": Needed("success", {"x": "v0.2.0"})})

    equal(bool(evaluate(expression, contexts)), value, describing=expression)


@pytest.mark.parametrize(
    "expression",
    ["success()", "needs.a.result == 'success' && always()", "env.HOME", "1 + 1"],
)
def test_an_expression_outside_the_modelled_grammar_is_refused_by_name(expression: str) -> None:
    """Status functions, unknown contexts and arithmetic are not guessed at."""
    with pytest.raises(UnsupportedError) as refused:
        evaluate(expression, Contexts())

    contains(str(refused.value), expression.split()[0].rstrip("()"), describing="what it named")


@pytest.mark.parametrize(
    "written",
    [
        # A line with no `=` is not `name=value`.
        "just-a-word",
        # A line whose name is empty would keep a value under no name at all.
        "=unnamed",
    ],
)
def test_an_output_line_that_names_nothing_is_refused_by_its_text(
    written: str, tmp_path: Path
) -> None:
    """An output the forge could not address is refused rather than kept under an empty name."""
    workflow = tmp_path / "odd.yml"
    workflow.write_text(
        "jobs:\n  odd:\n    runs-on: x\n    steps:\n"
        f"      - run: echo '{written}' >> \"$GITHUB_OUTPUT\"\n",
        encoding="utf-8",
    )
    (tmp_path / "checkout").mkdir()

    with pytest.raises(UnsupportedError) as refused:
        Runner(workflow, tmp_path / "checkout", path_first=tmp_path, env=clean_environment()).run()

    contains(str(refused.value), written, describing="what it named")


def test_a_step_construct_outside_the_modelled_set_is_refused_by_name(tmp_path: Path) -> None:
    """`continue-on-error` changes what a failure means, so it is refused rather than dropped."""
    workflow = tmp_path / "odd.yml"
    workflow.write_text(
        "jobs:\n  odd:\n    runs-on: x\n    steps:\n      - run: exit 1\n"
        "        continue-on-error: true\n",
        encoding="utf-8",
    )
    (tmp_path / "checkout").mkdir()

    with pytest.raises(UnsupportedError) as refused:
        Runner(workflow, tmp_path / "checkout", path_first=tmp_path, env=clean_environment()).run()

    contains(str(refused.value), "continue-on-error", describing="what it named")


@pytest.mark.parametrize(
    ("step", "named"),
    [
        # A step condition calling a status function is not guessed at.
        ("      - uses: actions/checkout@v5\n        if: always()\n", "always"),
        # An expression inside a step's text is the injection the forge documents.
        ('      - run: echo "${{ secrets.WORD }}"\n', "expression in its text"),
    ],
)
def test_a_step_shape_outside_the_modelled_set_is_refused_before_anything_runs(
    step: str, named: str, tmp_path: Path
) -> None:
    """A construct that would change what runs is refused rather than run some other way."""
    workflow = tmp_path / "odd.yml"
    workflow.write_text(f"jobs:\n  odd:\n    runs-on: x\n    steps:\n{step}", encoding="utf-8")
    (tmp_path / "checkout").mkdir()

    with pytest.raises(UnsupportedError) as refused:
        Runner(workflow, tmp_path / "checkout", path_first=tmp_path, env=clean_environment()).run()

    contains(str(refused.value), named, describing="what it named")


@pytest.mark.parametrize(
    ("document", "named"),
    [
        ("- not a workflow\n", "no mapping of jobs"),
        ("jobs:\n  odd:\n    needs: {a: b}\n    steps: []\n", "not the shape the forge reads"),
        ("jobs:\n  odd:\n    steps:\n      - just a string\n", "not a mapping"),
        ("jobs:\n  odd:\n    steps:\n      - name: neither\n", "exactly one of"),
        ("jobs:\n  odd:\n    timeout-minutes: 5\n    steps: []\n", "timeout-minutes"),
    ],
)
def test_a_document_outside_the_workflow_shape_is_refused_before_anything_runs(
    document: str, named: str, tmp_path: Path
) -> None:
    """The boundary is the file: what is not shaped as the forge reads it is refused there.

    A job's own shape is refused as the workflow is read; a step's is refused
    once it is known which jobs run and before any of them does, so that a
    job skipped by name may carry what this runner does not model.
    """
    workflow = tmp_path / "odd.yml"
    workflow.write_text(document, encoding="utf-8")

    with pytest.raises(UnsupportedError) as refused:
        Runner(workflow, tmp_path, path_first=tmp_path, env=clean_environment()).run()

    contains(str(refused.value), named, describing="what it named")


#: A workflow of two shapes: one job that runs under a dispatch and records a
#: file into the store, and one that reads it back — under this run, or under
#: the run a caller names — beside a job whose steps this runner does not model.
SEAM = """
name: seam
on: [push, workflow_dispatch, workflow_run]
jobs:
  records:
    runs-on: ubuntu-24.04
    outputs:
      word: ${{ steps.said.outputs.word || steps.dispatched.outputs.word }}
      event: ${{ steps.which.outputs.event }}
    steps:
      - uses: actions/checkout@v5
        with:
          ref: ${{ inputs.tag }}
          fetch-depth: 0
          deep: ${{ inputs.tag != '' }}
      - id: which
        run: echo "event=$EVENT" >> "$GITHUB_OUTPUT"
        env:
          EVENT: ${{ github.event_name }}
      - id: said
        if: github.event_name == 'push'
        run: echo "word=pushed" >> "$GITHUB_OUTPUT"
      - id: dispatched
        if: github.event_name == 'workflow_dispatch'
        run: |
          mkdir -p "$RUNNER_TEMP/record"
          echo "$TAG" > "$RUNNER_TEMP/record/tag"
          echo "word=dispatched-$TAG" >> "$GITHUB_OUTPUT"
        env:
          TAG: ${{ inputs.tag }}
      - uses: actions/upload-artifact@v4
        if: github.event_name == 'workflow_dispatch'
        with:
          name: record
          path: ${{ runner.temp }}/record
      - uses: actions/upload-artifact@v4
        with:
          name: nothing-there
          path: ${{ runner.temp }}/never-written
  reads:
    needs: records
    runs-on: ubuntu-24.04
    outputs:
      tag: ${{ steps.read.outputs.tag }}
    steps:
      - uses: actions/download-artifact@v4
        with:
          name: record
          run-id: ${{ github.event.workflow_run.id || github.run_id }}
          github-token: ${{ github.token }}
          path: ${{ runner.temp }}/record
      - id: read
        run: echo "tag=$(cat "$RUNNER_TEMP/record/tag")" >> "$GITHUB_OUTPUT"
  merges:
    needs: records
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/download-artifact@v4
        with:
          pattern: rec*
          merge-multiple: true
          path: merged
      - run: ls merged 2>/dev/null || echo none
  outside:
    runs-on: ubuntu-24.04
    steps:
      - run: exit 1
        continue-on-error: true
  after-outside:
    needs: outside
    runs-on: ubuntu-24.04
    steps:
      - run: echo never
"""


def run_seam(tmp_path: Path, store: ArtifactStore, event: Event, run_id: str) -> Runner:
    """A runner over the seam workflow, under `event`, sharing `store`."""
    workflow = tmp_path / "seam.yml"
    workflow.write_text(SEAM, encoding="utf-8")
    checkout = tmp_path / f"checkout-{run_id}"
    checkout.mkdir(exist_ok=True)
    (tmp_path / "stand-ins").mkdir(exist_ok=True)
    return Runner(
        workflow,
        checkout,
        path_first=tmp_path / "stand-ins",
        env=clean_environment(),
        event=event,
        artifacts=store,
        run_id=run_id,
    )


#: The jobs of the seam workflow this runner models, which is every one but
#: the pair whose step carries `continue-on-error`.
MODELLED = {"records", "reads", "merges"}


def test_a_dispatch_runs_its_own_steps_and_records_into_the_store(tmp_path: Path) -> None:
    """The dispatched step runs, the push step does not and answers nothing, the record is kept."""
    store = ArtifactStore(tmp_path / "store")
    event = Event("workflow_dispatch", inputs={"tag": "v0.2.0"})

    run = run_seam(tmp_path, store, event, "7").run(MODELLED)

    equal(
        {name: str(job.result) for name, job in run.jobs.items()},
        {
            "records": "success",
            "reads": "success",
            "merges": "success",
            "outside": "skipped",
            "after-outside": "skipped",
        },
        describing="what the forge would report each job as",
    )
    equal(
        run.jobs["records"].outputs,
        {"word": "dispatched-v0.2.0", "event": "workflow_dispatch"},
        describing="the outputs: the dispatched step's, and nothing under the skipped step's id",
    )
    equal(
        run.jobs["records"].boundary("actions/checkout@"),
        Boundary("actions/checkout@v5", {"ref": "v0.2.0", "fetch-depth": "0", "deep": "true"}),
        describing="the checkout boundary, with its inputs as the forge would hand them",
    )
    equal(store.names("7"), ["record"], describing="what the run uploaded: not a missing path")
    equal(store.read("7", "record", "tag"), "v0.2.0\n", describing="what the record holds")
    equal(run.jobs["reads"].outputs, {"tag": "v0.2.0"}, describing="what this run read back")
    contains(run.jobs["merges"].steps[-1].output, "tag", describing="the merged download")


def test_a_push_skips_the_dispatched_steps_and_a_download_of_nothing_fails(
    tmp_path: Path,
) -> None:
    """Under the default event the push step answers, nothing is uploaded, and the read fails."""
    store = ArtifactStore(tmp_path / "store")

    run = run_seam(tmp_path, store, Event(), "8").run(MODELLED)

    equal(run.jobs["records"].outputs["word"], "pushed", describing="the push step's output")
    equal(run.jobs["records"].outputs["event"], "push", describing="the default event")
    equal(
        run.jobs["records"].boundary("actions/checkout@").given["ref"],
        "",
        describing="an input nothing dispatched, interpolated to nothing",
    )
    equal(store.names("8"), [], describing="what a push uploaded")
    equal(run.result("reads"), Result.FAILURE, describing="a download of what nothing uploaded")
    failed = run.jobs["reads"].steps[-1]
    equal(failed.command, "uses: actions/download-artifact@v4", describing="the step that failed")
    contains(failed.output, "Artifact not found for name: record", describing="what it said")
    equal(len(run.jobs["reads"].steps), 1, describing="the steps run before the failure")
    equal(run.result("merges"), Result.SUCCESS, describing="a pattern matching nothing")


def test_a_workflow_run_reads_another_runs_artifacts_by_the_triggering_runs_id(
    tmp_path: Path,
) -> None:
    """Two runs over one store: the second reads what the first uploaded, by `run-id`."""
    store = ArtifactStore(tmp_path / "store")
    first = run_seam(tmp_path, store, Event("workflow_dispatch", inputs={"tag": "v0.3.0"}), "9")
    first.run({"records"})
    triggered = Event("workflow_run", workflow_run={"event": "workflow_dispatch", "id": "9"})

    run = run_seam(tmp_path, store, triggered, "10").run({"records", "reads"})

    equal(run.jobs["records"].outputs["event"], "workflow_run", describing="the event")
    equal(run.jobs["records"].outputs["word"], "", describing="neither trigger's step ran")
    equal(
        run.jobs["reads"].boundary("actions/download-artifact@").given["run-id"],
        "9",
        describing="the run the download named",
    )
    equal(run.jobs["reads"].outputs, {"tag": "v0.3.0"}, describing="what the first run recorded")
    equal(store.names("10"), [], describing="what the second run uploaded")


def test_a_job_outside_the_set_to_run_is_skipped_by_name_with_what_needs_it(
    tmp_path: Path,
) -> None:
    """The job carrying an unmodelled step never runs, and nothing of it is approximated."""
    store = ArtifactStore(tmp_path / "store")
    runner = run_seam(tmp_path, store, Event(), "11")

    run = runner.run({"records"})

    equal(run.result("records"), Result.SUCCESS, describing="the one job named")
    equal(
        {name for name, job in run.jobs.items() if job.result is Result.SKIPPED},
        {"reads", "merges", "outside", "after-outside"},
        describing="every job outside the set, and the jobs needing one",
    )
    with pytest.raises(UnsupportedError) as refused:
        runner.run({"records", "outside"})
    contains(str(refused.value), "continue-on-error", describing="a named job is still held")
    with pytest.raises(UnsupportedError) as unknown:
        runner.run({"records", "elsewhere"})
    contains(str(unknown.value), "elsewhere", describing="a job the workflow does not declare")


def test_the_contexts_an_event_answers_are_the_forges(tmp_path: Path) -> None:
    """`github`, `inputs` and `runner` resolve as the forge documents them."""
    event = Event("workflow_run", workflow_run={"event": "push", "id": "12", "head_sha": "abc"})
    contexts = Contexts(github=event.github("13"), inputs={}, runner={"temp": "the-temp"})

    equal(evaluate("github.event_name", contexts), "workflow_run", describing="the event")
    equal(evaluate("github.event.workflow_run.head_sha", contexts), "abc", describing="the sha")
    equal(evaluate("github.event.workflow_run.id", contexts), "12", describing="the run id")
    equal(evaluate("github.run_id", contexts), "13", describing="this run's id")
    equal(evaluate("inputs.tag", contexts), None, describing="an input nothing gave")
    equal(evaluate("runner.temp", contexts), "the-temp", describing="the job's temp")
    truth(
        evaluate(
            "github.event_name == 'workflow_run' && github.event.workflow_run.event == 'push'",
            contexts,
        )
        is True,
        describing="the condition the install-path workflow gates on",
    )


def test_an_upload_of_a_file_keeps_it_under_its_own_name_and_a_second_is_refused(
    tmp_path: Path,
) -> None:
    """A path naming one file is kept as that file, and a taken name is not uploaded over."""
    store = ArtifactStore(tmp_path / "store")
    one = tmp_path / "one.txt"
    one.write_text("one\n", encoding="utf-8")

    truth(store.upload("14", "files", one), describing="the upload of a file")
    with pytest.raises(ArtifactError) as taken:
        store.upload("14", "files", one)
    contains(str(taken.value), "already exists", describing="the action's own refusal")
    into = tmp_path / "into"
    store.download("14", "files", into)

    equal((into / "one.txt").read_text(encoding="utf-8"), "one\n", describing="what came down")
    store.download_matching("14", "fil*", tmp_path / "each", merge=False)
    equal(
        (tmp_path / "each" / "files" / "one.txt").read_text(encoding="utf-8"),
        "one\n",
        describing="a pattern download that is not merged, each under its name",
    )


def test_an_upload_of_a_path_holding_nothing_creates_no_artifact(tmp_path: Path) -> None:
    """A missing path and an empty directory alike: the action finds no files and uploads none."""
    store = ArtifactStore(tmp_path / "store")
    empty = tmp_path / "empty"
    empty.mkdir()

    truth(not store.upload("15", "nothing", tmp_path / "missing"), describing="a missing path")
    truth(not store.upload("15", "nothing", empty), describing="an empty directory")

    equal(store.names("15"), [], describing="what the run holds")
    with pytest.raises(ArtifactError, match="Artifact not found for name: nothing"):
        store.download("15", "nothing", tmp_path / "into")


@pytest.mark.parametrize("name", ["", ".", "..", "../escape", "a/b", "a\\b", "a:b", "a\nb"])
def test_a_name_that_is_not_a_single_component_is_refused_before_it_touches_the_store(
    name: str, tmp_path: Path
) -> None:
    """An artifact name and a run id are path components, held to what the action takes."""
    store = ArtifactStore(tmp_path / "store")
    one = tmp_path / "one.txt"
    one.write_text("one\n", encoding="utf-8")

    with pytest.raises(ArtifactError, match="not valid"):
        store.upload("16", name, one)
    with pytest.raises(ArtifactError, match="not valid"):
        store.download(name, "files", tmp_path / "into")

    equal(sorted(path.name for path in tmp_path.iterdir()), ["one.txt"], describing="the tree")


def test_an_artifact_path_outside_the_checkout_and_the_temp_is_refused(tmp_path: Path) -> None:
    """A step's artifact path is the checkout's or the job's temp, and nowhere else."""
    workflow = tmp_path / "odd.yml"
    workflow.write_text(
        "jobs:\n  odd:\n    runs-on: x\n    steps:\n"
        "      - uses: actions/upload-artifact@v4\n        with:\n"
        "          name: escaped\n          path: ../../elsewhere\n",
        encoding="utf-8",
    )
    (tmp_path / "checkout").mkdir()

    with pytest.raises(UnsupportedError) as refused:
        Runner(workflow, tmp_path / "checkout", path_first=tmp_path, env=clean_environment()).run()

    contains(str(refused.value), "elsewhere", describing="what it named")
