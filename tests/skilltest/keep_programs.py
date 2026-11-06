"""Keep the suite's own build of its programs where no ordinary build overwrites them.

`build-programs` builds with the `replay-clock` feature, so its `printobserver` differs
from the one every other target builds into the same directory, under the same name.
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

TARGET = Path(__file__).resolve().parents[2] / "target"
SUFFIX = ".exe" if sys.platform == "win32" else ""


def main() -> int:
    """Copy each program out of cargo's output directory."""
    kept = TARGET / "skilltest"
    kept.mkdir(exist_ok=True)
    for name in (
        "printobserver",
        "printobserver-server-responder",
        "printobserver-server-turn-launcher",
    ):
        shutil.copy2(TARGET / "debug" / f"{name}{SUFFIX}", kept / f"{name}{SUFFIX}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
