"""The `printobserver` skill, replaying each real print the way production runs it.

One test per case with a `replay`, discovered from the cases on disk. Each feeds
its events in order through the real server and the shipped harness, as one
continued conversation, and passes only when every decision point does. There is
no rate, no retry and no list of known misses: a failure is fixed at its cause.

`just skilltest -k <case>` runs one. Set `SKILLTEST_RUN_DIR` to keep each test's
record, which is what `just skilltest-record` folds into `results/latest.json`.
"""

from __future__ import annotations

import json
import os
import shutil
from pathlib import Path

import pytest
from cases import read_case, replayed_case_names
from replay import Replay, harness_token
from repo_checks import expect

RUN_DIR = os.environ.get("SKILLTEST_RUN_DIR", "").strip()


@pytest.mark.parametrize("name", replayed_case_names())
def test_the_agent_supervises_the_print(name: str) -> None:
    """Every decision point meets an accepted outcome and no step it must never take."""
    program = shutil.which("claude")
    if program is None:
        pytest.fail("the shipped harness is not installed: no `claude` is on PATH")
    if harness_token() is None:
        pytest.fail("no credential for the harness: set CLAUDE_CODE_OAUTH_TOKEN, or sign in")
    case = read_case(name)
    replay = Replay(case, [program], gcode=case.gcode.read_bytes())
    try:
        replayed = replay.run()
        if RUN_DIR:
            directory = Path(RUN_DIR)
            directory.mkdir(parents=True, exist_ok=True)
            (directory / f"{name}.json").write_text(
                json.dumps(replayed.as_record(), indent=2), encoding="utf-8"
            )
            (directory / f"{name}.transcript.jsonl").write_text(
                "".join(json.dumps(line) + "\n" for line in replayed.transcript), encoding="utf-8"
            )
        expect.equal(
            [session["harness_identity"] for session in replayed.ledger["sessions"]],
            ["claude-code"],
            describing=f"{name}'s one conversation",
        )
        # A record with nothing in it is the harness's output or transcript having
        # moved from where the replay reads them, which it must not pass over.
        for point in replayed.points:
            expect.truth(
                point.usage and point.models and point.commands,
                describing=f"{point.moment.id}'s turn to report its usage, models and commands",
            )
        expect.truth(replayed.passed, describing=f"every decision point:\n{replayed.failures()}")
    finally:
        replay.clean_up()
