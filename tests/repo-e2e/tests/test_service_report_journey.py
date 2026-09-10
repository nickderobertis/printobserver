"""What a completed install-path run says about the two commands it does not gate on.

The two commands after the routes cannot succeed unattended, so an install job
lets each fail without failing: made fatal, the workflow fails on every run
whatever the world looks like. The price of that waiver is that the run itself
has to say what they reached, because a green job that says nothing about
whether the service was established is a credential-dependent failure that has
disappeared.

Nothing here is mocked and nothing is restated. The step that writes that
report is read out of the COMMITTED workflow and run — the real shell, the real
`GITHUB_STEP_SUMMARY` file, the outcomes GitHub itself would hand it — and what
a reader of the run would see is what is asserted. A journey that composed the
line itself would prove a line nothing publishes.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import tomllib
from pathlib import Path

import pytest
import yaml
from journey import REPO_ROOT, capture, clean_environment
from repo_checks.expect import contains, equal, truth

#: The workflow whose install jobs take the waiver.
WORKFLOW = ".github/workflows/install-path.yml"

#: What GitHub hands a step for one that ran and one that did not. `outcome`
#: rather than `conclusion`, because `continue-on-error` is what makes
#: `conclusion` read `success` for a step that failed.
ESTABLISHED = "success"
NOT_ESTABLISHED = "failure"


def _declared() -> tuple[tuple[str, ...], str]:
    """The waived steps and the variable their report is written to."""
    policy = tomllib.loads((REPO_ROOT / "repo-policy.toml").read_text(encoding="utf-8"))
    workflows = policy["workflows"]
    return tuple(workflows["waived_steps"]), str(workflows["waived_report"])


def _reports() -> dict[str, str]:
    """The report step each install job of the committed workflow carries, by job."""
    workflow = yaml.safe_load((REPO_ROOT / WORKFLOW).read_text(encoding="utf-8"))
    _, summary = _declared()
    found: dict[str, str] = {}
    for name, job in workflow["jobs"].items():
        for step in job.get("steps", []):
            command = str(step.get("run", ""))
            if summary in command:
                found[name] = command
    return found


# llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
def _said(command: str, into: Path, installed: str, started: str) -> str:
    """Run one report step the way its own job runs it, and answer what the run shows."""
    _, summary = _declared()
    written = into / "summary.md"
    written.write_text("", encoding="utf-8")
    result = capture(
        ["bash", "-c", command],
        into,
        timeout=60,
        env=clean_environment(
            **{summary: str(written), "INSTALLED": installed, "STARTED": started}
        ),
    )
    equal(result.returncode, 0, describing=f"the report step to run: {result.stderr}")
    return written.read_text(encoding="utf-8")


def test_every_install_job_that_takes_the_waiver_carries_a_report() -> None:
    """One report per install job, and the job it names is the one it is in."""
    reports = _reports()

    truth(bool(reports), describing=f"{WORKFLOW} to carry a report step at all")
    for name, command in reports.items():
        contains(command, name, describing=f"the report of job `{name}` to name that route")


@pytest.mark.parametrize("job", sorted(_reports()))
def test_a_run_whose_service_was_established_says_so(job: str, tmp_path: Path) -> None:
    """Both commands reached, and the run says both reached."""
    said = _said(_reports()[job], tmp_path, ESTABLISHED, ESTABLISHED)

    contains(said, job, describing="the route this report belongs to")
    equal(said.count(ESTABLISHED), 2, describing=f"both commands reported as reached: {said}")
    truth(NOT_ESTABLISHED not in said, describing=f"nothing reported as not reached: {said}")


@pytest.mark.parametrize("job", sorted(_reports()))
def test_a_run_whose_service_was_not_established_says_that_instead(
    job: str, tmp_path: Path
) -> None:
    """The installer ran and the unit would not start, which is the ordinary state.

    This is what the waiver would otherwise hide: the job is green either way,
    so the two have to be told apart by what the run shows.
    """
    said = _said(_reports()[job], tmp_path, ESTABLISHED, NOT_ESTABLISHED)

    contains(said, NOT_ESTABLISHED, describing=f"the command that did not reach: {said}")
    truth(
        said != _said(_reports()[job], tmp_path, ESTABLISHED, ESTABLISHED),
        describing="a run whose service came up to read differently from one whose did not",
    )


@pytest.mark.parametrize("job", sorted(_reports()))
def test_each_of_the_two_commands_is_reported_apart_from_the_other(
    job: str, tmp_path: Path
) -> None:
    """Which of the two did not reach is the difference between two repairs.

    An installer that never ran and a unit that would not start are not the
    same finding, and a report collapsing them into one word would send a
    reader to the wrong one.
    """
    command = _reports()[job]

    neither = _said(command, tmp_path, NOT_ESTABLISHED, NOT_ESTABLISHED)
    installer = _said(command, tmp_path, NOT_ESTABLISHED, ESTABLISHED)
    unit = _said(command, tmp_path, ESTABLISHED, NOT_ESTABLISHED)

    equal(neither.count(NOT_ESTABLISHED), 2, describing=f"both reported as not reached: {neither}")
    truth(installer != unit, describing=f"the two repairs to read apart: {installer}{unit}")
