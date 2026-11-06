"""`skilltest-turns-per-run`: the real harness turns one full `just skilltest` run spends.

One turn per event a test feeds, so the figure is the `replay` entries of every
case on disk. It is counted fresh and runs no model. A test that runs the replay
holds the count here to the turns the harness actually received
(`tests/skilltest-wiring`).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from onebudgetspec_sdk import report

CASES = Path(__file__).resolve().parents[2] / "tests" / "real-prints"


class UnreadableCaseError(Exception):
    """A case whose `replay` is no list of scenario ids, which would count as nothing real."""


def replays(cases: Path = CASES) -> dict[str, list[str]]:
    """Each case's `replay` list, by the case's name, for the cases with one."""
    found = {}
    for path in sorted(cases.glob("*/case.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        assertions = document.get("assertions") if isinstance(document, dict) else None
        if not isinstance(assertions, dict):
            msg = f"{path} carries no assertions"
            raise UnreadableCaseError(msg)
        replay = assertions.get("replay", [])
        if not isinstance(replay, list) or not all(isinstance(entry, str) for entry in replay):
            msg = f"{path}'s replay is no list of scenario ids"
            raise UnreadableCaseError(msg)
        if replay:
            found[path.parent.name] = replay
    return found


def turns_per_test(cases: Path = CASES) -> dict[str, int]:
    """Each case's turns, by the case's name, from its `replay` list."""
    return {name: len(replay) for name, replay in replays(cases).items()}


def main() -> int:
    """Report the total, with the turns per test as its detail."""
    try:
        counted = turns_per_test()
    except (UnreadableCaseError, ValueError) as error:
        print(f"turns per full run: {error}", file=sys.stderr)
        return 1
    total = sum(counted.values())
    report(float(total), json.dumps({"turns_per_test": counted}))
    print(f"turns per full run: {total} ({counted})", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
