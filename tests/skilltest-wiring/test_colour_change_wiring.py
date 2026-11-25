"""The two colour-change cases, replayed through the real server with the mock harness.

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
"""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any

import replay as replay_module
from cases import CASES, REPO, Case, read_case
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
