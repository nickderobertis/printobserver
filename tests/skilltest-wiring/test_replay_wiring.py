"""The real-print replay's machinery, with oneharness's published mock harness as the model.

Every test here that replays runs the real `printobserver server` over a real
case, with the published responder answering at the harness's process boundary
and its scripted commands running against that server through the real program.
Nothing is doubled above that but what `doubles` doubles for the live suite, and
no case's LFS bytes are read: a frame is this directory's own plain fixture,
stamped with the name of the frame it stands for.
"""

from __future__ import annotations

import dataclasses
import json
import os
import shutil
import sys
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from http.client import HTTPConnection
from pathlib import Path
from typing import Any, get_args, get_type_hints
from urllib.parse import quote

import classify
import doubles
import mock_turn
import pytest
import record
import replay as replay_module
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
    instant,
    read_case,
    replayed_case_names,
)
from mock_turn import ScriptedRun, Seen
from replay import (
    Assessment,
    Confidence,
    Replay,
    Replayed,
    ReplayError,
    RunReport,
    delivered,
    harness_token,
    is_event,
    is_held_print,
    kept_harness,
    neutral_environment,
    program,
    refuse_a_pointer,
    service_config,
)
from replay import Secrets as Minted
from repo_checks.__main__ import main as repo_check
from repo_checks.checks_real_prints import POSITIONED_TRIGGERS, printed_gcode, stored_size
from repo_checks.checks_real_prints import Trigger as RepoCheckTrigger
from repo_checks.expect import contains, equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.shell import run
from turns_per_run import UnreadableCaseError, turns_per_test

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
    "petg-spool-jam-air-print": ["first-layers-healthy"],
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
    "under-extrusion-lace": ["carry-on-healthy-before-fault"],
    "fan-cut-bridge": ["carry-on-healthy-before-fault"],
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
        return self.images[Path(json.loads(ran[look]["out"])["image_path"]).name]


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
            # By name: a temporary directory's path can be spelled two ways on Windows.
            path.name: frame_name(path.read_bytes())
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


def test_the_suites_vocabularies_are_the_contracts() -> None:
    """Its trigger and machine-state words are the ones the schemas declare."""
    schema = json.loads((CASES / "case.schema.json").read_text(encoding="utf-8"))
    declared = schema["$defs"]["Scenario"]["properties"]["trigger"]["enum"]
    equal(sorted(trigger.value for trigger in Trigger), sorted(declared))
    equal(sorted(get_args(RepoCheckTrigger)), sorted(declared))
    assessment = json.loads(
        (REPO / "schemas/printobserver-supervisor-api/AgentAssessment.json").read_text("utf-8")
    )
    equal(sorted(Assessment.__annotations__), sorted(assessment["properties"]))
    equal(sorted(mock_turn.Assessment.__annotations__), sorted(assessment["properties"]))
    confidence = sorted(arm["const"] for arm in assessment["$defs"]["Confidence"]["oneOf"])
    equal(sorted(get_args(Confidence)), confidence)
    equal(sorted(get_args(mock_turn.Confidence)), confidence)
    primitive = {"string": str, "boolean": bool}
    declared_types = {
        name: primitive[field["type"]]
        for name, field in assessment["properties"].items()
        if "type" in field
    }
    for model in (Assessment, mock_turn.Assessment):
        hints = get_type_hints(model)
        equal({name: hints[name] for name in declared_types}, declared_types, describing=str(model))
    truth(set(declared) >= POSITIONED_TRIGGERS, describing="positioned triggers to be triggers")
    states = json.loads((REPO / "schemas/printobserver-types/PrinterState.json").read_text("utf-8"))
    named = {arm["const"] for arm in states["oneOf"] if "const" in arm}
    truth({state.value for state in MachineState} <= named, describing=f"states among {named}")


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


TWO_EVENTS = "jar-top-wrong-material-first-layer-detach"
LAST = "ring-dragged-off-its-place"


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
        (_past_the_end, "past the end of 3423-TOP-REV F.gcode"),
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
    root = _case_tree(tmp_path, TWO_EVENTS, edit)
    equal(repo_check(["real-prints-replay", "--root", str(root)]), 1)
    printed = capsys.readouterr()
    contains(printed.out + printed.err, named)


