"""The real-print replay's machinery, with oneharness's published mock harness as the model.

Every test here that replays runs the real `printobserver server` over a real
case, with the published responder answering at the harness's process boundary
and its scripted commands running against that server through the real program.
Nothing is doubled above that but what `doubles` doubles for the live suite, and
no case's LFS bytes are read: a frame is this directory's own plain fixture,
stamped with the name of the frame it stands for.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import classify
import pytest
import record
from cases import (
    CASES,
    REPO,
    Case,
    MachineState,
    Moment,
    Request,
    ScenarioId,
    Step,
    Trigger,
    read_case,
    replayed_case_names,
)
from mock_turn import ScriptedRun, Seen
from replay import (
    Replay,
    Replayed,
    ReplayError,
    delivered,
    harness_token,
    program,
    refuse_a_pointer,
    service_config,
)
from replay import Secrets as Minted
from repo_checks.__main__ import main as repo_check
from repo_checks.checks_real_prints import printed_gcode, stored_size
from repo_checks.expect import contains, equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.shell import run
from turns_per_run import turns_per_test

HERE = Path(__file__).resolve().parent
MOCK_TURN = HERE / "mock_turn.py"
FRAME = (HERE / "fixtures" / "frame.jpg").read_bytes()
#: The most a G-code line of these slices holds, which a `progress` position may be
#: off by: it is snapped to the start of the line after the exact byte.
LONGEST_GCODE_LINE = 256
#: The replays section 2 of the suite's design fixes, by case.
REPLAYS = {
    "jar-top-wrong-material-detach-spaghetti": [
        "wall-building-cleanly",
        "wall-top-porous-and-stringing",
        "part-detached-and-moved",
    ],
    "jar-top-wrong-material-first-layer-detach": [
        "first-layer-before-the-failure",
        "ring-dragged-off-its-place",
    ],
    "petg-spool-jam-air-print": ["first-layers-healthy", "air-print-parts-stopped-growing"],
    "petg-clip-detach-and-stringing-web": [
        "clip-came-loose",
        "stringing-web-warning",
        "detector-paused-for-nest",
        "warning-after-resume",
    ],
    "petg-printer-reset-nozzle-on-part": [
        "infill-floor-warning-false-alarm",
        "nozzle-left-on-part-after-reset",
        "gantry-low-over-remaining-bin",
    ],
    "under-extrusion-lace": ["carry-on-healthy-before-fault", "porous-walls"],
    "fan-cut-bridge": ["carry-on-healthy-before-fault", "bridge-sagging-uncooled"],
    "jar-top-petg-healthy": [
        "carry-on-004-other",
        "carry-on-013-other",
        "carry-on-024-support",
        "carry-on-028-bridge",
        "carry-on-031-other",
        "carry-on-042-other",
        "carry-on-049-support",
        "carry-on-059-other",
    ],
}
#: What a scripted turn runs to read the print and to look at it.
CONTEXT = ["printobserver", "context", "--print-id", "{print}", "--json"]
LOOK = ["printobserver", "look", "--print-id", "{print}", "--json"]
PAUSE = [
    "printobserver",
    "pause",
    "--print-id",
    "{print}",
    "--actor",
    "{actor}",
    "--reason",
    "a scripted reason",
]


def stamped(name: str) -> bytes:
    """The fixture picture, carrying the name of the frame it stands for."""
    comment = name.encode()
    return FRAME[:2] + b"\xff\xfe" + (len(comment) + 2).to_bytes(2, "big") + comment + FRAME[2:]


def frame_name(picture: bytes) -> str:
    """The frame a picture stands for, read back out of it wherever it landed.

    The camera double numbers its answers in a comment of its own, first, and the
    stamp is the comment after it.
    """
    at = 2
    comments = []
    while picture[at : at + 2] == b"\xff\xfe":
        length = int.from_bytes(picture[at + 2 : at + 4], "big")
        comments.append(picture[at + 4 : at + 2 + length].decode())
        at += 2 + length
    return comments[-1]


@dataclass
class Scripted:
    """A finished mock replay: what the server recorded and what the mock saw."""

    replayed: Replayed
    turns: list[Seen]
    images: dict[str, str]

    def seen_frame(self, turn: int, look: int) -> str:
        """The frame the `look`th scripted look of one turn answered."""
        ran = [entry for entry in self.turns[turn]["ran"] if entry["words"][1] == "look"]
        return self.images[json.loads(ran[look]["out"])["image_path"]]


def mock_replay(
    case: Case, turns: list[ScriptedRun], scratch: Path, gcode: bytes | None = None
) -> Scripted:
    """Run one case's whole replay, its turns scripted, and read back what happened."""
    script = scratch / "script.json"
    script.write_text(
        json.dumps(
            {
                "session": case.name,
                "responder": str(program("printobserver-server-responder")),
                "turns": turns,
            }
        ),
        encoding="utf-8",
    )
    replay = Replay(
        case, [sys.executable, str(MOCK_TURN), str(script)], picture=stamped, gcode=gcode
    )
    try:
        replayed = replay.run()
        images = {
            str(path): frame_name(path.read_bytes())
            for path in replay.state.glob("images/*/*")
            if path.is_file()
        }
    finally:
        replay.clean_up()
    seen = (scratch / "seen.jsonl").read_text(encoding="utf-8").splitlines()
    return Scripted(replayed, [json.loads(line) for line in seen], images)


