"""`just skilltest-record`: run the suite and write what it did to `results/latest.json`.

The record is `{"figures": {"turns": n}, "detail": {...}}`, the shape `repo_checks
telemetry_budget` reads: the commit the run used, the harness, every model a turn
answered with, each test's decision points with the commands the agent ran, and
each turn's usage. Given a directory of a finished run's test records, it folds
them; given none, it runs `just skilltest` to make one, and exits as that did.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path
from typing import TypedDict

from replay import DecisionRecord, TestRecord, is_assessment
from repo_checks.shell import run
from turns_per_run import CASES, UnreadableCaseError, replays

REPO = Path(__file__).resolve().parents[2]
RESULT = REPO / "tests" / "skilltest" / "results" / "latest.json"
HARNESS = "claude-code"


def commit() -> str:
    """The commit this tree's run used."""
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    return run(["git", "rev-parse", "HEAD"], cwd=REPO, check=True).stdout.strip()


class Figures(TypedDict):
    """The figures a budget reads off the record."""

    turns: int


class Detail(TypedDict):
    """What the record says beside its figures."""

    commit: str
    harness: str
    models: list[str]
    turns_the_cases_count: int
    compactions: int
    passed: bool
    tests: list[TestRecord]


class RunRecord(TypedDict):
    """The record of one run."""

    figures: Figures
    detail: Detail


class UnreadableRunError(Exception):
    """A run's directory holding something that is no test's record, or nothing at all."""


def _decision_point_is_readable(point: object) -> bool:
    """Whether a decision point has every field of `DecisionRecord`, with those read here typed."""
    return (
        isinstance(point, dict)
        and DecisionRecord.__annotations__.keys() <= point.keys()
        and isinstance(point["scenario"], str)
        and isinstance(point["at"], str)
        and isinstance(point["trigger"], str)
        and isinstance(point["requests"], list)
        and all(isinstance(request, str) for request in point["requests"])
        and isinstance(point["opened"], list)
        and all(isinstance(name, str) for name in point["opened"])
        and (point["why_not"] is None or isinstance(point["why_not"], str))
        and point["passed"] is (point["why_not"] is None)
        and is_assessment(point["assessment"])
        and type(point["looks"]) is int
        and point["looks"] >= 0
        and (point["usage"] is None or isinstance(point["usage"], dict))
        and isinstance(point["passed"], bool)
        and isinstance(point["compacted"], bool)
        and isinstance(point["commands"], list)
        and all(isinstance(command, str) for command in point["commands"])
        and isinstance(point["models"], list)
        and all(isinstance(model, str) for model in point["models"])
    )


def _compound_commands_are_readable(found: object, sessions: list[str]) -> bool:
    """Whether a test says, for each of its conversations, what it ran beside another command."""
    return (
        isinstance(found, dict)
        and set(found) == set(sessions)
        and all(
            isinstance(commands, list) and all(isinstance(command, str) for command in commands)
            for commands in found.values()
        )
    )


def _test_record(path: Path) -> TestRecord:
    """One test's record, once it has the fields the record is folded from."""
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        not isinstance(document, dict)
        or not isinstance(document.get("test"), str)
        or not isinstance(document.get("passed"), bool)
        or type(document.get("turns")) is not int
        or not isinstance(document.get("sessions"), list)
        or not all(isinstance(session, str) for session in document["sessions"])
        or not _compound_commands_are_readable(
            document.get("compound_commands"), document["sessions"]
        )
        or not isinstance(document.get("decision_points"), list)
        or not all(_decision_point_is_readable(point) for point in document["decision_points"])
        or document["turns"] != len(document["decision_points"])
        or document["passed"] != all(point["passed"] for point in document["decision_points"])
    ):
        msg = f"{path} is no test's record"
        raise UnreadableRunError(msg)
    return document


def folded(directory: Path, commit_id: str, cases: Path = CASES) -> RunRecord:
    """The record of one run, from each test's record in its directory."""
    tests = [_test_record(path) for path in sorted(directory.glob("*.json"))]
    named = [test["test"] for test in tests]
    if len(set(named)) != len(named):
        msg = f"{directory} holds one test twice: {sorted(named)}"
        raise UnreadableRunError(msg)
    points = [point for test in tests for point in test["decision_points"]]
    replayed = replays(cases)
    for test in tests:
        if replayed.get(test["test"]) != [point["scenario"] for point in test["decision_points"]]:
            msg = f"{test['test']} is no case with a replay of those decision points"
            raise UnreadableRunError(msg)
    return {
        "figures": {"turns": len(points)},
        "detail": {
            "commit": commit_id,
            "harness": HARNESS,
            "models": sorted({model for point in points for model in point["models"]}),
            "turns_the_cases_count": sum(len(replayed[test["test"]]) for test in tests),
            "compactions": sum(1 for point in points if point["compacted"]),
            "passed": all(test["passed"] for test in tests),
            "tests": tests,
        },
    }


def main(arguments: list[str]) -> int:
    """Run the suite, or read a finished run, write the record, and exit as the run went."""
    directory = Path(arguments[0]) if arguments and Path(arguments[0]).is_dir() else None
    status = 0
    if directory is None:
        directory = Path(tempfile.mkdtemp())
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        status = run(
            ["just", "skilltest", *arguments],
            cwd=REPO,
            env={**os.environ, "SKILLTEST_RUN_DIR": str(directory)},
            capture=False,
        ).returncode
    try:
        written = folded(directory, commit())
    except (UnreadableRunError, UnreadableCaseError, ValueError) as error:
        print(f"record: {error}", file=sys.stderr)
        return status or 1
    RESULT.parent.mkdir(parents=True, exist_ok=True)
    RESULT.write_text(json.dumps(written, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {RESULT.relative_to(REPO)}", file=sys.stderr)
    if status:
        return status
    if not written["detail"]["tests"] or not written["detail"]["passed"]:
        print("record: the run has no tests, or a test that failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