def test_a_position_in_a_case_carrying_no_gcode_is_refused(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A case with no G-code has nothing a position could be inside."""

    def _positioned(document: dict[str, Any]) -> None:
        scenario = document["assertions"]["scenarios"][0]
        scenario["file_position_bytes"] = 1
        scenario["file_position_source"] = "progress"

    root = _case_tree(tmp_path, "healthy-printing", _positioned)
    equal(repo_check(["real-prints-replay", "--root", str(root)]), 1)
    printed = capsys.readouterr()
    contains(printed.out + printed.err, "carries no G-code")


def test_a_plain_gcode_is_measured_as_a_pointer_is(tmp_path: Path) -> None:
    """Fetched or not, the file's size is the object's."""
    plain = tmp_path / "plain.gcode"
    plain.write_bytes(b"G1 X1\n" * 400)
    equal(stored_size(plain), 2400)
    pointed = tmp_path / "pointed.gcode"
    oid = b"a" * 64
    pointed.write_bytes(
        b"version https://git-lfs.github.com/spec/v1\noid sha256:" + oid + b"\nsize 99\n"
    )
    equal(stored_size(pointed), 99)


def test_a_file_named_with_spaces_counts_as_named(tmp_path: Path) -> None:
    """A file a `case.json` names is held, spaces in its name or not; another is refused."""
    root = _case_tree(tmp_path, "under-extrusion-lace", lambda document: None)
    case_directory = root / "tests" / "real-prints" / "under-extrusion-lace"
    path = case_directory / "case.json"
    document = json.loads(path.read_text(encoding="utf-8"))
    document["sources"]["model"] = "a part F.stl"
    path.write_text(json.dumps(document), encoding="utf-8")
    (case_directory / "a part F.stl").write_text("solid\n", encoding="utf-8")
    equal(repo_check(["real-prints-files", "--root", str(root)]), 0)
    (case_directory / "unnamed.stl").write_text("solid\n", encoding="utf-8")
    equal(repo_check(["real-prints-files", "--root", str(root)]), 1)


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
        equal(done.images[Path(answered["image_path"]).name], moment.event_image)


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
    """With no frame left before the next event a look repeats the newest."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(case, [{"commands": [LOOK, LOOK]}], scratch)
    first, second = case.moments
    between = case.frames[case.frame_at_or_before(first.at, first.event_image) + 1].file
    equal([done.seen_frame(0, 0), done.seen_frame(0, 1)], [between, between])
    jobs = [json.loads(entry["out"])["job"] for entry in done.turns[0]["ran"]]
    truth(
        jobs[-1]["file_position_bytes"] < second.file_position_bytes,
        describing="nothing a turn reads to pass the next event's moment",
    )
    # The event's frame, the two looks, and then the next event's own frame.
    equal(done.replayed.fetches[:4], [first.event_image, between, between, second.event_image])


def test_a_fault_is_never_answered_to_a_look_before_its_event_fires(scratch: Path) -> None:
    """Over the real cases whose next event shows a fault, however many looks a turn takes."""
    for name in (
        "jar-top-wrong-material-first-layer-detach",
        "jar-top-wrong-material-detach-spaghetti",
    ):
        case = read_case(name)
        faulty = case.moments[-1].event_image
        directory = scratch / name
        directory.mkdir()
        done = mock_replay(case, [{"commands": [LOOK, LOOK, LOOK]}], directory)
        seen = [done.seen_frame(0, look) for look in range(3)]
        truth(faulty not in seen, describing=f"{faulty} not to be among {seen}")


def test_every_counter_a_turn_reads_comes_from_one_clock(scratch: Path) -> None:
    """The record's times are the recording's, and a repeated frame says it is one."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    first, second = case.moments
    waiting = ["printobserver", "look", "--print-id", "{print}", "--wait-s", "1", "--json"]
    done = mock_replay(case, [{"commands": [waiting, waiting, waiting]}], scratch)
    looks = [json.loads(entry["out"]) for entry in done.turns[0]["ran"]]
    # The one frame between the event's and the fault's, repeated with its own time.
    captured = {look["event"]["payload"]["frame_captured_at"] for look in looks}
    equal(len(captured), 1, describing=f"one capture time among {captured}")
    between = case.frames[case.frame_at_or_before(first.at, first.event_image) + 1]
    received = looks[0]["event"]["received_at"][:16]
    equal(received, instant(str(between.captured)).strftime("%Y-%m-%dT%H:%M"))
    # A real wait moves the job on, and nothing passes the next event's moment.
    positions = [look["job"]["file_position_bytes"] for look in looks]
    truth(positions[-1] > first.file_position_bytes, describing=f"the job to move: {positions}")
    truth(positions[-1] < second.file_position_bytes, describing="it to stop short of the next")


def test_a_turn_reads_one_job_start_across_the_events(scratch: Path) -> None:
    """What the server records of the job's start stays put across a whole print."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    started = {
        json.loads(turn["ran"][0]["out"])["context"]["print"]["job_started_at"][:16]
        for turn in done.turns
    }
    equal(len(started), 1, describing=f"one job start among {started}")


def test_the_last_events_looks_stop_at_what_its_recorded_turn_saw(scratch: Path) -> None:
    """A look after the last event does not jump to the print's end."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    last = case.moments[-1]
    done = mock_replay(case, [{"commands": [LOOK, LOOK, LOOK]}], scratch)
    equal(done.replayed.fetches[-3:], [last.looks[-1]] * 3)


def test_the_job_keeps_moving_between_checks_while_frames_repeat_a_reading() -> None:
    """A look's frame repeats the last check's progress, and the job does not stall for it."""
    case = read_case("bubbler-guard-and-pad-pla-healthy")
    first, second = case.moments[2], case.moments[3]
    span = int((instant(second.at) - instant(first.at)).total_seconds())
    moved = second.file_position_bytes - first.file_position_bytes
    for seconds in range(0, span, 30):
        when = instant(first.at) + timedelta(seconds=seconds)
        steady = first.file_position_bytes + moved * seconds / span
        off = abs(case.position_at(when) - steady)
        truth(off < moved / 20, describing=f"{steady} at {seconds} s, off by {off}")


def test_a_single_event_replay_moves_the_job_at_the_recordings_rate() -> None:
    """A case replaying one event still has its other scenarios to read the rate from."""
    for name in ("under-extrusion-lace", "fan-cut-bridge", "petg-spool-jam-air-print"):
        case = read_case(name)
        event = case.moments[0]
        at = instant(event.at)
        after_a_look = case.position_at(at + timedelta(seconds=90))
        truth(
            event.file_position_bytes <= after_a_look < case.gcode_size / 2,
            describing=f"{name} a little past {event.file_position_bytes}, at {after_a_look}",
        )


@pytest.mark.parametrize(
    "name", ["under-extrusion-lace", "fan-cut-bridge", "petg-spool-jam-air-print"]
)
def test_a_look_after_the_last_replayed_event_stops_short_of_the_recordings_next_moment(
    name: str, scratch: Path
) -> None:
    """More recording follows the list's end: the job moves on, and never jumps to it."""
    case = read_case(name)
    event = case.moments[-1]
    later = case.next_recorded_after(event.at)
    if later is None:
        pytest.fail(f"{name} has no recording after its last event")
    waiting = ["printobserver", "look", "--print-id", "{print}", "--wait-s", "1", "--json"]
    done = mock_replay(case, [{"commands": [waiting, waiting, waiting]}], scratch)
    answered = [json.loads(entry["out"]) for entry in done.turns[-1]["ran"]]
    positions = [look["job"]["file_position_bytes"] for look in answered]
    truth(
        event.file_position_bytes <= positions[0] <= positions[-1] < later.position,
        describing=f"{positions} from {event.file_position_bytes} and short of {later.position}",
    )
    received = [instant_of_utc(look["event"]["received_at"]) for look in answered]
    truth(received[-1] < later.at, describing=f"{received[-1]} before {later.at}")


