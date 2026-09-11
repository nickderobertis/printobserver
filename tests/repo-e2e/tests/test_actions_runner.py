"""The rules `actions.py` runs a workflow under are the forge's, and nothing past them is guessed.

The release-workflow journey rests on this runner, so what it claims about the
forge is pinned here over a workflow small enough to read: a job waits on what
it `needs` and is skipped when that failed or was skipped; a job's `if:` reads
another job's outputs and skips it when false; a step's `GITHUB_OUTPUT` lines
become the job's outputs; and a construct outside the modelled set is refused
by name rather than run as something else.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from pathlib import Path

import pytest
from actions import Result, Runner, UnsupportedError, evaluate
from journey import clean_environment
from repo_checks.expect import contains, equal

SMALL = """
name: small
on: push
jobs:
  answers:
    runs-on: ubuntu-24.04
    outputs:
      word: ${{ steps.say.outputs.word }}
    steps:
      - uses: actions/checkout@v5
      - id: say
        run: echo "word=$WORD" >> "$GITHUB_OUTPUT"
        env:
          WORD: ${{ secrets.WORD }}
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
      - run: echo "${{ matrix.platform.id }}"
"""


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
    equal(run.jobs["answers"].outputs, {"word": "spoken"}, describing="the outputs a step wrote")
    equal(run.jobs["answers"].boundaries, ["actions/checkout@v5"], describing="the boundaries")
    equal(len(run.jobs["falls"].steps), 1, describing="the steps run before a failure stops a job")
    equal(
        [step.command for step in run.jobs["cells"].steps],
        ['echo "${{ matrix.platform.id }}"'] * 2,
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
    contexts = {"needs": {"a": {"outputs": {"x": "v0.2.0"}}}}

    equal(bool(evaluate(expression, contexts)), value, describing=expression)


@pytest.mark.parametrize(
    "expression",
    ["success()", "needs.a.result == 'success' && always()", "github.event_name", "1 + 1"],
)
def test_an_expression_outside_the_modelled_grammar_is_refused_by_name(expression: str) -> None:
    """Status functions, unknown contexts and arithmetic are not guessed at."""
    with pytest.raises(UnsupportedError) as refused:
        evaluate(expression, {"needs": {}})

    contains(str(refused.value), expression.split()[0].rstrip("()"), describing="what it named")


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
