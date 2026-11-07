"""What keeps this project's live tests from firing by accident.

Each runs a real harness and spends real model turns. The project graph keeps
them out of the gate: this directory's only test target is `skilltest`, and no
gate recipe fans out over that name. This keeps them from a developer's bare
`pytest` over the tree: they are collected only when the run opted in, as the
`skilltest` target and `just skilltest` do.
"""

from __future__ import annotations

import os
from pathlib import Path

collect_ignore = [] if os.environ.get("SKILLTEST_E2E") == "1" else ["test_replay.py"]


def pytest_ignore_collect(collection_path: Path) -> bool | None:
    """Leave the live module out unless the run opted in, whatever pytest was pointed at."""
    if collection_path.name in collect_ignore:
        return True
    return None