@pytest.mark.parametrize(
    "name", ["under-extrusion-lace", "fan-cut-bridge", "petg-spool-jam-air-print"]
)
def test_however_long_a_last_turn_runs_nothing_passes_the_next_recorded_moment(
    name: str, scratch: Path
) -> None:
    """Hours of waiting at the list's end still read as just short of what comes next."""
    case = read_case(name)
    later = case.next_recorded_after(case.moments[-1].at)
    if later is None:
        pytest.fail(f"{name} has no recording after its last event")
    timeline = doubles.Timeline(case=case, picture=stamped, clock=scratch / "clock")
    timeline.next_fetch()
    timeline.next_fetch()
    timeline.offset += timedelta(hours=10)
    truth(timeline.position() < later.position, describing=f"a position short of {later}")
    truth(timeline.now() < later.at, describing=f"a clock short of {later}")


def test_the_print_clock_never_goes_back_whatever_a_look_waited(scratch: Path) -> None:
    """A clock going back reads to the server as a later job: one print, one session."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(case, [{"commands": [LOOK]}], scratch)
    equal(len(done.replayed.ledger["sessions"]), 1)


def test_a_recorded_alert_reaches_the_agent_at_its_time(scratch: Path) -> None:
    """Posted to the real webhook, it is the next turn's event, with its own picture and state."""
    case = read_case("petg-clip-detach-and-stringing-web")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    prompts = [turn["prompt"] for turn in done.turns]
    contains(prompts[0], '"kind": "periodic_observation"')
    for prompt in prompts[1:]:
        contains(prompt, '"kind": "obico_failure_alert"')
    contains(prompts[2], '"detector_paused_the_print": true')
    # The alert's picture says when it was taken, and the check's its own frame's.
    alert_taken = instant(case.moments[1].at).strftime("%Y-%m-%dT%H:%M")
    contains(prompts[1], f'"frame_captured_at": "{alert_taken}')
    contains(prompts[0], '"frame_captured_at"')
    equal(
        [point.event_kind for point in done.replayed.points],
        [
            "periodic_observation",
            "obico_failure_alert",
            "obico_failure_alert",
            "obico_failure_alert",
        ],
    )
    pictures = [
        done.images[Path(json.loads(turn["ran"][0]["out"])["image_path"]).name]
        for turn in done.turns
    ]
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


def test_every_turn_runs_with_the_harness_kept_from_replacing_itself(scratch: Path) -> None:
    """The shipped launch sets it: the server this replay starts is not handed it."""
    case = read_case("under-extrusion-lace")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    for turn in done.turns:
        equal(turn["environment"].get("DISABLE_AUTOUPDATER"), "1")
    neutral = neutral_environment(scratch, scratch)
    truth("DISABLE_AUTOUPDATER" not in neutral, describing="the replay to add no such thing")


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


def instant_of_utc(held: str) -> datetime:
    """A time the server wrote, to the second."""
    return datetime.fromisoformat(held[:19]).replace(tzinfo=UTC)


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
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    plain = run(command, cwd=REPO, env=environment)
    truth("test_replay" not in plain.stdout, describing=f"no live test in {plain.stdout}")
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
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


def test_the_obico_double_serves_only_the_images_its_alerts_name(scratch: Path) -> None:
    """A decoded separator names no alert image, so it reads nothing outside the case."""
    case = read_case("petg-clip-detach-and-stringing-web")
    timeline = doubles.Timeline(case=case, picture=stamped, clock=scratch / "clock")
    server = doubles.obico(timeline)
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    connection = HTTPConnection("127.0.0.1", server.server_address[1], timeout=10)
    try:
        connection.request("GET", f"/media/{quote(case.moments[0].event_image)}")
        equal(frame_name(connection.getresponse().read()), case.moments[0].event_image)
        connection.request("GET", "/media/..%2Fcase.json")
        answered = connection.getresponse()
        answered.read()
        equal(answered.status, 404)
    finally:
        connection.close()
        server.shutdown()
        server.server_close()


