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
from typing import Any

from repo_checks.shell import run
from turns_per_run import CASES, turns_per_test

REPO = Path(__file__).resolve().parents[2]
RESULT = REPO / "tests" / "skilltest" / "results" / "latest.json"
HARNESS = "claude-code"


def commit() -> str:
    """The commit this tree's run used."""
    return run(["git", "rev-parse", "HEAD"], cwd=REPO, check=True).stdout.strip()


def folded(directory: Path, commit_id: str, cases: Path = CASES) -> dict[str, Any]:
    """The record of one run, from each test's record in its directory."""
    tests = [
        json.loads(path.read_text(encoding="utf-8")) for path in sorted(directory.glob("*.json"))
    ]
    points = [point for test in tests for point in test["decision_points"]]
    counted = turns_per_test(cases)
    return {
        "figures": {"turns": len(points)},
        "detail": {
            "commit": commit_id,
            "harness": HARNESS,
            "models": sorted({model for point in points for model in point["models"]}),
            "turns_the_cases_count": sum(counted[test["test"]] for test in tests),
            "compactions": sum(1 for point in points if point["compacted"]),
            "passed": all(test["passed"] for test in tests),
            "tests": tests,
        },
    }


def main(arguments: list[str]) -> int:
    """Run the suite, or read a finished run, and write the record."""
    directory = Path(arguments[0]) if arguments and Path(arguments[0]).is_dir() else None
    status = 0
    if directory is None:
        directory = Path(tempfile.mkdtemp())
        status = run(
            ["just", "skilltest", *arguments],
            cwd=REPO,
            env={**os.environ, "SKILLTEST_RUN_DIR": str(directory)},
            capture=False,
        ).returncode
    RESULT.parent.mkdir(parents=True, exist_ok=True)
    RESULT.write_text(json.dumps(folded(directory, commit()), indent=2) + "\n", encoding="utf-8")
    print(f"wrote {RESULT.relative_to(REPO)}", file=sys.stderr)
    return status


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
