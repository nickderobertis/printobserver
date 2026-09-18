"""The committed platform-dispatch workflow, run: one job, on one runner, and nothing else.

The forge dispatches a workflow only from the default branch, so the first
hosted run of `.github/workflows/platform-dispatch.yml` can exist only after
the change that adds it merges. What it does is proven here instead, by
running the committed workflow under the forge's rules (`actions.py`) as a
`workflow_dispatch` with its two inputs, over a copy of the tree:

  * `select` runs the real `just dispatch-resolve`, which runs the committed
    script over the copy, and its answer is the runner the `run` job lands on;
  * `run` runs the real `just dispatch-run`, which runs the source job's own
    `run:` steps off the copy's committed source workflow;
  * every OTHER recipe those steps reach — `just bootstrap`, the OctoPrint
    bring-up, the tier, the bring-down, the gate — is written to a record
    rather than run, because the question is which commands the dispatch runs
    and where, and running the printer tier to answer it would prove nothing
    more.

A pair naming a platform the supported-platform list does not name is then
dispatched the same way, and refused at `select` with nothing run.
"""

from __future__ import annotations

import shutil
from collections.abc import Callable
from pathlib import Path

from actions import Event, Result, Runner, WorkflowRun
from journey import REPO_ROOT, GateCopy, clean_environment
from repo_checks.expect import contains, equal, truth
from repo_checks.parsing import jobs_of, load_workflow

WORKFLOW = ".github/workflows/platform-dispatch.yml"
CI = ".github/workflows/ci.yml"

# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
JUST_STANDIN = """#!/bin/sh
# The real `just` for the two dispatch recipes; every other recipe is written
# to DISPATCH_STANDIN_RECORD instead of run.
case "$1" in
  dispatch-resolve|dispatch-run) exec "$DISPATCH_STANDIN_REAL_JUST" "$@" ;;
esac
printf 'just %s\\n' "$*" >> "$DISPATCH_STANDIN_RECORD"
"""


def dispatched(
    gate_copy: Callable[..., GateCopy], tmp_path: Path, job: str, platform: str
) -> tuple[WorkflowRun, list[str]]:
    """Dispatch the committed workflow with `job` and `platform` over a copy.

    Returns the run, and every recipe the source job's steps reached, in order.
    """
    copy = gate_copy()
    stand_ins = tmp_path / "stand-ins"
    stand_ins.mkdir()
    program = stand_ins / "just"
    program.write_text(JUST_STANDIN, encoding="utf-8")
    program.chmod(0o755)
    record = tmp_path / "reached"
    record.touch()
    real_just = shutil.which("just")
    truth(real_just is not None, describing="`just` to be on the PATH")

    run = Runner(
        copy.root / WORKFLOW,
        copy.root,
        path_first=stand_ins,
        env=clean_environment(
            UV_PROJECT_ENVIRONMENT=str(copy.shared_venv),
            DISPATCH_STANDIN_RECORD=str(record),
            DISPATCH_STANDIN_REAL_JUST=str(real_just),
        ),
        event=Event("workflow_dispatch", inputs={"job": job, "platform": platform}),
    ).run()
    return run, record.read_text(encoding="utf-8").splitlines()


def source_commands(job: str) -> list[str]:
    """The `run:` commands the committed `ci.yml` job carries, in order."""
    steps = jobs_of(load_workflow(REPO_ROOT / CI))[job]["steps"]
    return [str(step["run"]).strip() for step in steps if "run" in step]


def test_a_dispatch_runs_exactly_that_job_on_that_platforms_runner(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The integration tier on Apple silicon: its four commands, on `macos-15`, and nothing else."""
    run, reached = dispatched(gate_copy, tmp_path, "integration", "macos-aarch64")

    equal(run.result("select"), Result.SUCCESS, describing="the resolving job")
    equal(
        run.jobs["select"].outputs,
        {"runner": "macos-15"},
        describing="what `select` answered",
    )
    equal(run.result("run"), Result.SUCCESS, describing=f"the running job: {run.jobs['run'].steps}")
    equal(run.jobs["run"].runners, ["macos-15"], describing="the runner `run` landed on")
    equal(
        reached,
        source_commands("integration"),
        describing="every recipe the dispatch reached, against the source job's own commands",
    )
    equal(
        run.commands("run"),
        ['just dispatch-run "$JOB" "$PLATFORM"'],
        describing="the one command the running job ran",
    )


def test_a_dispatch_of_another_job_runs_that_job_alone(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The gate on Linux ARM: the gate's commands on its runner, none of the integration tier's."""
    run, reached = dispatched(gate_copy, tmp_path, "gate", "linux-aarch64")

    equal(run.jobs["run"].runners, ["ubuntu-24.04-arm"], describing="the runner `run` landed on")
    equal(reached, source_commands("gate"), describing="every recipe the dispatch reached")


def test_an_unknown_pair_is_refused_before_anything_runs(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """A platform the list does not name fails `select`, skips `run`, and reaches nothing."""
    run, reached = dispatched(gate_copy, tmp_path, "gate", "linux-riscv64")

    equal(run.result("select"), Result.FAILURE, describing="the resolving job")
    contains(
        run.jobs["select"].steps[-1].output,
        "refused: `linux-riscv64` is no platform",
        describing="what `select` said",
    )
    equal(run.result("run"), Result.SKIPPED, describing="the running job")
    equal(reached, [], describing="every recipe the dispatch reached")