@pytest.fixture
def scratch(tmp_path: Path) -> Path:
    """Where a mock turn keeps what it saw."""
    return tmp_path


def test_the_cases_carry_exactly_the_replays_the_suite_was_given() -> None:
    """Each named case's list, in order, and the bubbler's every scenario in time order."""
    for name, replay in REPLAYS.items():
        equal([moment.id for moment in read_case(name).moments], replay, describing=name)
    bubbler = read_case("bubbler-guard-and-pad-pla-healthy")
    equal(len(bubbler.moments), 18)
    equal(
        [moment.at for moment in bubbler.moments],
        sorted(moment.at for moment in bubbler.moments),
    )
    equal(sorted([*REPLAYS, bubbler.name]), replayed_case_names())


def _scenarios() -> Iterator[tuple[str, dict[str, Any], dict[str, Any]]]:
    for path in sorted(CASES.glob("*/case.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        for scenario in document["assertions"]["scenarios"]:
            yield path.parent.name, document, scenario


def test_a_recorded_position_is_the_job_snapshot_of_its_recorded_event() -> None:
    """Every `recorded` position, over every scenario, against the history that holds it."""
    checked = 0
    for name, document, scenario in _scenarios():
        if scenario.get("file_position_source") != "recorded":
            continue
        history = json.loads((CASES / name / "printobserver-history.json").read_text("utf-8"))
        owner = next(
            entry["event_id"]
            for entry in document["agent_images"]
            if entry["file"] == scenario["event_image"]
        )
        event = next(event for event in history["events"] if event["id"] == owner)
        equal(
            scenario["file_position_bytes"],
            event["payload"]["job"]["file_position_bytes"],
            describing=f"{name}/{scenario['id']}",
        )
        checked += 1
    truth(checked > 0, describing="a recorded position to check")


#: The one alert whose nearest frame only its label names.
ALERT_FRAME = {
    "spaghetti-small-nest/warning-then-detector-pause": "04-nest-at-obico-warning-p48.jpg"
}


def _frame_progress(document: dict[str, Any], scenario: dict[str, Any]) -> float:
    """The progress a scenario's frame stands for.

    Its own; for an alert's picture or a later frame with none, that of the frame
    its label names or the latest dated frame at or before it; and where the case
    dates nothing, where its fault starts.
    """
    frames = {frame["file"]: frame for frame in document["frames"]}
    own = frames.get(scenario["event_image"], {}).get("progress_pct")
    if own is not None:
        return own
    named = ALERT_FRAME.get(f"{document['case']}/{scenario['id']}")
    if named:
        return frames[named]["progress_pct"]
    at = scenario["at"].replace("T", " ")
    earlier = [
        frame["progress_pct"]
        for frame in document["frames"]
        if frame.get("progress_pct") is not None and frame.get("time") and frame["time"] <= at
    ]
    return earlier[-1] if earlier else document["fault"]["starts_at_progress_pct"]


def test_a_progress_position_is_the_frames_progress_through_its_gcode() -> None:
    """Every `progress` position lies within one G-code line of progress times size."""
    checked = 0
    for name, document, scenario in _scenarios():
        if scenario.get("file_position_source") != "progress":
            continue
        gcode = printed_gcode(CASES / name)
        if gcode is None:
            pytest.fail(f"{name} names a position and carries no G-code")
        size = stored_size(gcode)
        expected = _frame_progress(document, scenario) / 100 * size
        off = abs(scenario["file_position_bytes"] - expected)
        truth(
            off <= LONGEST_GCODE_LINE,
            describing=f"{name}/{scenario['id']}'s position to be within a line, off by {off}",
        )
        checked += 1
    truth(checked > 0, describing="a progress position to check")


def _case_tree(root: Path, name: str, edit: Callable[[dict[str, Any]], None]) -> Path:
    """A repository holding one real case with its `case.json` edited, and what the check reads."""
    (root / "tests" / "real-prints").mkdir(parents=True)
    shutil.copy2(CASES / "case.schema.json", root / "tests" / "real-prints")
    shutil.copy2(CASES / "service-config.toml", root / "tests" / "real-prints")
    shutil.copytree(CASES / name, root / "tests" / "real-prints" / name)
    shutil.copytree(REPO / "schemas", root / "schemas")
    path = root / "tests" / "real-prints" / name / "case.json"
    document = json.loads(path.read_text(encoding="utf-8"))
    edit(document)
    path.write_text(json.dumps(document), encoding="utf-8")
    return root


def _scenario_named(document: dict[str, Any], scenario: str) -> dict[str, Any]:
    return next(s for s in document["assertions"]["scenarios"] if s["id"] == scenario)


LACE = "under-extrusion-lace"
LAST = "porous-walls"


def _unknown_id(document: dict[str, Any]) -> None:
    document["assertions"]["replay"].append("no-such-scenario")


def _out_of_order(document: dict[str, Any]) -> None:
    document["assertions"]["replay"].reverse()


def _past_the_end(document: dict[str, Any]) -> None:
    _scenario_named(document, LAST)["file_position_bytes"] = 10**12


def _missing(document: dict[str, Any]) -> None:
    scenario = _scenario_named(document, LAST)
    del scenario["file_position_bytes"]
    del scenario["file_position_source"]


@pytest.mark.parametrize(
    ("edit", "named"),
    [
        (_unknown_id, "replay names no-such-scenario"),
        (_out_of_order, "not strictly increasing"),
        (_past_the_end, "past the end of lace-test.gcode"),
        (_missing, "names no file_position_bytes"),
    ],
)
def test_a_real_case_broken_one_way_is_refused_by_name(
    edit: Callable[[dict[str, Any]], None],
    named: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The real check, over a copy of a real case with one thing wrong."""
    root = _case_tree(tmp_path, LACE, edit)
    equal(repo_check(["real-prints-replay", "--root", str(root)]), 1)
    printed = capsys.readouterr()
    contains(printed.out + printed.err, named)


def test_every_case_passes_the_replay_check(capsys: pytest.CaptureFixture[str]) -> None:
    """The committed tree carries none of those."""
    equal(repo_check(["real-prints-replay", "--root", str(REPO)]), 0)
    printed = capsys.readouterr()
    equal(printed.out + printed.err, "")


def test_the_configuration_is_the_installers_with_only_the_blanks_filled(tmp_path: Path) -> None:
    """Every table an operator does not fill in reads as the per-user installer writes it."""
    written = service_config(tmp_path, "127.0.0.1:1", "http://o", "http://c", "http://b", Minted())
    for line in (
        'harness = "claude-code"',
        "agent_min_interval_s = 30",
        'agent = ["pause", "set_feedrate_factor", "set_flowrate_factor",',
    ):
        contains(written, line)


def test_one_session_spans_the_print_and_each_check_stands_at_its_moment(scratch: Path) -> None:
    """The port opens one conversation, and each turn is handed its frame and its position."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    ledger = done.replayed.ledger
    equal(len(ledger["sessions"]), 1)
    equal(len(ledger["turns"]), len(case.moments))
    equal(len({turn["session_name"] for turn in ledger["turns"]}), 1)
    arguments = [turn["arguments"] for turn in done.turns]
    truth("--resume" not in arguments[0], describing="the first event to open the session")
    truth("--resume" in arguments[1], describing="the later event to continue it")
    for turn, moment in zip(done.turns, case.moments, strict=True):
        answered = json.loads(turn["ran"][0]["out"])
        equal(answered["context"]["job"]["file_position_bytes"], moment.file_position_bytes)
        equal(done.images[answered["image_path"]], moment.event_image)


def test_looks_answer_the_next_frames_in_capture_order_without_repeating(scratch: Path) -> None:
    """Over a window with frames to spare, every look is a newer frame than the last."""
    case = read_case("petg-clip-detach-and-stringing-web")
    turn: ScriptedRun = {"commands": [LOOK, LOOK, LOOK]}
    done = mock_replay(case, [turn], scratch)
    names = [frame.file for frame in case.frames]
    start = names.index(case.moments[0].event_image)
    equal(
        [done.seen_frame(0, look) for look in range(3)],
        names[start + 1 : start + 4],
    )


def test_a_look_never_passes_the_next_events_moment(scratch: Path) -> None:
    """With no frame left before the next event a look repeats the newest; it keeps its own."""
    case = read_case("fan-cut-bridge")
    done = mock_replay(case, [{"commands": [LOOK, LOOK]}], scratch)
    first, second = case.moments
    equal([done.seen_frame(0, 0), done.seen_frame(0, 1)], [first.event_image, first.event_image])
    jobs = [json.loads(entry["out"])["job"] for entry in done.turns[0]["ran"]]
    truth(
        jobs[-1]["file_position_bytes"] < second.file_position_bytes,
        describing="nothing a turn reads to pass the next event's moment",
    )
    # The event's frame, the two looks, and then the next event's own frame.
    equal(done.replayed.fetches[:4], [first.event_image] * 3 + [second.event_image])


def test_a_recorded_alert_reaches_the_agent_at_its_time(scratch: Path) -> None:
    """Posted to the real webhook, it is the next turn's event, with its own picture and state."""
    case = read_case("petg-clip-detach-and-stringing-web")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    prompts = [turn["prompt"] for turn in done.turns]
    contains(prompts[0], '"kind": "periodic_observation"')
    for prompt in prompts[1:]:
        contains(prompt, '"kind": "obico_failure_alert"')
    contains(prompts[2], '"detector_paused_the_print": true')
    equal(
        [point.event_kind for point in done.replayed.points],
        [
            "periodic_observation",
            "obico_failure_alert",
            "obico_failure_alert",
            "obico_failure_alert",
        ],
    )
    pictures = [done.images[json.loads(turn["ran"][0]["out"])["image_path"]] for turn in done.turns]
    equal(pictures[1:], [moment.event_image for moment in case.moments[1:]])


def test_a_pause_at_a_carry_on_point_fails_it_and_its_test(scratch: Path) -> None:
    """What the server recorded the agent asking for is what is judged."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(case, [{"commands": [PAUSE]}], scratch)
    carry_on, fault = done.replayed.points
    contains(carry_on.why_not or "", "it must never pause")
    equal(fault.why_not, None)
    truth(not done.replayed.passed, describing="the test to fail on one failed decision point")


def test_an_accepted_action_passes_the_test(scratch: Path) -> None:
    """Carrying on at the carry-on point and pausing at the fault is the whole test passing."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(case, [{"commands": [CONTEXT]}, {"commands": [PAUSE]}], scratch)
    truth(done.replayed.passed, describing=f"every decision point: {done.replayed.failures()}")


def test_a_printer_that_cannot_be_read_still_gets_its_scheduled_check(scratch: Path) -> None:
    """After the reset the machine answers nothing, and the check comes all the same."""
    case = read_case("petg-printer-reset-nozzle-on-part")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    equal(
        [point.event_kind for point in done.replayed.points],
        ["obico_failure_alert", "periodic_observation", "periodic_observation"],
    )


def test_nothing_the_agent_can_read_says_it_is_under_test(scratch: Path) -> None:
    """The turn's environment and workspace name no test, runner or repository."""
    case = read_case("under-extrusion-lace")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    forbidden = ("pytest", "skilltest", "PYTEST_", "UV_", "VIRTUAL_ENV", str(REPO), "real-prints")
    for turn in done.turns:
        environment = {
            k: v for k, v in turn["environment"].items() if k != "CLAUDE_CODE_OAUTH_TOKEN"
        }
        said = json.dumps([environment, turn["cwd"], turn["workspace"], turn["prompt"]])
        for word in forbidden:
            truth(word not in said, describing=f"the turn to say nothing of {word}")
    equal(sorted(done.turns[0]["workspace"]), ["SKILL.md", "reference"])


def test_the_runner_makes_exactly_one_turn_per_replay_entry(scratch: Path) -> None:
    """The turns the mock received are the count the budget's command reports for the case."""
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    equal(len(done.turns), turns_per_test()[case.name])


def _moment_accepting(*outcome: Step, never: tuple[Step, ...] = ()) -> Moment:
    return Moment(
        id=ScenarioId("a-moment"),
        at="2026-01-01T00:00:00",
        trigger=Trigger.PERIODIC_OBSERVATION,
        event_image="frame.jpg",
        event_id=None,
        printer_state=MachineState.PRINTING,
        file_position_bytes=0,
        accept_any_of=(outcome,),
        never=never,
        summary="a moment",
    )


def test_an_outcome_is_met_only_by_its_values_in_its_order() -> None:
    """A wrong value, a wrong order and an argument-pinned never step each fail a decision."""
    pause = Step("pause")
    stop = Step("acknowledge-failure", {"disposition": "stop"})
    moment = _moment_accepting(pause, stop)
    pausing = Request("pause", {})
    stopping = Request("acknowledge-failure", {"disposition": "stop"})
    watching = Request("acknowledge-failure", {"disposition": "watch"})
    equal(moment.verdict([pausing, stopping]), None)
    truth(moment.verdict([stopping, pausing]) is not None, describing="the wrong order to fail")
    truth(moment.verdict([pausing, watching]) is not None, describing="the wrong value to fail")
    factor = _moment_accepting(Step("set-flowrate-factor", {"factor": 1.0}))
    equal(factor.verdict([Request("set-flowrate-factor", {"factor": 1})]), None)
    truth(
        factor.verdict([Request("set-flowrate-factor", {"factor": 0.9})]) is not None,
        describing="another number to fail",
    )
    forbidden = _moment_accepting(never=(stop,))
    equal(forbidden.verdict([watching]), None)
    contains(forbidden.verdict([stopping]) or "", "must never")


def test_a_plain_pytest_collects_no_live_test_and_an_opted_in_one_does() -> None:
    """Nothing fires by accident, and the opt-in reaches every case."""
    command = ["uv", "run", "-q", "pytest", "--collect-only", "-q", "tests/skilltest"]
    environment = {k: v for k, v in os.environ.items() if k != "SKILLTEST_E2E"}
    plain = run(command, cwd=REPO, env=environment)
    truth("test_replay" not in plain.stdout, describing=f"no live test in {plain.stdout}")
    opted = run(command, cwd=REPO, env={**environment, "SKILLTEST_E2E": "1"}, check=True)
    contains(opted.stdout, f"test_replay.py: {len(replayed_case_names())}")


def test_a_turns_assessment_is_attributed_to_its_own_decision_point(scratch: Path) -> None:
    """Each decision point carries what the agent answered that turn, whatever its order."""
    case = read_case("petg-clip-detach-and-stringing-web")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    for point in done.replayed.points:
        truth(point.assessment, describing=f"{point.moment.id} to carry its assessment")
    summaries = [turn["assessment"]["summary"] for turn in done.turns]
    equal(
        [point.assessment["summary"] for point in done.replayed.points if point.assessment],
        summaries,
    )


def test_the_printer_reports_the_temperatures_the_gcode_asks_for_at_its_position(
    scratch: Path,
) -> None:
    """The stand-in serves the G-code's own heater targets, through the real adapter."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    before = case.moments[1].file_position_bytes - 100
    gcode = b"M140 S60\nM104 S220\n" + b";\n" * (before // 2) + b"M104 S240\n"
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch, gcode=gcode)
    tools = [json.loads(turn["ran"][0]["out"])["context"]["printer"] for turn in done.turns]
    equal([printer["tools"][0]["target_c"]["value"] for printer in tools], [220.0, 240.0])
    equal(tools[0]["bed"]["target_c"]["value"], 60.0)


def test_an_unfetched_frame_is_refused_naming_the_recipe() -> None:
    """A checkout's pointer is no picture, and the replay says what to run."""
    case = read_case("under-extrusion-lace")
    pointer_bytes = (
        b"version https://git-lfs.github.com/spec/v1\noid sha256:" + b"a" * 64 + b"\nsize 3\n"
    )
    with pytest.raises(ReplayError, match="real-prints-fetch"):
        refuse_a_pointer(pointer_bytes, case.name, case.frames[0].file)


def test_a_replayed_alert_the_server_refuses_stops_the_replay() -> None:
    """A webhook answering anything but acceptance is no delivered alert."""
    equal(delivered(202), True)
    equal(delivered(403), False)


def test_a_run_the_engine_re_prompts_stays_one_turn_at_its_own_moment(scratch: Path) -> None:
    """The engine's second run of one turn neither ends it nor moves the replay on."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(
        case,
        [
            {
                "commands": [CONTEXT],
                "unreadable": True,
                "usage": {"input_tokens": 1, "output_tokens": 2},
            },
            {"commands": [LOOK], "usage": {"input_tokens": 10, "output_tokens": 20}},
            {"commands": [CONTEXT]},
        ],
        scratch,
    )
    usage = done.replayed.points[0].usage
    equal(usage, {"input_tokens": 11, "output_tokens": 22})
    equal(len(done.replayed.ledger["turns"]), 2)
    first, second = case.moments
    equal(done.images[json.loads(done.turns[0]["ran"][0]["out"])["image_path"]], first.event_image)
    equal(
        done.seen_frame(1, 0),
        case.frames[case.frame_at_or_before(first.at, first.event_image) + 1].file,
    )
    equal(done.images[json.loads(done.turns[2]["ran"][0]["out"])["image_path"]], second.event_image)


def test_the_record_command_folds_a_finished_run_or_runs_the_suite(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Given a directory it folds that; given none it runs the suite, and exits as that did."""
    written = tmp_path / "latest.json"
    monkeypatch.setattr(record, "RESULT", written)
    monkeypatch.setattr(record, "REPO", tmp_path)
    monkeypatch.setattr(record, "commit", lambda: "a-commit")
    finished = tmp_path / "finished"
    finished.mkdir()
    test_record = {
        "test": "fan-cut-bridge",
        "passed": True,
        "turns": 0,
        "sessions": [],
        "decision_points": [],
    }
    (finished / "fan-cut-bridge.json").write_text(json.dumps(test_record), encoding="utf-8")
    equal(record.main([str(finished)]), 0)
    equal(json.loads(written.read_text("utf-8"))["detail"]["commit"], "a-commit")
    failed = tmp_path / "failed"
    failed.mkdir()
    (failed / "fan-cut-bridge.json").write_text(
        json.dumps({**test_record, "passed": False}), encoding="utf-8"
    )
    equal(record.main([str(failed)]), 1)
    empty = tmp_path / "empty"
    empty.mkdir()
    equal(record.main([str(empty)]), 1)
    for name, unusable in (("list", []), ("truthy", {**test_record, "passed": "false"})):
        unreadable = tmp_path / name
        unreadable.mkdir()
        (unreadable / "a.json").write_text(json.dumps(unusable), encoding="utf-8")
        equal(record.main([str(unreadable)]), 1, describing=name)
    # A `just` of this test's own, standing where the real recipe would run the suite.
    fake = tmp_path / "bin"
    fake.mkdir()
    suffix = ".exe" if sys.platform == "win32" else ""
    shutil.copy2(program("printobserver-server-responder"), fake / f"just{suffix}")
    monkeypatch.setenv("PATH", f"{fake}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setenv("MOCK_EXIT", "7")
    equal(record.main([]), 7)
    equal(json.loads(written.read_text("utf-8"))["figures"], {"turns": 0})


def test_the_harness_credential_is_the_variable_then_the_hosts_sign_in(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The variable wins; a sign-in is read for its access token; anything else is none."""
    monkeypatch.setenv("CLAUDE_CONFIG_DIR", str(tmp_path))
    monkeypatch.setenv("CLAUDE_CODE_OAUTH_TOKEN", "from-the-variable")
    equal(harness_token(), "from-the-variable")
    monkeypatch.delenv("CLAUDE_CODE_OAUTH_TOKEN")
    equal(harness_token(), None)
    sign_in = tmp_path / ".credentials.json"
    sign_in.write_text('{"claudeAiOauth": {"accessToken": "from-the-file"}}', encoding="utf-8")
    equal(harness_token(), "from-the-file")
    for unreadable in ("not json", "[]", '{"claudeAiOauth": 3}', '{"claudeAiOauth": {}}'):
        sign_in.write_text(unreadable, encoding="utf-8")
        equal(harness_token(), None, describing=unreadable)


def test_the_record_carries_every_turns_usage_with_the_compaction_marked(
    scratch: Path, tmp_path: Path
) -> None:
    """`record` over a mock run writes each usage as the mock reported it."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    usages = [
        {"input_tokens": 101, "output_tokens": 11, "cache_read_input_tokens": 7},
        {"input_tokens": 202, "output_tokens": 22, "cache_read_input_tokens": 9},
    ]
    done = mock_replay(
        case,
        [
            {"commands": [CONTEXT], "usage": usages[0]},
            {"commands": [PAUSE], "usage": usages[1], "compact": True},
        ],
        scratch,
    )
    run_directory = tmp_path / "run"
    run_directory.mkdir()
    (run_directory / f"{case.name}.json").write_text(
        json.dumps(done.replayed.as_record()), encoding="utf-8"
    )
    written = record.folded(run_directory, "a-commit")
    equal(written["figures"], {"turns": 2})
    detail = written["detail"]
    equal(detail["commit"], "a-commit")
    equal(detail["harness"], "claude-code")
    equal(detail["models"], ["mock-model"])
    equal(detail["compactions"], 1)
    equal(detail["turns_the_cases_count"], 2)
    points = detail["tests"][0]["decision_points"]
    equal([point["usage"] for point in points], usages)
    equal([point["compacted"] for point in points], [False, True])
    contains(points[1]["commands"][0], "printobserver pause")


@dataclass
class Clone:
    """A scratch clone with a committed base, for building real diffs in."""

    root: Path
    base: str

    def commit(self, changes: dict[str, str]) -> str:
        """Write the files, commit them on a branch of the base, and answer the commit."""
        for name, text in changes.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        run(["git", "add", "-A"], cwd=self.root, check=True)
        run(["git", "commit", "-q", "-m", "chore: change"], cwd=self.root, check=True)
        return run(["git", "rev-parse", "HEAD"], cwd=self.root, check=True).stdout.strip()


LOCK_TEMPLATE = """version = 4

[[package]]
name = "oneharness"
version = "{oneharness}"

[[package]]
name = "serde"
version = "{serde}"
"""


@pytest.fixture
def clone(tmp_path: Path) -> Clone:
    """A repository holding the real policy, a lock and an install template."""
    root = tmp_path / "clone"
    init_repository(root)
    shutil.copy2(REPO / classify.POLICY, root / classify.POLICY)
    (root / classify.LOCK).write_text(
        LOCK_TEMPLATE.format(oneharness="1.0.0", serde="1.0.0"), encoding="utf-8"
    )
    (root / "scripts").mkdir()
    (root / "scripts" / "install.sh").write_text(
        '[supervisor]\nharness = "claude-code"\n\n[ingress]\nshared_secret = ""\n',
        encoding="utf-8",
    )
    run(["git", "add", "-A"], cwd=root, check=True)
    run(["git", "commit", "-q", "-m", "chore: base"], cwd=root, check=True)
    base = run(["git", "rev-parse", "HEAD"], cwd=root, check=True).stdout.strip()
    return Clone(root, base)


@pytest.mark.parametrize(
    "path",
    [
        "skills/printobserver/SKILL.md",
        "crates/printobserver-oneharness/src/turn.rs",
        "crates/printobserver-core/src/context.rs",
        "crates/printobserver-core/src/events.rs",
        "tests/skilltest/replay.py",
        "tests/real-prints/under-extrusion-lace/case.json",
    ],
)
def test_a_change_under_an_owed_path_owes_the_suite(clone: Clone, path: str) -> None:
    """Each path of the list, by a real diff in a scratch clone."""
    head = clone.commit({path: "changed\n"})
    answer = classify.classify(clone.root, clone.base, head)
    truth(answer.owed, describing=f"{path} to owe it: {answer.reason}")


def test_a_change_only_elsewhere_owes_nothing(clone: Clone) -> None:
    """And says why, as the job's log does."""
    head = clone.commit({"README.md": "changed\n", "crates/printobserver-octoprint/a.rs": "x\n"})
    answer = classify.classify(clone.root, clone.base, head)
    truth(not answer.owed, describing=f"it to owe nothing: {answer.reason}")
    contains(answer.outputs(), "owed=false\nreason=")


def test_the_command_says_why_in_the_runs_summary(
    clone: Clone,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A change that owes nothing passes, and the run's summary says why."""
    head = clone.commit({"README.md": "changed\n"})
    summary = tmp_path / "summary.md"
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(summary))
    monkeypatch.chdir(clone.root)
    equal(classify.main([clone.base, head]), 0)
    contains(capsys.readouterr().out, "owed=false")
    contains(summary.read_text(encoding="utf-8"), "not owed")


def test_a_lock_change_owes_it_only_when_oneharness_moves(clone: Clone) -> None:
    """Another package's release moves nothing the agent sees."""
    other = clone.commit({classify.LOCK: LOCK_TEMPLATE.format(oneharness="1.0.0", serde="2.0.0")})
    truth(not classify.classify(clone.root, clone.base, other).owed, describing="serde alone")
    moved = clone.commit({classify.LOCK: LOCK_TEMPLATE.format(oneharness="1.1.0", serde="2.0.0")})
    truth(classify.classify(clone.root, other, moved).owed, describing="oneharness moving")


def test_a_template_owes_it_only_where_its_supervisor_settings_move(clone: Clone) -> None:
    """The ingress table of the same file is nothing the agent's turn reads."""
    template = "scripts/install.sh"
    ingress = clone.commit(
        {template: '[supervisor]\nharness = "claude-code"\n\n[ingress]\nshared_secret = "a"\n'}
    )
    truth(not classify.classify(clone.root, clone.base, ingress).owed, describing="ingress alone")
    model = clone.commit(
        {
            template: (
                '[supervisor]\nharness = "claude-code"\nmodel = "m"\n\n'
                '[ingress]\nshared_secret = "a"\n'
            )
        }
    )
    truth(classify.classify(clone.root, ingress, model).owed, describing="a pinned model")


def test_the_claude_code_release_owes_it(clone: Clone) -> None:
    """The release the job installs is on the list by value, not by the policy file's path."""
    policy = (clone.root / classify.POLICY).read_text(encoding="utf-8")
    unrelated = clone.commit({classify.POLICY: policy + "\n# a comment\n"})
    truth(not classify.classify(clone.root, clone.base, unrelated).owed, describing="a comment")
    moved_policy = policy.replace('claude_code = "', 'claude_code = "9.')
    moved = clone.commit({classify.POLICY: moved_policy})
    truth(classify.classify(clone.root, unrelated, moved).owed, describing="the release moving")


def test_a_diff_it_cannot_read_owes_the_suite(clone: Clone) -> None:
    """Unknown is never omission."""
    truth(classify.classify(clone.root, "no-such-ref", "HEAD").owed, describing="an unknown base")
