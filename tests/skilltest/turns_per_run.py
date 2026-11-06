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


def turns_per_test(cases: Path = CASES) -> dict[str, int]:
    """Each case's turns, by the case's name, from its `replay` list."""
    counted = {}
    for path in sorted(cases.glob("*/case.json")):
        replay = json.loads(path.read_text(encoding="utf-8"))["assertions"].get("replay")
        if replay:
            counted[path.parent.name] = len(replay)
    return counted


def main() -> int:
    """Report the total, with the turns per test as its detail."""
    counted = turns_per_test()
    total = sum(counted.values())
    report(float(total), json.dumps({"turns_per_test": counted}))
    print(f"turns per full run: {total} ({counted})", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
