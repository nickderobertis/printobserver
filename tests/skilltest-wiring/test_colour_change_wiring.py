"""The colour-change cases, replayed through the real server with the mock harness.

`bubbler-colour-change-planned` plants a colour change at Z 50 and shows the walls
red from there; `bubbler-colour-change-off-plan` plants the same change, and shows
them red from Z 6, far below it. Each is replayed as `test_replay_wiring` replays
every case: the real `printobserver server`, the real skill's turn, and oneharness's
published responder as the model. What is asserted is what each check is handed in
its prompt, and how each decision point is judged from what the server recorded the
scripted turn asking for.

Neither case commits its printed G-code: it is the bubbler print's slice with the
case's `fault.diff` applied. No case's LFS bytes are read here, so the slice is the
plain copy the server crate's indexing journey serves, held first to be the slice
the diff names by the sha256 its pointer carries.

`two-colour-planned-change` is a real print with two planned changes, each loaded
with the planned colour. Its case carries no `replay` until a calibration fits its
camera, so its scenarios are named here, and its G-code is the core crate's plain
copy, held to the sha256 the case's pointer carries.
"""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any

import replay as replay_module
from cases import CASES, REPO, Case, read_case, replayed_case_names
from mock_turn import ScriptedRun
from repo_checks.checks_real_prints import apply_planted, planted_gcode
from repo_checks.expect import contains, equal, truth
from test_replay_wiring import PAUSE, Scripted, mock_replay

PLANNED = "bubbler-colour-change-planned"
OFF_PLAN = "bubbler-colour-change-off-plan"
#: The plain copy of the bubbler print's slice the server crate's journeys serve.
PLAIN_SLICE = (
    REPO / "crates/printobserver-server/tests/fixtures/real-prints/bubbler-guard-and-pad.gcode"
)
POINTER_OID = re.compile(rb"\noid sha256:(?P<oid>[0-9a-f]{64})\n")
#: Where the core crate declares how far past a change's line a job still waits at it.
REACH_SOURCE = REPO / "crates/printobserver-core/src/gcode_index.rs"


def printed(name: str) -> bytes:
    """A case's printed G-code: the plain slice with the case's diff applied."""
    planted = planted_gcode(CASES / name)
    if planted is None:
        message = f"{name} commits no fault.diff to make its G-code from"
        raise AssertionError(message)
    held = planted.clean.read_bytes()
    named = POINTER_OID.search(held)
    oid = named["oid"].decode() if named else hashlib.sha256(held).hexdigest()
    equal(
        hashlib.sha256(PLAIN_SLICE.read_bytes()).hexdigest(),
        oid,
        describing=f"the plain slice as the one {name}'s diff plants into",
    )
    return apply_planted(planted, PLAIN_SLICE.read_bytes())


def replayed(name: str, second: ScriptedRun, scratch: Path) -> tuple[Case, Scripted]:
    """One case replayed: a quiet first check, then `second` at its colour-change check."""
    case = read_case(name)
    return case, mock_replay(case, [{"commands": []}, second], scratch, gcode=printed(name))


def facts_of(prompt: str) -> dict[str, Any]:
    """The facts a check's prompt carries: its one line that is the context's JSON."""
    for line in prompt.splitlines():
        if line.startswith("{") and '"bounds"' in line:
            facts: dict[str, Any] = json.loads(line)
            return facts
    message = f"the prompt carries no facts: {prompt[:200]!r}"
    raise AssertionError(message)


def situation_of(prompt: str) -> dict[str, Any]:
    """The situation a check's prompt carries: its one line that is the situation's JSON."""
    for line in prompt.splitlines():
        if line.startswith("{") and '"noticed"' in line:
            situation: dict[str, Any] = json.loads(line)
            return situation
    message = f"the prompt carries no situation: {prompt[:200]!r}"
    raise AssertionError(message)


QUIET: ScriptedRun = {"commands": []}
PAUSING: ScriptedRun = {"commands": [PAUSE], "disposition": "stop"}


