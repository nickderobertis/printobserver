"""The `printobserver` skill, run on every real-print scenario through a real harness.

One test per scenario of every `tests/real-prints/*/case.json` `assertions`
block, discovered from the cases on disk. Each hands the skill the case's real
pictures as files and its event, with every `printobserver` command stubbed
(`scenario.build`), and judges what the agent ran by the case's own rule:

- each `never` step is a `not_called` eval, and every step all acceptable
  outcomes share is a `called` one, so skilltest's report fails on either;
- `accept_any_of` is judged here, over the commands the run report's
  `mock_calls` recorded, as ordered subsequences (`real_prints.outcome_met`).

The model is the one the shipped configuration pins for supervision turns, and
when it pins none — as today — none is pinned here, so the harness picks its
default exactly as a production turn's does.

The harness runs with a production turn's permissions (`scenario.harness_config`):
the read tools, and the shell for the program's own commands alone. They are
set by a `.oneharness.toml` above the workspace rather than by a variable the
agent's shell would inherit, because the model must not be able to tell it is
under test: the workspace is a neutral temporary directory laid out as a
supervisor's state directory, and the environment carries no variable naming
this repository's tooling.

Opt-in, never in `just check`: run it with `just skilltest` (`-k <scenario>` for
one). Set `SKILLTEST_REPORT_DIR` to keep each scenario's transcript and verdict.
"""

from __future__ import annotations

import json
import os
import shutil
import tempfile
from collections.abc import Iterator
from pathlib import Path

import pytest
from real_prints import (
    REPO,
    Scenario,
    commands_in,
    commands_written,
    met_outcome,
    scenarios,
    shipped_model,
)
from repo_checks import expect
from scenario import build, harness_config
from skilltest_pytest import Report, describe_failures, run_skill

ONEHARNESS = shutil.which("oneharness")
# Every scenario on disk, each one test; `test_skilltest_wiring` holds this to the cases.
SCENARIOS = scenarios()
# Captured before the run strips every `SKILLTEST_*` variable from the environment.
REPORT_DIR = os.environ.get("SKILLTEST_REPORT_DIR", "").strip()
# Long enough for several looks and an answer; a turn that needs longer is stuck.
TIMEOUT_S = 900


def _provider_available() -> bool:
    return ONEHARNESS is not None and shutil.which("claude") is not None


@pytest.fixture
def neutral_tmp() -> Iterator[Path]:
    """A throwaway directory with a neutral name, removed afterwards.

    Not pytest's `tmp_path`, whose path names pytest and the test, which the
    agent would read in every path it is handed.
    """
    path = Path(tempfile.mkdtemp())
    yield path
    shutil.rmtree(path, ignore_errors=True)


def _stealth(monkeypatch: pytest.MonkeyPatch, workspace: Path) -> None:
    """Strip what would tell the agent it is under test, for this run only."""
    for name in list(os.environ):
        if name.startswith(("ONEHARNESS_", "SKILLTEST_", "PYTEST_", "UV_")):
            monkeypatch.delenv(name)
    for name in ("VIRTUAL_ENV", "VIRTUAL_ENV_PROMPT", "OLDPWD", "UV", "_", "PYTHONHOME"):
        monkeypatch.delenv(name, raising=False)
    kept = [
        entry
        for entry in os.environ.get("PATH", "").split(os.pathsep)
        if entry and not Path(entry).resolve().is_relative_to(REPO)
    ]
    monkeypatch.setenv("PATH", os.pathsep.join(kept))
    monkeypatch.setenv("PWD", str(workspace))


def _unstubbed(report: Report) -> list[str]:
    """Every shell command naming a `printobserver` invocation that no stub intercepted.

    The stubs' patterns assume the shape of the call the harness hook matches
    them against; the run report's `mock_calls` are what the real hook saw and
    did, so a `printobserver` invocation it let through is a pattern that does
    not match a real call, and would have run a program that is not there.
    """
    found = []
    for run in report.runs:
        for call in run.mock_calls or []:
            command = call.input.get("command") if isinstance(call.input, dict) else None
            if isinstance(command, str) and commands_in(command) and call.action == "allow":
                found.append(command)
    return found


