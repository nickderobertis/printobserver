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
from pathlib import Path

import pytest
from cases import is_start_case, read_case, read_start_case, replayed_case_names
from replay import Replay, harness_token, kept_harness, refuse_a_pointer
from repo_checks import expect
from starts import StartsReplay

RUN_DIR = os.environ.get("SKILLTEST_RUN_DIR", "").strip()


@pytest.mark.parametrize("name", replayed_case_names())
def test_the_agent_supervises_the_print(name: str) -> None:
    """Every decision point meets an accepted outcome and no step it must never take."""
    program = kept_harness()
    if program is None:
        pytest.fail("the shipped harness is not installed: no `claude` is on PATH")
    if harness_token() is None:
        pytest.fail("no credential for the harness: set CLAUDE_CODE_OAUTH_TOKEN, or sign in")
    if is_start_case(name):
        _the_operators_agent_decides_each_start(name, program)
        return
    case = read_case(name)
    replay = Replay(
        case, [program], gcode=refuse_a_pointer(case.gcode.read_bytes(), case.name, case.gcode.name)
    )
    try:
        replayed = replay.run()
        _keep(name, replayed.as_record(), replayed.transcript)
        expect.equal(
            [session["harness_identity"] for session in replayed.ledger["sessions"]],
            ["claude-code"],
            describing=f"{name}'s one conversation",
        )
        # A record with nothing in it is the harness's output or transcript having
        # moved from where the replay reads them, which it must not pass over.
        for point in replayed.points:
            expect.truth(
                point.usage and point.models,
                describing=f"{point.moment.id}'s turn to report its usage and models",
            )
        expect.truth(
            any(point.commands for point in replayed.points),
            describing="the transcript to say what the agent ran",
        )
        expect.truth(
            any(point.opened for point in replayed.points),
            describing="the transcript to say what the agent opened",
        )
        expect.equal(
            {session: [] for session in replayed.compound_commands()},
            replayed.compound_commands(),
            describing="each printobserver command run on its own",
        )
        expect.truth(replayed.passed, describing=f"every decision point:\n{replayed.failures()}")
    finally:
        replay.clean_up()


def _keep(name: str, record: object, transcript: list[dict[str, object]]) -> None:
    """Write a test's record where `just skilltest-record` folds it from, when asked to."""
    if not RUN_DIR:
        return
    directory = Path(RUN_DIR)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{name}.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    (directory / f"{name}.transcript.jsonl").write_text(
        "".join(json.dumps(line) + "\n" for line in transcript), encoding="utf-8"
    )


def _the_operators_agent_decides_each_start(name: str, program: str) -> None:
    """Every start request starts on the sheet the frame shows, or tells a person why not."""
    replay = StartsReplay(read_start_case(name), [program])
    try:
        replayed = replay.run()
        _keep(name, replayed.as_record(), replayed.transcript)
        for point in replayed.points:
            expect.truth(
                point.report.usage and point.report.models,
                describing=f"{point.request.id}'s conversation to report its usage and models",
            )
        # A request whose frame settles it needs no command, but a day of them does.
        expect.truth(
            any(point.report.commands for point in replayed.points),
            describing="the transcript to say what the agent ran",
        )
        expect.equal(
            {session: [] for session in replayed.sessions},
            replayed.compound_commands(),
            describing="each printobserver command run on its own",
        )
        expect.truth(replayed.passed, describing=f"every start request:\n{replayed.failures()}")
    finally:
        replay.clean_up()