def _printer_record(timeline: doubles.Timeline) -> tuple[int, dict[str, Any]]:
    """What the Obico double answers its printer read with, over a real socket."""
    server = doubles.obico(timeline)
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    connection = HTTPConnection("127.0.0.1", server.server_address[1], timeout=10)
    try:
        connection.request("GET", f"/api/v1/printers/{doubles.OBICO_PRINTER}/")
        answered = connection.getresponse()
        return answered.status, json.loads(answered.read())
    finally:
        connection.close()
        server.shutdown()
        server.server_close()


def test_the_obico_doubles_printer_record_is_the_cases_recorded_detector(scratch: Path) -> None:
    """A case recording the detector names its print and a snapshot that moves; none, no print.

    The print is named by the identifier the case's first recorded alert names, its
    file and its start, which is what the server matches it to its own print by.
    """
    case = read_case("petg-clip-detach-and-stringing-web")
    truth(case.detector_recorded, describing="the case records what the detector did")
    timeline = doubles.Timeline(case=case, picture=stamped, clock=scratch / "clock")
    status, first = _printer_record(timeline)
    equal(status, 200)
    named = case.obico_print_id()
    truth(named is not None, describing="the case's recorded alert names Obico's print")
    equal(first["current_print"]["id"], named)
    equal(first["current_print"]["filename"], case.gcode.name)
    equal(
        datetime.fromisoformat(first["current_print"]["started_at"]),
        case.job_started(),
    )
    truth("auth_token" not in first, describing="the double carries no printer credential")
    timeline.offset += timedelta(seconds=5)
    _, later = _printer_record(timeline)
    truth(
        later["pic"] != first["pic"], describing="a watching Obico's snapshot moved between reads"
    )

    unrecorded = dataclasses.replace(case, detector_recorded=False)
    quiet = doubles.Timeline(case=unrecorded, picture=stamped, clock=scratch / "quiet-clock")
    _, record_of_none = _printer_record(quiet)
    equal(record_of_none["current_print"], None)

    minted = Minted()
    config = tomllib.loads(
        service_config(
            scratch / "state",
            "127.0.0.1:1",
            "http://127.0.0.1:2",
            "http://127.0.0.1:3",
            "http://127.0.0.1:4",
            minted,
        )
    )
    equal(config["obico"]["printer_id"], doubles.OBICO_PRINTER)


@pytest.mark.parametrize(
    "event",
    [
        "no event",
        {"kind": "k", "received_at": "2026-01-01T00:00:00Z"},
        {"id": "e", "kind": "k", "received_at": "yesterday", "payload": {}},
        {
            "id": "e",
            "kind": "k",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {},
            "image": {"sha256": 3},
        },
        {
            "id": "e",
            "kind": "action_requested",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {"action": {"action": "pause", "actor": {"agent": {}}}},
        },
        {"id": "e", "kind": "k", "received_at": "2026-01-01T00:00:00", "payload": {}},
        {"id": "e", "kind": "k", "received_at": "2026-01-01T00:00:00.123456garbage", "payload": {}},
        {"id": "e", "kind": "k", "received_at": "2026-01-01T00:00:00Z"},
        {"id": "e", "kind": "k", "received_at": "2026-01-01T00:00:00Z", "payload": []},
        {"id": "e", "kind": "port_failure", "received_at": "2026-01-01T00:00:00Z", "payload": {}},
        {
            "id": "e",
            "kind": "port_failure",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {"site": "supervision_turn", "detail": "failed"},
        },
        {
            "id": "e",
            "kind": "action_requested",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {"action": {"action": "pause", "actor": ["agent"]}},
        },
        {
            "id": "e",
            "kind": "action_requested",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {"action": 1},
        },
        {
            "id": "e",
            "kind": "action_requested",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {"action": {"action": "pause"}},
        },
        {
            "id": "e",
            "kind": "action_requested",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {
                "action": {
                    "action": "shake_the_printer",
                    "actor": {"agent": {"session_name": "print-a"}},
                    "reason": "no such action",
                }
            },
        },
        {
            "id": "e",
            "kind": "action_requested",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {
                "action": {
                    "action": "set_fan_percent",
                    "actor": {"agent": {"session_name": "print-a"}},
                    "reason": "an argument of the wrong type",
                    "percent": "full",
                }
            },
        },
        {
            "id": "e",
            "kind": "agent_assessment",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {"assessment": {"summary": "fine"}},
        },
        {
            "id": "e",
            "kind": "agent_assessment",
            "received_at": "2026-01-01T00:00:00Z",
            "payload": {},
        },
    ],
)
def test_a_history_event_the_replay_could_not_read_is_refused(event: object) -> None:
    """Each shape the judging reads is held at the boundary rather than where it is used."""
    equal(is_event(event), False)