def _shell_commands(report: Report) -> list[str]:
    """Every shell command the agent ran, in order, as it wrote them.

    The mock channel records every tool call of a run that declares mocks, so
    its `mock_calls` are every call the agent made, stubbed or not.
    """
    found = []
    for run in report.runs:
        for call in run.mock_calls or []:
            command = call.input.get("command") if isinstance(call.input, dict) else None
            if (call.tool or "").lower() == "bash" and isinstance(command, str):
                found.append(command)
    return found


def _keep(scenario: Scenario, report: Report, ran: list[str], passed: bool) -> None:
    if not REPORT_DIR:
        return
    directory = Path(REPORT_DIR)
    directory.mkdir(parents=True, exist_ok=True)
    stem = scenario.test_id.replace("/", "__")
    (directory / f"{stem}.report.json").write_text(report.model_dump_json(indent=2), "utf-8")
    verdict = {
        "scenario": scenario.test_id,
        "passed": passed,
        "models": [run.model for run in report.runs],
        "commands": ran,
    }
    with (directory / "verdicts.jsonl").open("a", encoding="utf-8") as verdicts:
        verdicts.write(json.dumps(verdict) + "\n")


@pytest.mark.skilltest_e2e
@pytest.mark.skipif(
    not _provider_available(),
    reason="no skilltest provider: oneharness and Claude Code must both be on PATH",
)
@pytest.mark.parametrize("scenario", SCENARIOS, ids=lambda scenario: scenario.test_id)
def test_the_skill_takes_an_action_the_case_accepts(
    scenario: Scenario, neutral_tmp: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The agent's commands meet one acceptable outcome and no step it must never take."""
    built = build(scenario, neutral_tmp)
    (neutral_tmp / ".oneharness.toml").write_text(harness_config(), encoding="utf-8")
    config = Path(tempfile.mkdtemp()) / "skilltest.yaml"
    config.write_text(
        "provider:\n"
        "  kind: oneharness\n"
        f"  bin: {json.dumps(ONEHARNESS)}\n"
        "  judge_harness: claude-code\n"
        f"  timeout_secs: {TIMEOUT_S}\n",
        encoding="utf-8",
    )
    _stealth(monkeypatch, built.workspace)
    model = shipped_model()
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        report = run_skill(
            built.case,
            platforms=["claude-code"],
            # An empty model is skilltest's "unpinned": the harness's default.
            models=[model or ""],
            config=config,
            cwd=built.workspace,
        )
    finally:
        shutil.rmtree(config.parent, ignore_errors=True)

    expect.equal(
        _unstubbed(report),
        [],
        describing=f"every printobserver command {scenario.test_id} ran to reach a stub",
    )
    shell = _shell_commands(report)
    ran = commands_written(shell, cwd=built.workspace)
    met = met_outcome(scenario, ran)
    passed = report.passed and met is not None
    _keep(scenario, report, [command.describe() for command in ran], passed)
    print(
        f"\n{scenario.test_id} on {[run.model or '(harness default)' for run in report.runs]}: "
        f"{'PASS' if passed else 'FAIL'}"
    )
    for command in ran:
        print(f"  ran: {command.describe()}")

    ran_lines = "\n".join(f"  {command.describe()}" for command in ran) or "  (none)"
    accepted = "\n".join(
        "  " + (" then ".join(step.describe() for step in outcome) or "(nothing)")
        for outcome in scenario.accept_any_of
    )
    expect.truth(
        passed,
        describing=f"{scenario.test_id} to meet an acceptable outcome and take no never step "
        f"({scenario.summary})\n"
        f"printobserver commands the agent ran:\n{ran_lines}\n"
        f"acceptable outcomes:\n{accepted}\n"
        f"outcome met: {'yes' if met is not None else 'none'}\n"
        f"{describe_failures(report)}",
    )