def test_the_check_after_a_planned_change_names_its_colours_and_a_quiet_turn_passes(
    tmp_path: Path,
) -> None:
    """The check after a planned change names its colours, and carrying on passes it.

    The server's own check names the colour now expected, its layer and the colour
    before, and the plan names no change still waited at.
    """
    _, done = replayed(PLANNED, QUIET, tmp_path)
    after = done.turns[1]["prompt"]
    noticed = situation_of(after)["noticed"]
    for named in ["filament_change_planned_resumed", "#FF0000", "layer 272", "Z 50", "#000000"]:
        contains(noticed, named, describing="the situation of the check after the change")
    slicer = facts_of(after)["slicer"]
    equal(slicer.get("expected_color"), "#FF0000", describing="the colour expected")
    truth("change_waiting" not in slicer, describing="no change waited at once carried on")
    equal(
        [point.event_kind for point in done.replayed.points],
        ["periodic_observation", "filament_change_planned_resumed"],
        describing="what each decision point was the check of",
    )
    truth(done.replayed.passed, describing=f"every point: {done.replayed.failures()}")


def test_a_pause_after_a_planned_change_fails_it(tmp_path: Path) -> None:
    """Pausing at the planned colour is what the scenario never accepts."""
    _, done = replayed(PLANNED, PAUSING, tmp_path)
    contains(done.replayed.points[1].why_not or "", "pause", describing="the failed point")
    truth(not done.replayed.passed, describing="the replay to fail")


def test_the_off_plan_check_names_the_colour_expected_and_a_pause_passes(tmp_path: Path) -> None:
    """An off-plan colour's check is handed the colour expected there, and a pause passes it.

    Red walls far below the planned change are checked with black expected and no
    change at or just before the check's position.
    """
    case, done = replayed(OFF_PLAN, PAUSING, tmp_path)
    position = case.moments[1].file_position_bytes
    slicer = facts_of(done.turns[1]["prompt"])["slicer"]
    equal(slicer.get("expected_color"), "#000000", describing="the colour expected there")
    truth("change_waiting" not in slicer, describing="no change waited at")
    upcoming = slicer["next_change"]["offset_bytes"]
    truth(
        upcoming > position + replay_module.PLANNED_CHANGE_REACH_BYTES,
        describing=f"the only planned change, at {upcoming}, far past the check at {position}",
    )
    equal(
        done.replayed.points[1].event_kind,
        "periodic_observation",
        describing="the off-plan check, a scheduled one",
    )
    truth(done.replayed.passed, describing=f"every point: {done.replayed.failures()}")


def test_a_quiet_turn_at_an_off_plan_colour_fails_it(tmp_path: Path) -> None:
    """Carrying on over a colour change nothing planned is what the scenario refuses."""
    _, done = replayed(OFF_PLAN, QUIET, tmp_path)
    truth(done.replayed.points[1].why_not is not None, describing="the off-plan point to fail")
    truth(not done.replayed.passed, describing="the replay to fail")


def test_the_replays_reach_is_the_servers() -> None:
    """The replay stands a waiting job within the reach the core crate declares."""
    declared = re.search(
        r"pub const PLANNED_CHANGE_REACH_BYTES: u64 = (?P<bytes>\d+);",
        REACH_SOURCE.read_text(encoding="utf-8"),
    )
    truth(declared is not None, describing="the core crate declares the reach")
    if declared is not None:
        equal(
            replay_module.PLANNED_CHANGE_REACH_BYTES,
            int(declared["bytes"]),
            describing="the replay's reach",
        )


#: The real two-colour print, whose case waits for a calibration before the live
#: suite replays it, and so lists its scenarios here rather than in a `replay`.
REAL = "two-colour-planned-change"
REAL_SCENARIOS = (
    "carry-on-before-the-first-change",
    "quiet-through-the-white-wait-and-on-at-its-resume",
    "white-judged-expected-once-visible",
    "quiet-through-the-black-wait-and-on-at-its-resume",
    "black-judged-expected-once-visible",
)
#: The plain copy of that print's G-code the core crate's index tests read.
REAL_GCODE = REPO / "crates/printobserver-core/tests/fixtures/real-prints/two-colour-box.gcode"
HISTORY = ["printobserver", "history", "--print-id", "{print}", "--limit", "1000", "--json"]


