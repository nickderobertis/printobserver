"""The copy of the tree every check here is pointed at is a whole one.

The checked-in schema tree is rewritten on disk by one journey in the adapter's
suite while every other suite runs beside it, and a copy of that tree taken
file by file in that moment carries an empty schema. So a copy is taken under
the shared schema lock, and one started while the exclusive lock is held — as
that journey holds it — waits for it to go.
"""

from __future__ import annotations

import fcntl
import json
import threading
import time
from pathlib import Path

from repo_checks.expect import equal, truth
from treecopy import REPO_ROOT, SchemaTreeLock, copy_tree

ASSESSMENT_SCHEMA = "schemas/printobserver-supervisor-api/AgentAssessment.json"


def test_a_copy_started_while_a_schema_is_being_rewritten_waits_for_it(tmp_path: Path) -> None:
    """The copy neither starts over a half-written tree nor carries one."""
    rewriting = SchemaTreeLock()
    fcntl.flock(rewriting.file.fileno(), fcntl.LOCK_EX)
    copied: list[Path] = []
    copying = threading.Thread(target=lambda: copied.append(copy_tree(tmp_path / "copy")))
    try:
        copying.start()
        time.sleep(1.0)
        truth(copying.is_alive(), describing="the copy to wait while the schema tree is held")
        truth(
            not any((tmp_path / "copy").iterdir()),
            describing="nothing to have been copied while the schema tree is held",
        )
    finally:
        rewriting.release()
    copying.join(timeout=120)

    truth(not copying.is_alive(), describing="the copy to finish once the lock is released")
    equal(
        json.loads((copied[0] / ASSESSMENT_SCHEMA).read_text(encoding="utf-8")),
        json.loads((REPO_ROOT / ASSESSMENT_SCHEMA).read_text(encoding="utf-8")),
        describing="the schema the copy carries",
    )