def test_the_harness_install_is_the_pinned_release_and_says_how_it_went(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The command names the policy's release, and its failure is the program's."""
    policy = tmp_path / "repo-policy.toml"
    policy.write_text('[skilltest]\nclaude_code = "9.9.9"\n', encoding="utf-8")
    equal(
        classify.harness_install(policy),
        ["npm", "install", "-g", "@anthropic-ai/claude-code@9.9.9"],
    )
    fake = tmp_path / "bin"
    fake.mkdir()
    suffix = ".exe" if sys.platform == "win32" else ""
    shutil.copy2(program("printobserver-server-responder"), fake / f"npm{suffix}")
    monkeypatch.setenv("PATH", f"{fake}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setenv("MOCK_EXIT", "7")
    equal(classify.main(["--install-harness"]), 7)


def test_a_supervisor_table_ending_its_template_is_still_read() -> None:
    """A change to the last table of an install template is a change to the settings."""
    equal(classify.supervisor_table("[supervisor]\nharness = 'a'\n"), "harness = 'a'\n")
    equal(classify.supervisor_table("[supervisor]\nharness = 'a'\n[next]\n"), "harness = 'a'\n")
    equal(classify.supervisor_table("[other]\n"), None)


def test_a_false_image_is_refused_rather_than_read_as_none() -> None:
    """Only an absent or null image means an event carries none."""
    event = {"id": "e", "kind": "k", "received_at": "2026-01-01T00:00:00Z", "payload": {}}
    equal(is_event({**event, "image": None}), True)
    equal(is_event({**event, "image": {"sha256": "ab"}}), True)
    equal(is_event({**event, "image": False}), False)


def test_the_kept_harness_is_a_copy_that_starts_and_outlives_its_install(
    scratch: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Found on PATH, copied once, runnable, and still there when the install goes."""
    install = scratch / "install"
    install.mkdir()
    built = program("printobserver")
    source = install / f"claude{built.suffix}"
    shutil.copy2(built, source)
    root = scratch / "tree"
    monkeypatch.setenv("PATH", str(scratch / "nothing-here"))
    equal(kept_harness(root), None)
    monkeypatch.setenv("PATH", str(install))
    kept = kept_harness(root)
    truth(kept is not None, describing="a kept copy of the harness on PATH")
    equal(Path(str(kept)).parent, root / "target" / "skilltest" / "harness")
    source.unlink()
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    equal(run([str(kept), "--version"]).returncode, 0)
    shutil.copy2(built, source)
    equal(kept_harness(root), kept)
    equal(len(list(Path(str(kept)).parent.iterdir())), 1)
    # The same size in other bytes is another release, and gets a copy of its own.
    changed = bytearray(built.read_bytes())
    changed[-1] ^= 0xFF
    source.write_bytes(changed)
    other = kept_harness(root)
    truth(other != kept, describing="a changed harness to be copied afresh")
    equal(Path(str(other)).read_bytes(), bytes(changed))


def test_the_budgets_command_reports_the_turns_the_cases_count(scratch: Path) -> None:
    """Its entry point writes the total and each test's turns where the budget reads them."""
    result = scratch / "result.json"
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run(
        [sys.executable, str(REPO / "tests" / "skilltest" / "turns_per_run.py")],
        env={**os.environ, "ONEBUDGETSPEC_RESULT": str(result)},
    )
    equal(done.returncode, 0, describing=done.stderr)
    counted = turns_per_test()
    reported = json.loads(result.read_text(encoding="utf-8"))
    equal(reported["value"], sum(counted.values()))
    equal(json.loads(reported["detail"]), {"turns_per_test": counted})
    contains(done.stderr, f"turns per full run: {sum(counted.values())}")


def test_a_cases_directory_with_no_case_in_it_is_refused(scratch: Path) -> None:
    """An absent or an empty directory is no suite of zero turns."""
    script = str(REPO / "tests" / "skilltest" / "turns_per_run.py")
    result = scratch / "result.json"
    for cases in (scratch / "absent", scratch):
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        done = run(
            [sys.executable, script, str(cases)],
            env={**os.environ, "ONEBUDGETSPEC_RESULT": str(result)},
        )
        equal(done.returncode, 1)
        contains(done.stderr, "holds no case")
        truth(not result.exists(), describing="no figure reported for no suite")


def test_a_history_command_that_fails_stops_the_replay(scratch: Path) -> None:
    """An unanswered read is the replay gone wrong, never a print with no events."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    replay = Replay(case, [sys.executable, str(MOCK_TURN), str(scratch / "script.json")])
    try:
        replay._install()
        replay.print_id = replay_module.PrintId("a-print")
        with pytest.raises(ReplayError, match="`printobserver history` answered"):
            replay._events()
    finally:
        replay.stop()
        replay.clean_up()


@pytest.mark.parametrize(
    "document",
    [["no case"], {"assertions": {"replay": "a-string"}}, {"assertions": {"replay": [1]}}],
)
def test_a_case_whose_replay_is_no_list_of_ids_is_refused(document: object, scratch: Path) -> None:
    """A string's length is no count of turns, so the budget stops rather than report it."""
    cases = scratch / "cases"
    case = cases / "a-case"
    case.mkdir(parents=True)
    (case / "case.json").write_text(json.dumps(document), encoding="utf-8")
    with pytest.raises(UnreadableCaseError):
        turns_per_test(cases)
    result = scratch / "result.json"
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run(
        [sys.executable, str(REPO / "tests" / "skilltest" / "turns_per_run.py"), str(cases)],
        env={**os.environ, "ONEBUDGETSPEC_RESULT": str(result)},
    )
    equal(done.returncode, 1)
    contains(done.stderr, "a-case")
    truth(not result.exists(), describing="no figure reported for an unreadable case")


def test_a_print_with_an_unreadable_job_start_is_refused() -> None:
    """A job start is read as a time, so anything else stops there."""
    equal(is_held_print({"id": "p", "job_started_at": "2026-01-01T00:00:00.123456Z"}), True)
    equal(is_held_print({"id": "p", "job_started_at": None}), True)
    equal(is_held_print({"id": "p", "job_started_at": "soon"}), False)
    equal(is_held_print({"job_started_at": None}), False)


def test_a_replay_spending_past_its_allowance_stops(
    scratch: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Past the runs it allows, it stops rather than go on spending the operator's quota."""
    monkeypatch.setattr(replay_module, "RUN_ALLOWANCE_PER_ENTRY", 0)
    with pytest.raises(ReplayError, match="stopping the spend"):
        mock_replay(
            read_case("jar-top-wrong-material-first-layer-detach"),
            [{"commands": [CONTEXT]}],
            scratch,
        )


def test_the_doubles_refuse_what_no_adapter_sends(scratch: Path) -> None:
    """A malformed body, an unknown printer command and another Obico route are refused."""
    timeline = doubles.Timeline(
        case=read_case("jar-top-wrong-material-first-layer-detach"),
        picture=stamped,
        clock=scratch / "clock",
    )
    octoprint = doubles.octoprint(timeline, "key")
    obico = doubles.obico(timeline)
    try:
        for server, path, body, status in (
            (octoprint, "/api/job", "not json", 400),
            (octoprint, "/api/job", "[]", 400),
            (octoprint, "/api/printer/tool", '{"command": "x"}', 400),
            (octoprint, "/api/printer/tool", '{"command": "flowrate", "factor": "all"}', 400),
            (octoprint, "/api/printer/command", '{"commands": [3]}', 400),
            (octoprint, "/api/printer/printhead", '{"command": "feedrate", "factor": 100}', 204),
            (octoprint, "/api/printer/bed", '{"command": "target", "target": 60}', 204),
            (obico, "/api/other", "{}", 404),
            (obico, "/api/v1/printers/3/acknowledge_alert/", "{}", 200),
        ):
            # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
            connection = HTTPConnection("127.0.0.1", server.server_address[1], timeout=10)
            connection.request("POST", path, body, {"X-Api-Key": "key"})
            answered = connection.getresponse()
            answered.read()
            connection.close()
            equal(answered.status, status, describing=f"{path} {body}")
        # Declared and never sent: the double refuses without reading, and closes.
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        connection = HTTPConnection("127.0.0.1", octoprint.server_address[1], timeout=10)
        declared = {"X-Api-Key": "key", "Content-Length": str(doubles.MAX_BODY_BYTES + 1)}
        connection.request("POST", "/api/job", headers=declared)
        answered = connection.getresponse()
        answered.read()
        connection.close()
        equal(answered.status, 413, describing="a body past the limit")
    finally:
        for server in (octoprint, obico):
            server.shutdown()
            server.server_close()


def test_usage_a_later_run_adds_is_summed_or_kept() -> None:
    """Counts add up across a turn's runs; anything else keeps its first value."""
    report = RunReport()
    report.take(
        [
            {"type": "result", "usage": {"input_tokens": 1, "service_tier": "standard"}},
            {
                "type": "result",
                "usage": {"input_tokens": 2, "service_tier": "batch", "output_tokens": 5},
            },
        ]
    )
    equal(report.usage, {"input_tokens": 3, "service_tier": "standard", "output_tokens": 5})


def test_a_harness_that_cannot_start_stops_the_replay_naming_why(scratch: Path) -> None:
    """The launcher's own reason is what the replay reports, and everything stops."""
    case = read_case("fan-cut-bridge")
    replay = Replay(case, [str(scratch / "no-such-harness")], picture=stamped)
    try:
        with pytest.raises(ReplayError, match="the launcher said"):
            replay.run()
        truth(
            replay.server is not None and replay.server.poll() is not None,
            describing="the server to have stopped",
        )
    finally:
        replay.clean_up()


def test_a_turn_past_its_deadline_stops_the_replay_and_its_spending(
    scratch: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A hung turn is reported by its count, and the server is stopped."""
    monkeypatch.setattr(replay_module, "TURN_DEADLINE_S", 1)
    with pytest.raises(ReplayError, match="turns ended in"):
        mock_replay(read_case("fan-cut-bridge"), [{"commands": [], "stall_s": 4.0}], scratch)


def test_stopping_a_replay_stops_its_server_and_its_doubles(scratch: Path) -> None:
    """What a replay started does not outlive it."""
    script = scratch / "script.json"
    script.write_text(
        json.dumps(
            {
                "session": "stopping",
                "responder": str(program("printobserver-server-responder")),
                "turns": [{"commands": [CONTEXT]}],
            }
        ),
        encoding="utf-8",
    )
    replay = Replay(
        read_case("fan-cut-bridge"),
        [sys.executable, str(MOCK_TURN), str(script)],
        picture=stamped,
    )
    try:
        replay.run()
        truth(
            replay.server is not None and replay.server.poll() is not None,
            describing="the server to have stopped",
        )
        for double in replay.servers:
            equal(double.socket.fileno(), -1, describing="a double's socket to be closed")
    finally:
        replay.clean_up()


def test_the_event_kinds_the_judging_reads_are_the_servers() -> None:
    """A kind the server renamed would otherwise judge every turn as having asked nothing."""
    declared = "\n".join(
        (REPO / "crates" / crate / "src" / name).read_text(encoding="utf-8")
        for crate, name in (
            ("printobserver-core", "kinds.rs"),
            ("printobserver-obico", "events.rs"),
        )
    )
    for kind in (
        *replay_module.REQUESTED,
        *replay_module.TRIGGERS,
        replay_module.ASSESSED,
        replay_module.FAILED,
        replay_module.CAMERA_LOOK,
    ):
        contains(declared, f'const KIND: &\'static str = "{kind}";')


def test_a_harnesss_own_bookkeeping_is_no_model_a_turn_answered_with() -> None:
    """`<synthetic>` is written by no model, and a real one beside it is still read."""
    report = RunReport()
    report.take(
        [
            {"message": {"model": "<synthetic>", "content": []}},
            {"message": {"model": "a-real-model", "content": []}},
        ]
    )
    equal(report.models, ["a-real-model"])


def test_a_turn_says_how_many_looks_it_took(scratch: Path) -> None:
    """The record counts the server's own looks for that turn, and only those."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(case, [{"commands": [LOOK, LOOK]}, {"commands": [CONTEXT]}], scratch)
    equal([point.looks for point in done.replayed.points], [2, 0])


def test_a_turn_says_each_call_once_and_what_it_opened(scratch: Path) -> None:
    """A call the transcript writes twice ran once, and a file tool's read is reported."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    done = mock_replay(
        case,
        [{"commands": [CONTEXT], "reads": ["/somewhere/SKILL.md"], "streamed_twice": True}],
        scratch,
    )
    point = done.replayed.points[0]
    equal(len(point.commands), 1)
    contains(point.commands[0], "printobserver context")
    equal(point.opened, ["SKILL.md"])


def test_a_turn_the_engine_gave_up_on_fails_its_own_decision_point(scratch: Path) -> None:
    """Its failure is that entry's verdict, and the next event still gets its turn."""
    case = read_case("jar-top-wrong-material-first-layer-detach")
    unreadable: ScriptedRun = {"commands": [CONTEXT], "unreadable": True}
    done = mock_replay(case, [unreadable, unreadable, unreadable, {"commands": [CONTEXT]}], scratch)
    first, second = done.replayed.points
    truth(first.why_not is not None, describing="the failed turn to fail its decision point")
    contains(str(first.why_not), "the turn failed")
    truth(second.moment.id == case.moments[1].id, describing="the next event to get its turn")
    equal(first.assessment, None, describing="the failed turn, which answered nothing")
    truth(second.assessment is not None, describing="the next turn's assessment to be its own")
    equal(done.replayed.passed, False)


def test_only_the_ingress_accepting_an_alert_counts_as_delivering_it() -> None:
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
    equal(
        done.images[Path(json.loads(done.turns[0]["ran"][0]["out"])["image_path"]).name],
        first.event_image,
    )
    equal(
        done.seen_frame(1, 0),
        case.frames[case.frame_at_or_before(first.at, first.event_image) + 1].file,
    )
    equal(
        done.images[Path(json.loads(done.turns[2]["ran"][0]["out"])["image_path"]).name],
        second.event_image,
    )


def _a_finished_test() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    """A real case's decision points, and the record of a test that passed them."""
    point = {
        "scenario": "a-scenario",
        "at": "2026-09-27T18:40:05",
        "trigger": "periodic_observation",
        "passed": True,
        "why_not": None,
        "requests": [],
        "commands": [],
        "opened": [],
        "assessment": None,
        "looks": 0,
        "usage": None,
        "models": [],
        "compacted": False,
    }
    carry_on, bridge = REPLAYS["jar-top-wrong-material-first-layer-detach"]
    points = [{**point, "scenario": carry_on}, {**point, "scenario": bridge}]
    test_record = {
        "test": "jar-top-wrong-material-first-layer-detach",
        "passed": True,
        "turns": 2,
        "sessions": [],
        "decision_points": points,
    }
    return points, test_record


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
    points, test_record = _a_finished_test()
    (finished / "jar-top-wrong-material-first-layer-detach.json").write_text(
        json.dumps(test_record), encoding="utf-8"
    )
    # What a live run writes beside it, which is no test's record.
    (finished / "jar-top-wrong-material-first-layer-detach.transcript.jsonl").write_text(
        "{}\n", encoding="utf-8"
    )
    equal(record.main([str(finished)]), 0)
    equal(json.loads(written.read_text("utf-8"))["detail"]["commit"], "a-commit")
    failed = tmp_path / "failed"
    failed.mkdir()
    failing = {**points[1], "passed": False, "why_not": "it paused"}
    (failed / "jar-top-wrong-material-first-layer-detach.json").write_text(
        json.dumps({**test_record, "passed": False, "decision_points": [points[0], failing]}),
        encoding="utf-8",
    )
    equal(record.main([str(failed)]), 1)
    empty = tmp_path / "empty"
    empty.mkdir()
    equal(record.main([str(empty)]), 1)
    for name, unusable in (
        ("list", []),
        ("truthy", {**test_record, "passed": "false"}),
        ("boolean-turns", {**test_record, "turns": True, "decision_points": points[:1]}),
        (
            "negative-looks",
            {**test_record, "decision_points": [{**points[0], "looks": -1}, points[1]]},
        ),
        (
            "boolean-looks",
            {**test_record, "decision_points": [{**points[0], "looks": True}, points[1]]},
        ),
        (
            "passed-with-a-reason",
            {**test_record, "decision_points": [{**points[0], "why_not": "it paused"}, points[1]]},
        ),
        (
            "failed-with-no-reason",
            {
                **test_record,
                "passed": False,
                "decision_points": [{**points[0], "passed": False}, points[1]],
            },
        ),
        ("short", {**test_record, "turns": 0, "decision_points": []}),
        ("repeated", {**test_record, "decision_points": [points[0], points[0]]}),
        ("reordered", {**test_record, "decision_points": points[::-1]}),
        ("sessions", {**test_record, "sessions": [3]}),
        (
            "why",
            {**test_record, "decision_points": [{**points[0], "why_not": 3}, points[1]]},
        ),
        (
            "assessment",
            {**test_record, "decision_points": [{**points[0], "assessment": {}}, points[1]]},
        ),
        (
            "field",
            {**test_record, "decision_points": [{**points[0], "looks": "many"}, points[1]]},
        ),
    ):
        unreadable = tmp_path / name
        unreadable.mkdir()
        (unreadable / "a.json").write_text(json.dumps(unusable), encoding="utf-8")
        equal(record.main([str(unreadable)]), 1, describing=name)
    twice = tmp_path / "twice"
    twice.mkdir()
    for name in ("one.json", "two.json"):
        (twice / name).write_text(json.dumps(test_record), encoding="utf-8")
    equal(record.main([str(twice)]), 1)
    # A `just` of this test's own, standing where the real recipe would run the suite.
    fake = tmp_path / "bin"
    fake.mkdir()
    suffix = ".exe" if sys.platform == "win32" else ""
    shutil.copy2(program("printobserver-server-responder"), fake / f"just{suffix}")
    monkeypatch.setenv("PATH", f"{fake}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setenv("MOCK_EXIT", "7")
    equal(record.main([]), 7)
    equal(json.loads(written.read_text("utf-8"))["figures"], {"turns": 0})


def test_the_record_command_runs_the_suite_and_folds_what_that_run_wrote(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Its arguments reach the recipe, which writes where it is told, and that is folded."""
    written = tmp_path / "latest.json"
    monkeypatch.setattr(record, "RESULT", written)
    monkeypatch.setattr(record, "REPO", tmp_path)
    monkeypatch.setattr(record, "commit", lambda: "a-commit")
    _, test_record = _a_finished_test()
    produced = tmp_path / "produced.json"
    produced.write_text(json.dumps(test_record), encoding="utf-8")
    called = tmp_path / "called.json"
    stand_in = tmp_path / "stand_in.py"
    stand_in.write_text(
        "import json, os, shutil, sys\n"
        f"json.dump(sys.argv[1:], open({str(called)!r}, 'w'))\n"
        f"shutil.copy({str(produced)!r}, "
        "os.path.join(os.environ['SKILLTEST_RUN_DIR'], 'a-test.json'))\n",
        encoding="utf-8",
    )
    # `just` as each platform's PATH finds one: a script here, a batch file on Windows.
    fake = tmp_path / "bin"
    fake.mkdir()
    script = fake / "just"
    script.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{stand_in}" "$@"\n', encoding="utf-8")
    script.chmod(0o755)
    (fake / "just.bat").write_text(f'@"{sys.executable}" "{stand_in}" %*\n', encoding="utf-8")
    monkeypatch.setenv("PATH", f"{fake}{os.pathsep}{os.environ['PATH']}")

    equal(record.main(["-k", "a-test"]), 0)

    equal(
        json.loads(called.read_text("utf-8")),
        ["skilltest", "-k", "a-test"],
        describing="what the recipe was asked",
    )
    folded = json.loads(written.read_text("utf-8"))
    equal(folded["figures"], {"turns": 2}, describing="the turns the run wrote")
    equal(folded["detail"]["passed"], True, describing="the run's verdict")
    equal(
        [test["test"] for test in folded["detail"]["tests"]],
        ["jar-top-wrong-material-first-layer-detach"],
        describing="the tests the run wrote",
    )


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
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        run(["git", "add", "-A"], cwd=self.root, check=True)
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        run(["git", "commit", "-q", "-m", "chore: change"], cwd=self.root, check=True)
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
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
    for name, value in (("user.name", "wiring"), ("user.email", "wiring@invalid")):
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        run(["git", "config", "--local", name, value], cwd=root, check=True)
    shutil.copy2(REPO / classify.POLICY, root / classify.POLICY)
    (root / classify.LOCK).write_text(
        LOCK_TEMPLATE.format(oneharness="1.0.0", serde="1.0.0"), encoding="utf-8"
    )
    (root / "scripts").mkdir()
    (root / "scripts" / "install.sh").write_text(
        '[supervisor]\nharness = "claude-code"\n\n[ingress]\nshared_secret = ""\n',
        encoding="utf-8",
    )
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    run(["git", "add", "-A"], cwd=root, check=True)
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    run(["git", "commit", "-q", "-m", "chore: base"], cwd=root, check=True)
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
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


def test_a_reason_git_wrote_over_several_lines_stays_one_output(clone: Clone) -> None:
    """A base git cannot read owes the suite, in the two lines the workflow reads."""
    answer = classify.classify(clone.root, "no-such-revision", clone.base)
    truth(answer.owed, describing=f"an unreadable diff to owe it: {answer.reason}")
    multiline = classify.Classification(owed=True, reason="git said:\nfatal: first\nhint: second\n")
    equal(
        multiline.outputs(),
        "owed=true\nreason=git said: fatal: first hint: second\n",
        describing="what the workflow appends to its output file",
    )
    equal(len(answer.outputs().splitlines()), 2, describing=answer.outputs())


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