def event_kind(payload: str) -> str:
    """The event kind a payload is recorded under, as its generated schema names it."""
    schema = REPO / "schemas/printobserver-core" / f"{payload}Payload.json"
    kind: str = json.loads(schema.read_text(encoding="utf-8"))["x-event-kind"]
    return kind


REACHED = event_kind("FilamentChangePlannedReached")
RESUMED = event_kind("FilamentChangePlannedResumed")
OBSERVED = event_kind("PeriodicObservation")
#: What a turn leaves in the history: none may fall inside a wait.
TURN_KINDS = {
    event_kind(payload)
    for payload in ("AgentAssessment", "ActionRequested", "ActionExecuted", "ActionRejected")
}


def real_replay(turns: list[ScriptedRun], scratch: Path) -> tuple[Case, Scripted]:
    """The real two-colour print's every scenario, replayed from its plain G-code.

    The plain copy is held first to be the bytes the case's LFS pointer names.
    """
    case = read_case(REAL, replay=REAL_SCENARIOS)
    plain = REAL_GCODE.read_bytes()
    named = POINTER_OID.search(case.gcode.read_bytes())
    held = case.gcode.read_bytes()
    oid = named["oid"].decode() if named else hashlib.sha256(held).hexdigest()
    equal(hashlib.sha256(plain).hexdigest(), oid, describing="the plain copy as the case's G-code")
    return case, mock_replay(case, turns, scratch, gcode=plain)


def test_the_real_two_colour_print_is_quiet_through_each_wait_and_carries_on(
    tmp_path: Path,
) -> None:
    """Each wait runs no turn, and each check after a change names the colour planned.

    The last check reads the server's own history, in which nothing a turn writes
    falls between a change reached and its resumption.
    """
    quiet_reading: ScriptedRun = {"commands": [HISTORY]}
    _, done = real_replay([QUIET, QUIET, QUIET, QUIET, quiet_reading], tmp_path)
    truth(REAL in replayed_case_names(), describing="the case to carry a live replay")
    equal(
        [point.event_kind for point in done.replayed.points],
        [
            OBSERVED,
            RESUMED,
            OBSERVED,
            RESUMED,
            OBSERVED,
        ],
        describing="what each decision point was the check of",
    )
    equal(len(done.replayed.ledger["turns"]), 5, describing="one turn per check, none in a wait")
    for turn, colour, named in ((1, "#FFFFFF", "layer 20"), (3, "#000000", "layer 40")):
        prompt = done.turns[turn]["prompt"]
        contains(situation_of(prompt)["noticed"], named, describing=f"check {turn}'s situation")
        equal(facts_of(prompt)["slicer"].get("expected_color"), colour, describing=f"check {turn}")
    for turn, colour in ((2, "#FFFFFF"), (4, "#000000")):
        slicer = facts_of(done.turns[turn]["prompt"])["slicer"]
        equal(slicer.get("expected_color"), colour, describing=f"the colour at check {turn}")
    # The order is the store's: each id is a UUID version 7 minted on the server's real
    # clock as the event is written, while `received_at` is the replay's clock, which
    # stands back at each change's wait and so cannot order a turn against the wait.
    events = sorted(
        json.loads(done.turns[4]["ran"][0]["out"])["events"], key=lambda event: event["id"]
    )
    kinds = [event["kind"] for event in events]
    for offset in (35454, 57998):
        reached = next(
            at
            for at, event in enumerate(events)
            if event["kind"] == REACHED and event["payload"]["change"]["offset_bytes"] == offset
        )
        resumed = kinds.index(RESUMED, reached)
        written = set(kinds[reached:resumed]) & (TURN_KINDS | {OBSERVED})
        equal(written, set(), describing=f"what a turn began from or wrote in the wait at {offset}")
    truth(done.replayed.passed, describing=f"every point: {done.replayed.failures()}")


def test_a_pause_at_the_real_prints_planned_change_fails_it(tmp_path: Path) -> None:
    """Pausing as the print carries on from a planned change is what its scenario refuses."""
    _, done = real_replay([QUIET, PAUSING], tmp_path)
    contains(done.replayed.points[1].why_not or "", "pause", describing="the failed point")
    truth(not done.replayed.passed, describing="the replay to fail")
