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
import re
import shutil
import sys
import time
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass, replace
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
import shell_lines
from cases import (
    CASES,
    REPO,
    Case,
    MachineState,
    Milestone,
    Moment,
    Request,
    ScenarioId,
    Step,
    Trigger,
    instant,
    is_start_case,
    read_case,
    read_start_case,
    replayed_case_names,
)
from mock_turn import ScriptedRun, Seen
from outside import MILESTONE_WORDS, Outside
from replay import (
    BARE_PROBE_RECORD,
    IMAGE_PROBE_RECORD,
    PROBE_RECORD,
    Assessment,
    CallUsage,
    CheckClass,
    Confidence,
    DecisionPoint,
    DecisionRecord,
    Disposition,
    LedgerCompaction,
    LedgerTurn,
    Replay,
    Replayed,
    ReplayError,
    RunReport,
    alignment_of,
    calibration_copy,
    check_image,
    check_wait,
    composed_check_image,
    delivered,
    first_layer_end,
    harness_token,
    installer_compaction,
    is_event,
    is_held_print,
    kept_harness,
    neutral_environment,
    program,
    refuse_a_pointer,
    service_config,
)
from replay import Secrets as Minted
from replay import TestRecord as RecordedTest
from repo_checks.__main__ import main as repo_check
from repo_checks.checks_real_prints import POSITIONED_TRIGGERS, printed_gcode, stored_size
from repo_checks.checks_real_prints import Trigger as RepoCheckTrigger
from repo_checks.expect import contains, equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.shell import run
from turns_per_run import PROBE_TURNS, UnreadableCaseError, turns_per_test

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
        "outside-first-layer",
        "outside-first-support",
        "outside-first-overhang",
        "carry-on-013-other",
        "carry-on-024-support",
        "carry-on-028-bridge",
        "carry-on-031-other",
        "carry-on-042-other",
        "carry-on-049-support",
        "carry-on-059-other",
        "outside-end",
    ],
}
#: The watch lines each healthy case hands its outside supervisor, by case, in order.
WATCH_LINES = {
    "bubbler-guard-and-pad-pla-healthy": [
        "outside-first-layer",
        "outside-first-support",
        "outside-first-overhang",
        "outside-end",
    ],
    "jar-top-petg-healthy": [
        "outside-first-layer",
        "outside-first-support",
        "outside-first-overhang",
        "outside-end",
    ],
}
#: The start requests section 2 of the start replays fixes, by case: every request of
#: each bed case but those repeating another's bed state, in time order.
START_REPLAYS = {
    "bed-and-sheet-check": [
        "clear-smooth-pei-20261007-075606",
        "not-clear-smooth-pei-20261007-105905",
        "partial-textured-20261008-140612",
        "not-clear-textured-20261008-172933",
        "clear-textured-20261010-061436",
        "not-clear-textured-20261010-164500",
        "not-clear-textured-20261010-174900",
        "clear-textured-20261027-153224",
        "corrupted-textured-20261027-154933",
        "partial-textured-20261029-183517",
        "not-clear-textured-20261029-195324",
        "not-clear-textured-20261030-142205",
        "not-clear-textured-20261030-174128",
        "partial-smooth-pei-20261031-164529",
        "not-clear-smooth-pei-20261031-191910",
    ],
    "bed-clear-check": [
        "not-clear-02-failed-print-left-on-bed-1546",
        "clear-01-bed-lowered-1727",
        "clear-02-bed-raised-1921",
        "clear-04-heating-before-first-layer-2044",
        "not-clear-01-one-stray-strand-2218",
        "clear-05-head-parked-2219",
        "not-clear-03-parts-and-strands-0415",
    ],
}
CONTEXT = ["printobserver", "context", "--print-id", "{print}", "--json"]
#: The status read, which states the job whole where the agent's context does not.
STATUS = ["printobserver", "status", "--print-id", "{print}", "--json"]
LOOK = ["printobserver", "look", "--print-id", "{print}", "--json"]
#: How long a waiting look waits: long enough to see, short enough to wait out.
LOOK_WAIT_S = 5
LOOK_WAITING = [
    "printobserver",
    "look",
    "--print-id",
    "{print}",
    "--wait-s",
    str(LOOK_WAIT_S),
    "--json",
]
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
    #: Every read of the job the stood-in instance answered, in order.
    job_reads: list[doubles.JobRead] = dataclasses.field(default_factory=list)

    def seen_frame(self, turn: int, look: int) -> str:
        """The frame the `look`th scripted look of one turn answered."""
        ran = [entry for entry in self.turns[turn]["ran"] if entry["words"][1] == "look"]
        return self.images[Path(json.loads(ran[look]["out"])["image_path"]).name]


def mock_replay(
    case: Case,
    turns: list[ScriptedRun],
    scratch: Path,
    gcode: bytes | None = None,
    *,
    real_waits: bool = False,
    compaction: mock_turn.Compaction | None = None,
    picture: Callable[[str], bytes] = stamped,
) -> Scripted:
    """Run one case's whole replay, its turns scripted, and read back what happened.

    Each frame is `picture`'s bytes for its name, by default the plain fixture
    stamped with it.
    """
    script = scratch / "script.json"
    script.write_text(
        json.dumps(
            {
                "session": case.name,
                "responder": str(program("printobserver-server-responder")),
                "turns": turns,
                **({"compaction": compaction} if compaction is not None else {}),
            }
        ),
        encoding="utf-8",
    )
    target = [sys.executable, str(MOCK_TURN), str(script)]
    replay = Replay(
        case,
        target,
        picture=picture,
        gcode=gcode,
        real_waits=real_waits,
        outside=lambda replaying: Outside(replaying, target),
    )
    try:
        replayed = replay.run()
        images = {
            # By name: a temporary directory's path can be spelled two ways on Windows.
            path.name: frame_name(path.read_bytes())
            for path in replay.state.glob("images/*/*")
            if path.is_file()
        }
        job_reads = list(replay.timeline.job_reads)
    finally:
        replay.clean_up()
    seen = (scratch / "seen.jsonl").read_text(encoding="utf-8").splitlines()
    return Scripted(replayed, [json.loads(line) for line in seen], images, job_reads)


@pytest.fixture
def scratch(tmp_path: Path) -> Path:
    """Where a mock turn keeps what it saw."""
    return tmp_path


def turns_of(name: str) -> Case:
    """One case, its replay cut to printobserver's own turns: no watch line in it."""
    return read_case(
        name, replay=tuple(moment.id for moment in read_case(name).moments if not moment.is_outside)
    )


#: An outside supervisor's run that does what a watch line asks: a look of its own at
#: the printer, with no print named, and the frame it named opened.
PRINTER_LOOK = ["printobserver", "look", "--json"]
OUTSIDE_LOOKS: ScriptedRun = {"commands": [PRINTER_LOOK], "reads": ["{looked}"]}


def scripted_for(case: Case, turns: list[ScriptedRun]) -> list[ScriptedRun]:
    """A script for every harness run of a case, its own turns' first and its watch lines'.

    The runs are counted over the replay's entries in order; printobserver's turns take
    `turns` in turn, the last standing for every later one, and each watch line's run
    does what the line asks.
    """
    scripted: list[ScriptedRun] = []
    taken = 0
    for moment in case.moments:
        if moment.is_outside:
            scripted.append(OUTSIDE_LOOKS)
        else:
            scripted.append(turns[min(taken, len(turns) - 1)])
            taken += 1
    return scripted


def test_the_suites_vocabularies_are_the_contracts() -> None:
    """Its trigger, machine-state and assessment words are the ones the schemas declare."""
    schema = json.loads((CASES / "case.schema.json").read_text(encoding="utf-8"))
    declared = schema["$defs"]["Scenario"]["properties"]["trigger"]["enum"]
    equal(sorted(trigger.value for trigger in Trigger), sorted(declared))
    equal(sorted(get_args(RepoCheckTrigger)), sorted(declared))
    milestones = schema["$defs"]["Scenario"]["properties"]["milestone"]["enum"]
    equal(sorted(milestone.value for milestone in Milestone), sorted(milestones))
    equal(set(MILESTONE_WORDS), set(Milestone), describing="a watch line's words for each")
    assessment = json.loads(
        (REPO / "schemas/printobserver-supervisor-api/AgentAssessment.json").read_text("utf-8")
    )
    equal(sorted(Assessment.__annotations__), sorted(assessment["properties"]))
    equal(sorted(mock_turn.Assessment.__annotations__), sorted(assessment["properties"]))
    confidence = sorted(arm["const"] for arm in assessment["$defs"]["Confidence"]["oneOf"])
    equal(sorted(get_args(Confidence)), confidence)
    equal(sorted(get_args(mock_turn.Confidence)), confidence)
    disposition = sorted(arm["const"] for arm in assessment["$defs"]["Disposition"]["oneOf"])
    equal(sorted(get_args(Disposition)), disposition)
    equal(sorted(get_args(mock_turn.Disposition)), disposition)
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
    equal(len(bubbler.moments), 36)
    for name, lines in WATCH_LINES.items():
        equal(
            [moment.id for moment in read_case(name).moments if moment.is_outside],
            lines,
            describing=f"{name}'s watch lines",
        )
    equal(
        [moment.at for moment in bubbler.moments],
        sorted(moment.at for moment in bubbler.moments),
    )
    for name, replay in START_REPLAYS.items():
        truth(is_start_case(name), describing=f"{name}'s replay to be start requests alone")
        equal([request.id for request in read_start_case(name).requests], replay, describing=name)
    equal(sorted([*REPLAYS, *START_REPLAYS, bubbler.name]), replayed_case_names())


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


#: The real case the repository check is handed a copy of with one thing wrong.
BROKEN_COPY_CASE = "jar-top-wrong-material-detach-spaghetti"
#: A case's two periodic observations with one frame between them, and every
#: scenario of them: the timeline the tests of the replay's own clock read.
TIMELINE_CASE = "jar-top-wrong-material-first-layer-detach"
TIMELINE = ("first-layer-before-the-failure", "ring-dragged-off-its-place")


def _timeline() -> Case:
    """The two-event timeline the clock's tests read, whatever the suite replays of it."""
    return read_case(TIMELINE_CASE, replay=TIMELINE)


LAST = "wall-top-porous-and-stringing"


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
        (_past_the_end, "past the end of 3423-TOP-REV F brim.gcode"),
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
    root = _case_tree(tmp_path, BROKEN_COPY_CASE, edit)
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


def test_a_case_naming_a_calibration_configures_it_on_the_doubles_stream(tmp_path: Path) -> None:
    """The case's calibration is configured as it is, its stream the camera double's.

    The calibration was fitted on the recording's camera, which the double stands in
    for at an address of its own; every other field is the committed file's.
    """
    case = read_case("jar-top-petg-healthy")
    calibration = case.camera_calibration
    if calibration is None:
        pytest.fail("jar-top-petg-healthy names its calibration")
    copied = calibration_copy(calibration, "http://c", tmp_path)
    committed = json.loads(calibration.read_text(encoding="utf-8"))
    equal(
        json.loads(copied.read_text(encoding="utf-8")),
        {**committed, "camera_snapshot_url": "http://c/api/frame.jpeg"},
    )
    written = service_config(
        tmp_path, "127.0.0.1:1", "http://o", "http://c", "http://b", Minted(), calibration=copied
    )
    equal(
        tomllib.loads(written)["camera"],
        {"snapshot_url": "http://c/api/frame.jpeg", "calibration": copied.as_posix()},
    )
    without = service_config(tmp_path, "127.0.0.1:1", "http://o", "http://c", "http://b", Minted())
    equal(tomllib.loads(without)["camera"], {"snapshot_url": "http://c/api/frame.jpeg"})
    equal(read_case("under-extrusion-lace").camera_calibration, None)
    unreadable = tmp_path / "not-a-calibration.json"
    unreadable.write_text("[]", encoding="utf-8")
    with pytest.raises(ReplayError, match="no camera calibration"):
        calibration_copy(unreadable, "http://c", tmp_path)


def test_one_session_spans_the_print_and_each_check_stands_at_its_moment(scratch: Path) -> None:
    """The port opens one conversation, and each turn is handed its frame and its position."""
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    done = mock_replay(case, [{"commands": [CONTEXT, STATUS]}], scratch)
    ledger = done.replayed.ledger
    equal(len(ledger["sessions"]), 1)
    equal(len(ledger["turns"]), len(case.moments))
    equal(len({turn["session_name"] for turn in ledger["turns"]}), 1)
    arguments = [turn["arguments"] for turn in done.turns]
    truth("--resume" not in arguments[0], describing="the first event to open the session")
    truth("--resume" in arguments[1], describing="the later event to continue it")
    for turn, moment in zip(done.turns, case.moments, strict=True):
        answered = json.loads(turn["ran"][0]["out"])
        status = json.loads(turn["ran"][1]["out"])
        equal(status["job"]["file_position_bytes"], moment.file_position_bytes)
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
    case = _timeline()
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
    for name, case in (
        (TIMELINE_CASE, _timeline()),
        (
            "jar-top-wrong-material-detach-spaghetti",
            read_case("jar-top-wrong-material-detach-spaghetti"),
        ),
    ):
        faulty = case.moments[-1].event_image
        directory = scratch / name
        directory.mkdir()
        done = mock_replay(case, [{"commands": [LOOK, LOOK, LOOK]}], directory)
        seen = [done.seen_frame(0, look) for look in range(3)]
        truth(faulty not in seen, describing=f"{faulty} not to be among {seen}")


def test_every_counter_a_turn_reads_comes_from_one_clock(scratch: Path) -> None:
    """The record's times are the recording's, and a repeated frame says it is one."""
    case = _timeline()
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
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    done = mock_replay(case, [{"commands": [STATUS]}], scratch)
    started = {
        json.loads(turn["ran"][0]["out"])["print"]["job_started_at"][:16] for turn in done.turns
    }
    equal(len(started), 1, describing=f"one job start among {started}")


def test_the_last_events_looks_stop_at_what_its_recorded_turn_saw(scratch: Path) -> None:
    """A look after the last event does not jump to the print's end."""
    case = read_case("jar-top-wrong-material-detach-spaghetti")
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


def test_a_replay_moves_the_job_at_the_recordings_rate_after_its_first_event() -> None:
    """A look after a case's first event finds the job a little further on, not at its end."""
    for name in ("under-extrusion-lace", "fan-cut-bridge", "petg-spool-jam-air-print"):
        case = read_case(name)
        event = case.moments[0]
        at = instant(event.at)
        after_a_look = case.position_at(at + timedelta(seconds=90))
        truth(
            event.file_position_bytes <= after_a_look < case.gcode_size / 2,
            describing=f"{name} a little past {event.file_position_bytes}, at {after_a_look}",
        )


@pytest.mark.parametrize("name", ["jar-top-petg-healthy"])
def test_a_look_after_the_last_replayed_event_stops_short_of_the_recordings_next_moment(
    name: str, scratch: Path
) -> None:
    """More recording follows the list's end: the job moves on, and never jumps to it."""
    case = turns_of(name)
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


@pytest.mark.parametrize("name", ["jar-top-petg-healthy"])
def test_however_long_a_last_turn_runs_nothing_passes_the_next_recorded_moment(
    name: str, scratch: Path
) -> None:
    """Hours of waiting at the list's end still read as just short of what comes next."""
    case = turns_of(name)
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
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    done = mock_replay(case, [{"commands": [LOOK]}], scratch)
    equal(len(done.replayed.ledger["sessions"]), 1)


def test_a_recorded_alert_reaches_the_agent_at_its_time(scratch: Path) -> None:
    """Posted to the real webhook, it is the next turn's event, with its own picture and state."""
    case = read_case("petg-clip-detach-and-stringing-web")
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    prompts = [turn["prompt"] for turn in done.turns]
    contains(prompts[0], '"kind":"periodic_observation"')
    for prompt in prompts[1:]:
        contains(prompt, '"kind":"obico_failure_alert"')
    contains(prompts[2], '"detector_paused_the_print":true')
    # The alert's picture says when it was taken, and the check's its own frame's.
    alert_taken = instant(case.moments[1].at).strftime("%Y-%m-%dT%H:%M")
    contains(prompts[1], f'"frame_captured_at":"{alert_taken}')
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
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    # A turn that pauses answers that the print should stop, as the skill asks.
    done = mock_replay(case, [{"commands": [PAUSE], "disposition": "stop"}], scratch)
    carry_on, fault, moved = done.replayed.points
    contains(carry_on.why_not or "", "it must never pause")
    equal(fault.why_not, None)
    equal(moved.why_not, None)
    truth(not done.replayed.passed, describing="the test to fail on one failed decision point")


def test_an_accepted_action_passes_the_test(scratch: Path) -> None:
    """Carrying on at the carry-on point and pausing at the fault is the whole test passing."""
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    done = mock_replay(
        case, [{"commands": [CONTEXT]}, {"commands": [PAUSE], "disposition": "stop"}], scratch
    )
    truth(done.replayed.passed, describing=f"every decision point: {done.replayed.failures()}")


def test_the_agents_minimum_interval_spans_the_recorded_time_between_checks(
    scratch: Path,
) -> None:
    """A pause at one check leaves the next check's pause, recorded much later, unrefused.

    The replay reaches the second check seconds after the first, but the recording
    reached it far past the agent's minimum interval, and the print then would have
    taken a pause; one measured on the host's own clock would refuse it.
    """
    case = read_case(TIMELINE_CASE)
    first, second = case.moments
    truth(
        (instant(second.at) - instant(first.at)).total_seconds() > 30,
        describing="the two checks recorded further apart than the minimum interval",
    )
    pausing: ScriptedRun = {"commands": [PAUSE], "disposition": "stop"}
    done = mock_replay(case, [pausing, pausing], scratch)
    equal(
        [[(entry["status"], entry["err"]) for entry in turn["ran"]] for turn in done.turns],
        [[(0, "")], [(0, "")]],
        describing="each check's pause taken",
    )


def test_a_supervision_turns_compound_command_is_reported_under_its_conversation(
    scratch: Path,
) -> None:
    """A chained read is written down under the session it ran in, and the record carries it.

    With no ledger turn to read a decision point's session from, it is the print's last.
    """
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    chained = "printobserver context --print-id {print} --json; echo read"
    done = mock_replay(case, [{"commands": [chained]}, {"commands": [CONTEXT]}], scratch)
    replayed = done.replayed
    sessions = [session["session_name"] for session in replayed.ledger["sessions"]]
    equal(len(sessions), 1)
    ran = [line for line in replayed.points[0].commands if "; echo read" in line]
    equal(len(ran), 1)
    equal(replayed.compound_commands(), {sessions[0]: ran})
    equal(replayed.as_record()["compound_commands"], {sessions[0]: ran})
    unattributed = dataclasses.replace(replayed, ledger={**replayed.ledger, "turns": []})
    equal(unattributed.compound_commands(), {sessions[0]: ran})


def test_a_read_piped_only_through_a_filter_is_no_compound_command(scratch: Path) -> None:
    """Production runs a read piped into `grep`, so the run's record reports nothing of it."""
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    piped = 'printobserver context --print-id {print} --json | grep -E "detector|slicer"'
    done = mock_replay(case, [{"commands": [piped]}, {"commands": [CONTEXT]}], scratch)
    replayed = done.replayed
    ran = [line for line in replayed.points[0].commands if "| grep" in line]
    equal(len(ran), 1)
    equal(replayed.compound_commands(), {session: [] for session in replayed.compound_commands()})
    truth(
        all(not lines for lines in replayed.as_record()["compound_commands"].values()),
        describing="the record to carry no compound command",
    )


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
    """Nothing fires by accident, and the opt-in reaches every case and the probe."""
    command = ["uv", "run", "-q", "pytest", "--collect-only", "-q", "tests/skilltest"]
    environment = {k: v for k, v in os.environ.items() if k != "SKILLTEST_E2E"}
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    plain = run(command, cwd=REPO, env=environment)
    truth("test_replay" not in plain.stdout, describing=f"no live test in {plain.stdout}")
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    opted = run(command, cwd=REPO, env={**environment, "SKILLTEST_E2E": "1"}, check=True)
    contains(opted.stdout, f"test_replay.py: {len(replayed_case_names()) + PROBE_TURNS}")


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
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    before = case.moments[1].file_position_bytes - 100
    gcode = b"M140 S60\nM104 S220\n" + b";\n" * (before // 2) + b"M104 S240\n"
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch, gcode=gcode)
    tools = [json.loads(turn["ran"][0]["out"])["context"]["printer"] for turn in done.turns]
    # The last two checks stand past the change, so each reads its target.
    equal([printer["tools"][0]["target_c"] for printer in tools], [220.0, 240.0, 240.0])
    equal(tools[0]["bed"]["target_c"], 60.0)


@pytest.mark.parametrize(
    ("gcode", "change"),
    [
        # The first layer lays a skirt, so it ends at the second layer change.
        (
            b"G90\nM83\n;LAYER_CHANGE\n;TYPE:Skirt/Brim\nG1 X1 Y1 E0.5\n;LAYER_CHANGE\nG1 X2 E1\n",
            2,
        ),
        # A layer laying supports alone is skipped for the next.
        (
            b"M83\n;LAYER_CHANGE\n;TYPE:Support material\nG1 X1 E1\n;LAYER_CHANGE\n"
            b";TYPE:Perimeter\nG1 X2 E1\n;LAYER_CHANGE\nG1 X3 E1\n",
            3,
        ),
        # In absolute extrusion only a rising E lays material.
        (
            b"M82\nG92 E5\n;TYPE:Perimeter\n;LAYER_CHANGE\nG1 X1 E5\n;LAYER_CHANGE\n"
            b"G1 X2 E6\n;LAYER_CHANGE\nG1 X3 E7\n",
            3,
        ),
        # Extrusion under no declared type lays no part, as the index reads it.
        (b"M83\n;LAYER_CHANGE\nG1 X1 E1\n;LAYER_CHANGE\nG1 X2 E1\n", None),
        # Nothing after the first such layer, so it ends nowhere.
        (b";LAYER_CHANGE\n;TYPE:Perimeter\nG1 X1 E1\n", None),
    ],
)
def test_the_first_layer_ends_where_the_servers_index_says(
    gcode: bytes, change: int | None
) -> None:
    """At the `;LAYER_CHANGE` after the first layer extruding a declared non-support type."""
    changes = [found.start() for found in re.finditer(rb";LAYER_CHANGE", gcode)]
    equal(first_layer_end(gcode), None if change is None else changes[change - 1])


def test_the_watch_sees_the_job_pass_its_first_layer_between_two_events(scratch: Path) -> None:
    """Between two events the replay stands where the first layer went down, on its frame.

    A calibrated case whose first layer ends between its first two events: as the
    first event's turn ends, the stood-in printer reports the position the first
    frame past that layer records, and the camera answers that frame, before the
    next event stands as it always does — with one turn per event.
    """
    case = turns_of("jar-top-petg-healthy")
    ends = 10_000
    first, second = case.moments[:2]
    layer = b";LAYER_CHANGE\n;Z:0.2\nM83\n;TYPE:Perimeter\nG1 X1 Y1 E1\n"
    gcode = layer + b";\n" * ((ends - len(layer)) // 2) + b";LAYER_CHANGE\n;Z:0.4\n"
    gcode += b";\n" * ((second.file_position_bytes - len(gcode)) // 2 + 1000)
    layer_ends = first_layer_end(gcode)
    if layer_ends is None:
        pytest.fail("the slice's first layer ends nowhere")
    passing = next(
        (index, frame)
        for index, frame in enumerate(case.frames)
        if frame.progress_pct is not None
        and int(frame.progress_pct / 100 * case.gcode_size) > layer_ends
    )
    truth(
        instant(first.at) < case.frame_times()[passing[0]] <= instant(second.at),
        describing="the passing frame to fall between the first two events",
    )
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch, gcode=gcode)
    contains(done.replayed.fetches, passing[1].file)
    position = int((passing[1].progress_pct or 0) / 100 * case.gcode_size)
    truth(
        any(read.position == position for read in done.job_reads),
        describing=f"a job read at the passing position {position}",
    )
    equal(len(done.replayed.ledger["turns"]), len(case.moments))


def test_a_first_layer_down_before_the_first_event_is_decided_before_it(scratch: Path) -> None:
    """The server is started once, with no check due, to decide at the passing frame.

    The case's first layer ended before its first event, so the replay first starts
    the server at the frame taken just past it, until it decides; then starts it
    again over the same state at the first event. The verdict stands, the camera was
    asked for that frame before the first check's own, and no turn ran beyond one
    per event.
    """
    case = read_case(TIMELINE_CASE)
    first = case.moments[0]
    layer = b";LAYER_CHANGE\n;Z:0.2\nM83\n;TYPE:Perimeter\nG1 X1 Y1 E1\n"
    gcode = layer + b";LAYER_CHANGE\n;Z:0.4\n"
    gcode += b";\n" * ((case.moments[-1].file_position_bytes - len(gcode)) // 2 + 1000)
    truth(
        first_layer_end(gcode) is not None
        and (first_layer_end(gcode) or 0) < first.file_position_bytes,
        describing="the first layer to end before the first event",
    )
    passing = next(
        frame
        for frame in case.frames
        if frame.progress_pct is not None
        and int(frame.progress_pct / 100 * case.gcode_size) > (first_layer_end(gcode) or 0)
    )
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch, gcode=gcode)
    equal(done.replayed.fetches[:2], [passing.file, first.event_image])
    truth(done.replayed.alignment is not None, describing="the verdict decided before it")
    equal(len(done.replayed.ledger["turns"]), len(case.moments))


def test_a_timeline_passing_its_first_layer_answers_that_until_the_next_event(
    tmp_path: Path,
) -> None:
    """While passing, every job read and camera fetch answers the passing frame; then not."""
    case = turns_of("jar-top-petg-healthy")
    timeline = doubles.Timeline(case=case, picture=stamped, clock=tmp_path / "clock")
    when = case.frame_times()[6]
    timeline.pass_at(doubles.Passing(22_555, 6, when))
    equal(timeline.position(), 22_555)
    truth(
        timedelta() <= timeline.now() - when < timedelta(seconds=5),
        describing="the clock to stand at the passing frame's capture",
    )
    for _ in range(2):
        due = timeline.next_fetch()
        equal(due.name if due else None, case.frames[6].file)
    timeline.stand_at(1)
    equal(timeline.passing, None)
    equal(timeline.position(), case.moments[1].file_position_bytes)


def test_a_gcode_named_with_spaces_is_planned_through_the_real_adapter(scratch: Path) -> None:
    """The stand-in's download link reaches the file whatever its name holds.

    The case's G-code is named with spaces, so a link written with its name raw
    is one the adapter cannot follow, and every turn would be handed no plan.
    """
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    truth(" " in case.gcode.name, describing="the case's G-code to be named with spaces")
    last = max(moment.file_position_bytes for moment in case.moments)
    gcode = b";LAYER_CHANGE\n;Z:0.2\n" + b";\n" * (last // 2 + 1)
    done = mock_replay(case, [{"commands": [CONTEXT]}], scratch, gcode=gcode)
    plans = [json.loads(turn["ran"][0]["out"])["context"].get("slicer") for turn in done.turns]
    equal([plan and plan.get("layer") for plan in plans], [1] * len(case.moments))


@dataclass(frozen=True)
class Looked:
    """What one look handed the agent."""

    #: The frame it answered.
    frame: str
    #: When that frame was captured.
    frame_captured_at: str
    #: How long the look waited, in whole seconds.
    waited_s: int
    #: When the look's event was received, to the second.
    received_at: str
    #: Where the job stood.
    file_position_bytes: int


def _looked(done: Scripted) -> list[Looked]:
    """What each look of the first turn handed the agent."""
    seen = []
    for at, entry in enumerate(e for e in done.turns[0]["ran"] if e["words"][1] == "look"):
        answered = json.loads(entry["out"])
        payload = answered["event"]["payload"]
        seen.append(
            Looked(
                frame=done.seen_frame(0, at),
                frame_captured_at=str(payload["frame_captured_at"]),
                waited_s=int(payload["waited_s"]),
                received_at=str(answered["event"]["received_at"])[:19],
                file_position_bytes=int(answered["job"]["file_position_bytes"]),
            )
        )
    return seen


def _offsets_at(done: Scripted, looked: list[Looked]) -> list[timedelta]:
    """The clock's offset from real time at the job read each look answered.

    Each look's read is the first after the previous look's to place the job
    where that look said it stood.
    """
    offsets, at = [], 0
    for look in looked:
        at = next(
            (
                index
                for index in range(at, len(done.job_reads))
                if done.job_reads[index].position == look.file_position_bytes
            ),
            -1,
        )
        truth(at >= 0, describing=f"a read of the job that placed it at {look.file_position_bytes}")
        offsets.append(done.job_reads[at].offset)
    return offsets


def test_a_looks_wait_passes_on_the_replays_clock_and_hands_the_agent_the_same(
    tmp_path: Path,
) -> None:
    """A look's wait passes on the replay's clock and hands the agent the same.

    It passes at once, and hands the agent exactly what waiting it out in real
    time hands it.

    The frame, its capture time and the wait are compared whole, and the look's
    received time to the second. The job's position is read off the replay's
    clock some milliseconds after the frame is fetched, and those milliseconds
    are the host's, which no two runs share, so the two runs' positions are not
    compared. What says the job stood where waiting it out puts it is the clock
    each position was read by: between the two looks' reads of the job, the
    jumped replay's clock moved on from real time by exactly the wait, and the
    real-time replay's not at all.
    """
    # One frame lies between this case's two events: the first look moves the
    # clock to it, and the second, with nothing newer, moves it by its wait alone.
    case = _timeline()
    turn: ScriptedRun = {"commands": [LOOK_WAITING, LOOK_WAITING]}
    for each in ("real", "jumped"):
        (tmp_path / each).mkdir()
    real_at = time.monotonic()
    real = mock_replay(case, [turn], tmp_path / "real", real_waits=True)
    real_s = time.monotonic() - real_at
    jumped_at = time.monotonic()
    jumped = mock_replay(case, [turn], tmp_path / "jumped")
    jumped_s = time.monotonic() - jumped_at
    looked, expected = _looked(jumped), _looked(real)
    equal(
        [replace(look, file_position_bytes=0) for look in looked],
        [replace(look, file_position_bytes=0) for look in expected],
    )
    equal(expected[0].frame, expected[1].frame)
    for runs in (looked, expected):
        truth(
            runs[1].file_position_bytes > runs[0].file_position_bytes,
            describing="the job to move on while the second look waits",
        )
    jumped_offsets, real_offsets = _offsets_at(jumped, looked), _offsets_at(real, expected)
    equal(jumped_offsets[1] - jumped_offsets[0], timedelta(seconds=LOOK_WAIT_S))
    equal(real_offsets[1] - real_offsets[0], timedelta())
    equal([look.waited_s for look in looked], [LOOK_WAIT_S, LOOK_WAIT_S])
    truth(
        jumped_s < real_s - LOOK_WAIT_S,
        describing=f"the jumped replay ({jumped_s:.1f} s) to skip the real waits ({real_s:.1f} s)",
    )


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


def _detectors_read(done: Scripted) -> list[str]:
    """The detector's status each turn's context read answered, in turn order."""
    return [
        json.loads(entry["out"])["context"]["detector"]
        for seen in done.turns
        for entry in seen["ran"]
        if entry["words"][1] == "context"
    ]


def test_the_real_supervisor_reads_the_replays_detector_through_obicos_record(
    scratch: Path,
) -> None:
    """The real server reads the double's printer record over HTTP into every turn's context.

    A case recording the detector is one Obico watched, so once two of the server's
    reads apart have seen its snapshot move, a turn reads it scoring with nothing
    flagged; the same case with no detector recorded never reads scoring.
    """
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    truth(len(case.moments) >= 2, describing="two turns, so two reads lie between them")
    for directory in ("watched", "unwatched"):
        (scratch / directory).mkdir()
    watched = _detectors_read(mock_replay(case, [{"commands": [CONTEXT]}], scratch / "watched"))
    equal(len(watched), len(case.moments))
    equal(watched[-1], "scoring_nothing_flagged")

    unrecorded = dataclasses.replace(case, detector_recorded=False)
    unwatched = _detectors_read(
        mock_replay(unrecorded, [{"commands": [CONTEXT]}], scratch / "unwatched")
    )
    equal(len(unwatched), len(case.moments))
    truth(
        not any(status.startswith("scoring") for status in unwatched),
        describing=f"no scoring read for a case recording no detector: {unwatched}",
    )


def test_the_obico_doubles_printer_record_is_the_cases_recorded_detector(scratch: Path) -> None:
    """A case recording the detector names its print and a snapshot that moves; none, no print.

    The print is named by the identifier the case's first recorded alert names, its
    file and its start, which is what the server matches it to its own print by.
    """
    case = read_case("petg-clip-detach-and-stringing-web")
    truth(case.detector_recorded, describing="the case records what the detector did")
    timeline = doubles.Timeline(case=case, picture=stamped, clock=scratch / "clock")
    first = timeline.obico_printer()
    named = case.obico_print_id()
    truth(named is not None, describing="the case's recorded alert names Obico's print")
    current = first["current_print"]
    truth(current is not None, describing="the record names the print Obico watched")
    if current is not None:
        equal(current["id"], named)
        equal(current["filename"], case.gcode.name)
        equal(datetime.fromisoformat(current["started_at"]), case.job_started())
    truth("auth_token" not in first, describing="the double carries no printer credential")
    # The replay's clock is read from its file, so a read later in real time is
    # a read later on the replay's clock; a snapshot is named to the second.
    time.sleep(1.1)
    later = timeline.obico_printer()
    truth(
        later["pic"] != first["pic"], describing="a watching Obico's snapshot moved between reads"
    )

    unrecorded = dataclasses.replace(case, detector_recorded=False)
    quiet = doubles.Timeline(case=unrecorded, picture=stamped, clock=scratch / "quiet-clock")
    equal(quiet.obico_printer()["current_print"], None)

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


def test_the_suites_build_leaves_the_ordinary_program_alone() -> None:
    """The replay build never replaces the program every other target builds and digests.

    This target runs after `build-programs`. Had that build written its replay-clock
    `printobserver` over the dev profile's, a concurrent test would find a program other
    than the one it had already packaged.
    """
    metadata = run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"], cwd=REPO
    )
    equal(metadata.returncode, 0, describing=f"cargo metadata: {metadata.stderr}")
    built = program("printobserver")
    ordinary = Path(json.loads(metadata.stdout)["target_directory"]) / "debug" / built.name
    replay_clock = b"PRINTOBSERVER_REPLAY_REAL_WAITS"
    contains(built.read_bytes(), replay_clock, describing="the suite's own build")
    if ordinary.is_file():
        truth(
            replay_clock not in ordinary.read_bytes(),
            describing=f"{ordinary} to be the ordinary build, not the suite's",
        )


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
    equal(reported["value"], sum(counted.values()) + PROBE_TURNS)
    equal(json.loads(reported["detail"]), {"turns_per_test": counted, "probe_turns": PROBE_TURNS})
    contains(done.stderr, f"turns per full run: {sum(counted.values()) + PROBE_TURNS}")


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
    case = read_case("jar-top-wrong-material-detach-spaghetti")
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
            read_case("jar-top-wrong-material-detach-spaghetti"),
            [{"commands": [CONTEXT]}],
            scratch,
        )


def test_the_doubles_refuse_what_no_adapter_sends(scratch: Path) -> None:
    """A malformed body, an unknown printer command and another Obico route are refused."""
    timeline = doubles.Timeline(
        case=read_case("jar-top-wrong-material-detach-spaghetti"),
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
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    done = mock_replay(case, [{"commands": [LOOK, LOOK]}, {"commands": [CONTEXT]}], scratch)
    equal([point.looks for point in done.replayed.points], [2, 0, 0])


def test_a_turn_says_each_call_once_and_what_it_opened(scratch: Path) -> None:
    """A call the transcript writes twice ran once, and a file tool's read is reported."""
    case = read_case("jar-top-wrong-material-detach-spaghetti")
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
    case = read_case(TIMELINE_CASE)
    unreadable: ScriptedRun = {"commands": [CONTEXT], "unreadable": True}
    done = mock_replay(case, [unreadable, unreadable, unreadable, {"commands": [CONTEXT]}], scratch)
    first, second = done.replayed.points
    truth(first.why_not is not None, describing="the failed turn to fail its decision point")
    contains(str(first.why_not), "the turn failed")
    # The harness's own words are kept, because the engine's refusal names no cause.
    contains(str(first.why_not), "the harness answered:")
    contains(str(first.why_not), "no answer in the shape asked for")
    truth(second.moment.id == case.moments[1].id, describing="the next event to get its turn")
    equal(first.assessment, None, describing="the failed turn, which answered nothing")
    truth(second.assessment is not None, describing="the next turn's assessment to be its own")
    equal(done.replayed.passed, False)


def test_a_stop_that_stopped_nothing_fails_its_decision_point(scratch: Path) -> None:
    """An answer of `stop` with no pause, refused and given again, fails its decision point.

    The server refuses it once in the same turn, which the replay keeps as that turn,
    and writes it down when it comes again; the record carries why it failed.
    """
    case = read_case(TIMELINE_CASE)
    unacted: ScriptedRun = {"commands": [], "disposition": "stop"}
    done = mock_replay(case, [{"commands": [CONTEXT]}, unacted, unacted], scratch)
    before, dragged = done.replayed.points
    equal(before.why_not, None)
    truth(dragged.stopped_nothing, describing="the unacted stop to be read off the record")
    contains(dragged.why_not or "", "it answered stop and asked for no pause")
    equal(len(done.replayed.ledger["turns"]), 2, describing="the refusal stays one turn")
    equal(len(done.turns), 3, describing="the harness ran the refusal as a run of its own")


def test_only_the_ingress_accepting_an_alert_counts_as_delivering_it() -> None:
    """A webhook answering anything but acceptance is no delivered alert."""
    equal(delivered(202), True)
    equal(delivered(403), False)


def test_a_run_the_engine_re_prompts_stays_one_turn_at_its_own_moment(scratch: Path) -> None:
    """The engine's second run of one turn neither ends it nor moves the replay on."""
    case = read_case(TIMELINE_CASE)
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
        "calls": [{"input_tokens": 3, "output_tokens": 80, "cache_write_tokens": 5_000}],
        "check_class": "quiet",
    }
    before, dragged = REPLAYS["jar-top-wrong-material-first-layer-detach"]
    points = [{**point, "scenario": before}, {**point, "scenario": dragged}]
    test_record = {
        "test": "jar-top-wrong-material-first-layer-detach",
        "passed": True,
        "turns": 2,
        "sessions": [],
        "compound_commands": {},
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
        ("no compound report", {k: v for k, v in test_record.items() if k != "compound_commands"}),
        ("a conversation unreported", {**test_record, "sessions": ["operator-2026-10-07"]}),
        ("compound", {**test_record, "compound_commands": {"": [3]}}),
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
        (
            "an image of no kind",
            {
                **test_record,
                "decision_points": [
                    {**points[0], "image": {"kind": "panels", "why": None, "note": "x"}},
                    points[1],
                ],
            },
        ),
        (
            "an image with no line",
            {
                **test_record,
                "decision_points": [
                    {**points[0], "image": {"kind": "composite", "why": None}},
                    points[1],
                ],
            },
        ),
        (
            "a passing check reporting no call",
            {**test_record, "decision_points": [{**points[0], "calls": []}, points[1]]},
        ),
        (
            "a call reporting no counts",
            {**test_record, "decision_points": [{**points[0], "calls": [{}]}, points[1]]},
        ),
        (
            "a negative count",
            {
                **test_record,
                "decision_points": [
                    {**points[0], "calls": [{"input_tokens": -1, "output_tokens": 2}]},
                    points[1],
                ],
            },
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
    equal(
        folded["figures"],
        {"turns": 2, "quiet_check_model_calls": 1.0},
        describing="the figures of the run it wrote, which kept no probe",
    )
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
    """`record` over a mock run writes each usage as the mock reported it.

    And each check's calls as the supervisor port recorded them from the stream
    the harness wrote, with its class: quiet for the read, acting for the pause.
    """
    case = read_case(TIMELINE_CASE)
    usages = [
        {"input_tokens": 101, "output_tokens": 11, "cache_read_input_tokens": 7},
        {"input_tokens": 202, "output_tokens": 22, "cache_read_input_tokens": 9},
    ]
    done = mock_replay(
        case,
        [
            {"commands": [CONTEXT], "usage": usages[0]},
            {"commands": [PAUSE], "usage": usages[1], "compact": True, "disposition": "stop"},
        ],
        scratch,
    )
    run_directory = tmp_path / "run"
    run_directory.mkdir()
    (run_directory / f"{case.name}.json").write_text(
        json.dumps(done.replayed.as_record()), encoding="utf-8"
    )
    written = record.folded(run_directory, "a-commit")
    equal(
        written["figures"],
        {
            "turns": 2,
            "quiet_check_model_calls": 1.0,
            "acting_check_model_calls": 1.0,
            "compaction_delayed_checks": 0.0,
        },
    )
    detail = written["detail"]
    equal(
        detail["compaction"],
        {"idle_compact_tokens": 150_000, "auto_compact_tokens": 200_000},
        describing="the replays run with the installer's compaction settings",
    )
    equal(detail["compaction"], installer_compaction())
    equal(detail["idle_compactions"], 0)
    equal(detail["commit"], "a-commit")
    equal(detail["harness"], "claude-code")
    equal(detail["models"], ["mock-model"])
    equal(detail["compactions"], 1)
    equal(detail["turns_the_cases_count"], 2)
    points = detail["tests"][0]["decision_points"]
    equal([point["usage"] for point in points], usages)
    equal([point["compacted"] for point in points], [False, True])
    equal(
        [point["calls"] for point in points],
        [
            [{"input_tokens": 101, "output_tokens": 11, "cache_read_tokens": 7}],
            [{"input_tokens": 202, "output_tokens": 22, "cache_read_tokens": 9}],
        ],
        describing="each check's calls, as the port recorded them",
    )
    equal([point["check_class"] for point in points], ["quiet", "acting"])
    contains(points[1]["commands"][0], "printobserver pause")


def test_the_facts_are_measured_off_the_prompt_a_real_check_is_handed(scratch: Path) -> None:
    """The replay finds the facts in the prompt the supervisor really wrote.

    `replay.FACTS_HEADING` restates the turn prompt's heading; a prompt that moved
    its facts would leave every check's `facts_bytes` unmeasured, which this fails.
    """
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    handed = (scratch / mock_turn.CONVERSATION_FILE).read_text(encoding="utf-8")
    report = RunReport()

    report.take([{"type": "user", "message": {"content": [{"type": "text", "text": handed}]}}])

    truth(
        report.facts_bytes is not None and report.facts_bytes > 0,
        describing=f"the facts measured off the prompt the check was handed: {handed}",
    )


def test_the_image_a_check_was_shown_is_read_off_the_prompt_a_real_check_is_handed(
    scratch: Path,
) -> None:
    """The replay reads which image a check got, and why, off the real server's prompt.

    The case names a calibration, so the server weighs the three-panel image; with
    no plan to place the check in, it is shown the whole frame, which the server's
    line says, and the record carries it as the whole frame with that reason.
    """
    case = read_case("jar-top-wrong-material-first-layer-detach")
    mock_replay(case, [{"commands": [CONTEXT]}], scratch)
    handed = (scratch / mock_turn.CONVERSATION_FILE).read_text(encoding="utf-8")
    report = RunReport()

    report.take([{"type": "user", "message": {"content": [{"type": "text", "text": handed}]}}])

    note = report.image_note
    if note is None:
        pytest.fail(f"no image line in the prompt the check was handed: {handed}")
    shown = check_image(note)
    equal(shown["kind"], "whole_frame", describing=note)
    # The gate serves no case's G-code, an LFS object it never reads, so the server
    # has no plan to place this check in; the live suite serves the real file.
    contains(shown["why"] or "", "G-code index gives no file position")


def test_the_composers_image_line_reads_as_the_composite(tmp_path: Path) -> None:
    """The server's own composition says its image's line, which reads as the composite.

    Composed of the plain fixture copies of a calibrated case's frame and slice, so
    the replay's reading of the line is held to what the server writes rather than
    to a copy of its words; the whole frame's line is held so by the replay above.
    """
    calibration = read_case("jar-top-petg-healthy").camera_calibration
    if calibration is None:
        pytest.fail("jar-top-petg-healthy names its calibration")
    out = tmp_path / "check-image.jpg"
    said = composed_check_image(
        (
            REPO / "crates/printobserver-core/tests/fixtures/real-prints/jar-top-petg.gcode"
        ).read_bytes(),
        REPO
        / "crates/printobserver-panels/tests/fixtures/frames/jar-top-petg-healthy"
        / "agent-018-obs-123246-p24.5.jpg",
        1_385_571,
        calibration,
        out,
    )
    equal((said["width"], said["height"], said["bytes"]), (1024, 1848, out.stat().st_size))
    equal(check_image(said["note"])["kind"], "composite", describing=said["note"])
    with pytest.raises(ReplayError, match="could not be composed"):
        composed_check_image(b"", tmp_path / "absent.jpg", 1, calibration, tmp_path / "x.jpg")


def test_each_image_line_the_server_writes_is_read_as_its_kind() -> None:
    """The composite, the whole frame with its reason, and a frame no line names apart."""
    equal(check_image("the three-panel check image, 1024x1848: panel 1 ...")["kind"], "composite")
    whole = check_image(
        "the whole camera frame, scaled to fit 1280x720 and uncropped, because no camera "
        "calibration is configured"
    )
    equal((whole["kind"], whole["why"]), ("whole_frame", "no camera calibration is configured"))
    stored = check_image("the frame as the camera took it, because its check image could not be")
    equal(stored["kind"], "stored_frame")


@pytest.mark.parametrize(
    ("kept", "read"),
    [
        (
            {"verdict": "passed", "offset_px": 14.6},
            {"verdict": "passed", "offset_px": 14.6, "failure": None},
        ),
        (
            {"verdict": "failed", "reason": {"failure": "offset", "offset_px": 48.5}},
            {"verdict": "failed", "offset_px": 48.5, "failure": "offset"},
        ),
        (
            {
                "verdict": "failed",
                "reason": {"failure": "resolution", "expected": [1920, 1080], "got": [1280, 720]},
            },
            {"verdict": "failed", "offset_px": None, "failure": "resolution"},
        ),
        (
            {"verdict": "unavailable"},
            {"verdict": "unavailable", "offset_px": None, "failure": None},
        ),
        ({"offset_px": 3}, None),
        ([], None),
    ],
)
def test_a_cases_alignment_verdict_is_read_as_the_server_keeps_it(
    kept: object, read: object
) -> None:
    """Each verdict the server writes reads as its verdict and offset; anything else as none.

    Each is held to the committed `PrintAlignment` schema the server's contract test
    keeps in step with its type, so these are verdicts the server can write.
    """
    equal(alignment_of(kept), read)


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


def _a_check(commands: list[str], requests: list[Request]) -> DecisionPoint:
    """A decision point whose outcome any requests meet, so only how it acted can fail it."""
    case = read_case("jar-top-wrong-material-detach-spaghetti")
    moment = dataclasses.replace(case.moments[0], accept_any_of=((),), never=())
    return DecisionPoint(
        moment=moment,
        requests=requests,
        commands=commands,
        opened=[],
        assessment=None,
        usage=None,
        models=[],
        compacted=False,
        failure=None,
        event_kind="periodic_observation",
        image_sha256=None,
        looks=0,
    )


@pytest.mark.parametrize(
    "acting",
    [
        "printobserver pause --print-id P --actor A --reason r",
        "printobserver cancel --print-id P --actor A --reason r",
        "printobserver resume --print-id P --actor A --reason r",
        "printobserver set-fan-percent --print-id P --percent 80 --actor A --reason r",
        "printobserver set-flowrate-factor --print-id P --factor 1.0 --actor A --reason r",
        "printobserver set-feedrate-factor --print-id P --factor 0.9 --actor A --reason r",
        "printobserver set-tool-target-c --print-id P --tool 0 --target 200 --actor A --reason r",
        "printobserver set-bed-target-c --print-id P --target 60 --actor A --reason r",
    ],
)
def test_a_check_that_looks_again_after_acting_fails_its_decision_point(acting: str) -> None:
    """A `look` anywhere after a command that acts fails the check, naming both."""
    looked = _a_check(
        [acting, "printobserver context --print-id P", "printobserver look --print-id P 2>&1"],
        [Request(acting.split()[1], {})],
    )
    reason = looked.why_not
    truth(reason is not None, describing=f"a look after `{acting}` to fail its check")
    contains(str(reason), "it looked again after acting")
    contains(str(reason), acting.split()[1])
    equal(looked.as_record()["passed"], False)


def test_a_check_that_looks_before_acting_passes_its_decision_point() -> None:
    """A look before the action, and an action with no look after it, pass."""
    for commands in (
        ["printobserver look --print-id P", "printobserver pause --print-id P --reason r"],
        ["printobserver set-fan-percent --print-id P --percent 80 --reason r"],
        ["printobserver look --print-id P", "printobserver look --print-id P"],
    ):
        equal(_a_check(commands, []).why_not, None, describing=f"{commands}")


def _point_with(scenario: str, check_class: str, calls: list[dict[str, int]]) -> dict[str, Any]:
    point, _ = _a_finished_test()
    return {**point[0], "scenario": scenario, "check_class": check_class, "calls": calls}


def test_the_record_works_out_each_figure_the_budgets_read(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Every figure is the value worked out by hand from known calls, history-free inputs included.

    One test of two checks — quiet, then acting — and a probe whose first call reads
    4,000. Each call's input is its input, cache reads and cache writes.
    """
    monkeypatch.setattr(record, "commit", lambda: "a-commit")
    finished = tmp_path / "finished"
    finished.mkdir()
    first, second = REPLAYS["jar-top-wrong-material-first-layer-detach"]
    # The first check: one call reading 6,000, the probe's base plus its own 2,000.
    quiet = [
        {
            "input_tokens": 10,
            "cache_write_tokens": 2_000,
            "cache_read_tokens": 3_990,
            "output_tokens": 100,
        }
    ]
    # The second, acting: two calls, after a check whose last call read 6,000 and wrote 100.
    acting = [
        {"input_tokens": 20, "cache_read_tokens": 9_000, "output_tokens": 50},
        {"input_tokens": 30, "cache_read_tokens": 9_500, "output_tokens": 60},
    ]
    points = [_point_with(first, "quiet", quiet), _point_with(second, "acting", acting)]
    test_record = {
        "test": "jar-top-wrong-material-first-layer-detach",
        "passed": True,
        "turns": 2,
        "sessions": [],
        "compound_commands": {},
        "decision_points": points,
    }
    (finished / "jar-top-wrong-material-first-layer-detach.json").write_text(
        json.dumps(test_record), encoding="utf-8"
    )
    probe = {
        "prompt": "Answer with the assessment for a print that is printing normally.",
        "calls": [{"input_tokens": 4, "cache_write_tokens": 3_996, "output_tokens": 70}],
        "skill_bytes": 5_983,
    }
    (finished / PROBE_RECORD).write_text(json.dumps(probe), encoding="utf-8")
    # The image probe: the same prompt with one 1024x1848 check image, its first call
    # reading 2,443 more than the text-only probe's.
    image_probe = {
        "prompt": probe["prompt"],
        "calls": [{"input_tokens": 4, "cache_write_tokens": 6_439, "output_tokens": 71}],
        "case": "jar-top-petg-healthy",
        "scenario": "carry-on-031-other",
        "width": 1024,
        "height": 1848,
        "bytes": 412_345,
    }
    (finished / IMAGE_PROBE_RECORD).write_text(json.dumps(image_probe), encoding="utf-8")

    folded = record.folded(finished, "a-commit")
    base = 4_000
    quiet_input = 10 + 2_000 + 3_990
    history = quiet_input + 100
    acting_raw = (20 + 9_000) + (30 + 9_500)
    equal(
        folded["figures"],
        {
            "turns": 2,
            "quiet_check_model_calls": 1.0,
            "acting_check_model_calls": 2.0,
            # The first check carries nothing: its history is the base.
            "quiet_check_input_tokens": float(quiet_input),
            "acting_check_input_tokens": float(acting_raw - 2 * (history - base)),
            "base_context_tokens": float(base),
            "frame_image_tokens": float((4 + 6_439) - base),
        },
    )
    equal(folded["figures"]["frame_image_tokens"], 2_443.0, describing="by hand")
    equal(folded["figures"]["acting_check_input_tokens"], 14_350.0, describing="by hand")
    detail = folded["detail"]["figures"]
    equal(detail["base_context_tokens"]["cache_write_tokens"], 3_996)
    equal(detail["base_context_tokens"]["skill_bytes"], 5_983)
    equal(detail["acting_check_input_tokens"]["each"][0]["history"], history)
    equal(detail["acting_check_model_calls"]["max"], 2)
    equal(detail["quiet_check_model_calls"]["above_one_call"], [])
    equal(folded["detail"]["probe"], probe)
    equal(folded["detail"]["image_probe"], image_probe)
    equal(
        detail["frame_image_tokens"],
        {
            "case": "jar-top-petg-healthy",
            "scenario": "carry-on-031-other",
            "pixels": 1024 * 1848,
            "width": 1024,
            "height": 1848,
            "bytes": 412_345,
            "image_probe_first_call_input": 6_443,
            "text_probe_first_call_input": 4_000,
            # Anthropic's rule, one token per 28x28 px rounded up: 37 x 66 and 46 x 26.
            "rule_tokens": 2_442,
            "whole_frame_rule_tokens": 1_196,
        },
    )

    # With no image probe, no image figure; one that is no record is refused.
    (finished / IMAGE_PROBE_RECORD).unlink()
    equal("frame_image_tokens" in record.folded(finished, "a-commit")["figures"], False)
    for unusable in ({**image_probe, "width": 0}, {**image_probe, "calls": []}, []):
        (finished / IMAGE_PROBE_RECORD).write_text(json.dumps(unusable), encoding="utf-8")
        with pytest.raises(record.UnreadableRunError, match="no image probe"):
            record.folded(finished, "a-commit")
    (finished / IMAGE_PROBE_RECORD).unlink()

    # With no probe, no input figure can be worked out, and none is written.
    (finished / PROBE_RECORD).unlink()
    without = record.folded(finished, "a-commit")["figures"]
    equal(
        without,
        {"turns": 2, "quiet_check_model_calls": 1.0, "acting_check_model_calls": 2.0},
    )


def test_the_record_splits_each_figure_by_part(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Each part of the base and of the median checks is the value worked out by hand.

    One test of three checks — quiet, acting, quiet — beside the three probes: the text
    probe's first call reads 4,000, the harness-only probe's 2,500 and the image probe's
    6,443, so the harness's part is 2,500, printobserver's 1,500 and the image 2,443.
    """
    monkeypatch.setattr(record, "commit", lambda: "a-commit")
    finished = tmp_path / "finished"
    finished.mkdir()
    name = "jar-top-wrong-material-detach-spaghetti"
    first, second, third = REPLAYS[name]
    # 7,500 with the base as its history: the base, the image and 1,057 of facts.
    quiet = [
        {
            "input_tokens": 10,
            "cache_write_tokens": 3_500,
            "cache_read_tokens": 3_990,
            "output_tokens": 100,
        },
    ]
    # After a conversation of 7,600: 11,020 then 11,530, each carrying 3,600 of history.
    acting = [
        {"input_tokens": 20, "cache_read_tokens": 11_000, "output_tokens": 50},
        {"input_tokens": 30, "cache_read_tokens": 11_500, "output_tokens": 60},
    ]
    # After a conversation of 11,590: 14,540, carrying 7,590 of history.
    later = [
        {
            "input_tokens": 40,
            "cache_read_tokens": 14_000,
            "cache_write_tokens": 500,
            "output_tokens": 80,
        },
    ]
    points = [
        _point_with(first, "quiet", quiet),
        _point_with(second, "acting", acting),
        _point_with(third, "quiet", later),
    ]
    (finished / f"{name}.json").write_text(
        json.dumps(
            {
                "test": name,
                "passed": True,
                "turns": 3,
                "sessions": [],
                "compound_commands": {},
                "decision_points": points,
            }
        ),
        encoding="utf-8",
    )
    prompt = "Answer with the assessment for a print that is printing normally."
    for file, calls in (
        (PROBE_RECORD, [{"input_tokens": 4, "cache_write_tokens": 3_996, "output_tokens": 70}]),
        (
            BARE_PROBE_RECORD,
            [{"input_tokens": 4, "cache_write_tokens": 2_496, "output_tokens": 70}],
        ),
    ):
        written: dict[str, object] = {"prompt": prompt, "calls": calls}
        if file == PROBE_RECORD:
            written["skill_bytes"] = 5_983
        (finished / file).write_text(json.dumps(written), encoding="utf-8")
    (finished / IMAGE_PROBE_RECORD).write_text(
        json.dumps(
            {
                "prompt": prompt,
                "calls": [{"input_tokens": 4, "cache_write_tokens": 6_439, "output_tokens": 71}],
                "case": "jar-top-petg-healthy",
                "scenario": "carry-on-031-other",
                "width": 1024,
                "height": 1848,
                "bytes": 412_345,
            }
        ),
        encoding="utf-8",
    )

    folded = record.folded(finished, "a-commit")
    detail = folded["detail"]["figures"]
    equal(detail["base_context_tokens"]["split"], {"harness": 2_500, "printobserver": 1_500})
    # Quiet: 7,500 and 14,540 - (11,590 - 4,000) = 6,950; the lower middle is 6,950.
    equal(folded["figures"]["quiet_check_input_tokens"], 7_225.0)
    equal(
        detail["quiet_check_input_tokens"]["split"],
        {
            "test": name,
            "scenario": third,
            "input": 6_950,
            "calls": 1,
            "harness": 2_500,
            "skill_and_instruction": 1_500,
            "image": 2_443,
            "facts_and_framing": 6_950 - 4_000 - 2_443,
        },
    )
    # It left 14,620 - 11,590; the first quiet check has no check before it.
    equal(detail["quiet_check_input_tokens"]["leaves"], 3_030.0)
    # Acting: 22,550 - 2 x 3,600 = 15,350, its first call 11,020 - 3,600 = 7,420.
    equal(folded["figures"]["acting_check_input_tokens"], 15_350.0)
    equal(
        detail["acting_check_input_tokens"]["split"],
        {
            "test": name,
            "scenario": second,
            "input": 15_350,
            "calls": 2,
            "harness": 2_500,
            "skill_and_instruction": 1_500,
            "image": 2_443,
            "facts_and_framing": 7_420 - 4_000 - 2_443,
            "repeated": 7_420,
            "action_and_result": 15_350 - 2 * 7_420,
        },
    )
    equal(detail["acting_check_input_tokens"]["leaves"], float(11_590 - 7_600))
    bare = folded["detail"]["bare_probe"]
    truth(bare is not None, describing="the harness-only probe in the record")
    equal(bare["calls"][0]["cache_write_tokens"] if bare else None, 2_496)

    # Without the harness-only probe the base stands whole in each split.
    (finished / BARE_PROBE_RECORD).unlink()
    without = record.folded(finished, "a-commit")["detail"]["figures"]
    equal(without["base_context_tokens"]["split"], None)
    equal(without["quiet_check_input_tokens"]["split"]["base"], 4_000)
    equal("harness" in without["quiet_check_input_tokens"]["split"], False)
    # One that is no record is refused.
    (finished / BARE_PROBE_RECORD).write_text(json.dumps({"prompt": prompt, "calls": []}))
    with pytest.raises(record.UnreadableRunError, match="no harness-only probe"):
        record.folded(finished, "a-commit")


@pytest.mark.parametrize(
    "usage",
    [
        {},
        {"input_tokens": 3},
        {"input_tokens": 3, "output_tokens": -1},
        {"input_tokens": "3", "output_tokens": 2},
        {"input_tokens": 3, "output_tokens": 2, "cache_read_tokens": 1.5},
        [3, 2],
    ],
)
def test_a_probe_call_reporting_no_measurable_usage_is_refused(usage: object) -> None:
    """A call missing its input or output count, or carrying one that is no count, is refused."""
    with pytest.raises(ReplayError, match="probe reported"):
        replay_module._call_usage(usage)


def test_a_probe_call_keeps_its_counts_and_nothing_else() -> None:
    """The counts a call reports, its cost and anything else left out."""
    equal(
        replay_module._call_usage(
            {"input_tokens": 2, "output_tokens": 8, "cache_read_tokens": 3, "cost_usd": 0.1}
        ),
        {"input_tokens": 2, "output_tokens": 8, "cache_read_tokens": 3},
    )


def _ledger_with(calls: object) -> dict[str, Any]:
    """A ledger of one session and one turn carrying `calls`."""
    return {
        "sessions": [{"session_name": "print-p", "harness_identity": "claude-code"}],
        "turns": [{"session_name": "print-p", "calls": calls}],
    }


@pytest.mark.parametrize(
    "calls",
    [
        {},
        [{}],
        [{"input_tokens": 3}],
        [{"input_tokens": 3, "output_tokens": -2}],
        [{"input_tokens": 3, "output_tokens": 2, "cost_usd": 0.1}],
        [[3, 2]],
    ],
)
def test_a_ledger_turn_whose_calls_are_no_counts_is_refused(calls: object) -> None:
    """The replay reads no figure off a turn whose calls it cannot hold to counts."""
    with pytest.raises(ReplayError, match="no ledger"):
        replay_module._ledger_of(_ledger_with(calls), Path("ledger.json"))


def test_a_ledger_turn_keeps_its_calls_and_a_turn_with_none_reads_as_none() -> None:
    """Counted calls read back as they are; an earlier ledger's turn carries none."""
    counted = [{"input_tokens": 3, "output_tokens": 2, "cache_read_tokens": 9}]
    equal(replay_module._ledger_of(_ledger_with(counted), Path("l"))["turns"][0]["calls"], counted)
    earlier = _ledger_with([])
    del earlier["turns"][0]["calls"]
    equal(replay_module._ledger_of(earlier, Path("l"))["turns"][0].get("calls"), None)


def test_the_global_options_taking_a_value_are_the_programs_own() -> None:
    """What reading a subcommand skips is what `printobserver --help` says takes a value."""
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run([str(program("printobserver")), "--help"], check=True)
    _, _, globals_said = done.stdout.partition("Every command also takes")
    truth(bool(globals_said), describing="the help to name the global options")
    valued = set(re.findall(r"(--[a-z-]+) <", globals_said))
    equal(valued, shell_lines.VALUED_OPTIONS)


#: A case whose replay is periodic observations alone, enough of them to compact between.
COMPACTING_CASE = "jar-top-petg-healthy"


def test_an_idle_compaction_runs_between_checks_and_the_next_scheduled_check_waits_for_it(
    scratch: Path, tmp_path: Path
) -> None:
    """A check leaving the conversation at the threshold is followed by an idle compaction.

    The replay's server runs with the installer's settings; the compaction runs
    between the two checks, serves no replay entry, and the scheduled check that
    comes due while it runs waits for it and then runs, its wait in the record.
    """
    case = read_case(COMPACTING_CASE)
    large = {
        "input_tokens": 3,
        "cache_read_input_tokens": 149_000,
        "cache_creation_input_tokens": 1_000,
        "output_tokens": 200,
    }
    done = mock_replay(
        case,
        scripted_for(case, [{"commands": [CONTEXT], "calls": [large]}, {"commands": [CONTEXT]}]),
        scratch,
        compaction={"before": 150_203, "after": 9_000, "stall_s": 8},
    )
    replayed = done.replayed
    equal(len(done.turns), len(case.moments), describing="a compaction was taken for a turn")
    compactions = replayed.ledger["compactions"]
    equal([compaction["ended"] for compaction in compactions], ["completed"])
    compaction = compactions[0]
    equal(compaction["size_before"], 150_203)
    equal(compaction.get("reported_after"), 9_000)
    turns = replayed.ledger["turns"]
    truth(
        turns[0]["ran_at"] <= compaction["started_at"]
        and compaction["ended_at"] <= turns[1]["started_at"],
        describing="the compaction ran in the gap between the first two checks",
    )
    points = replayed.points
    equal([point.follows_compaction for point in points][:3], [False, True, False])
    second = points[1].wait
    truth(second is not None, describing="the second check's wait is recorded")
    if second is not None:
        waited = second["waited_for_compaction_ms"]
        truth(
            waited is not None and waited > 0,
            describing=f"the scheduled check waited for the compaction: {second}",
        )
        equal(second["delayed"], False)
    equal(replayed.passed, True, describing=replayed.failures())

    run_directory = tmp_path / "run"
    run_directory.mkdir()
    (run_directory / f"{case.name}.json").write_text(
        json.dumps(replayed.as_record()), encoding="utf-8"
    )
    written = record.folded(run_directory, "a-commit")
    equal(written["figures"]["compaction_delayed_checks"], 0.0)
    equal(written["detail"]["idle_compactions"], 1)


def _call(input_tokens: int, output: int, cached: int = 0) -> CallUsage:
    return {"input_tokens": input_tokens, "output_tokens": output, "cache_read_tokens": cached}


def test_the_token_figures_and_the_delayed_checks_are_what_their_budgets_define() -> None:
    """The projections and the delayed count, worked out by hand from scripted checks.

    Base 4,000; quiet checks with no history read 7,000; each quiet check grows the
    conversation by 4,000; an acting check makes 2 calls; a completed compaction leaves
    10,000 and reads 150,000. One cancelled compaction runs beside the acting check's
    event, and one completed compaction beside the last check's, which is the one
    check delayed.
    """
    stamp = "2026-10-31T16:{:02}:00Z".format
    ledger_turns: list[LedgerTurn] = [
        {"session_name": "s", "started_at": stamp(minute), "ran_at": stamp(minute + 1)}
        for minute in (1, 3, 5, 7, 9, 11)
    ]
    compactions: list[LedgerCompaction] = [
        {
            "session_name": "s",
            "started_at": stamp(4),
            "ended_at": stamp(5),
            "ended": "cancelled",
            "size_before": 152_000,
            "usage": {},
        },
        {
            "session_name": "s",
            "started_at": stamp(6),
            "ended_at": stamp(7),
            "ended": "completed",
            "size_before": 152_000,
            "reported_after": 10_000,
            "usage": {"input_tokens": 5, "cache_read_tokens": 149_995},
            "scheduled_check_waited_ms": 2_500,
        },
        {
            "session_name": "s",
            "started_at": stamp(10),
            "ended_at": stamp(11),
            "ended": "completed",
            "size_before": 152_000,
            "reported_after": 10_000,
            "usage": {"input_tokens": 5, "cache_read_tokens": 149_995},
        },
    ]
    scripted: list[tuple[CheckClass, list[CallUsage]]] = [
        ("quiet", [_call(7_000, 1_000)]),
        ("quiet", [_call(3_000, 1_000, cached=8_000)]),
        ("acting", [_call(15_000, 500), _call(16_000, 1_000)]),
        ("quiet", [_call(13_000, 1_000)]),
        ("quiet", [_call(17_000, 1_000)]),
        ("quiet", [_call(21_000, 1_000)]),
    ]
    arrivals = [stamp(1), stamp(3), "2026-10-31T16:04:30Z", stamp(7), stamp(9), stamp(10)]
    points: list[DecisionRecord] = []
    previous: str | None = None
    for number, ((check_class, calls), turn, arrived) in enumerate(
        zip(scripted, ledger_turns, arrivals, strict=True)
    ):
        wait, follows = check_wait(arrived, turn, previous, compactions)
        previous = turn["ran_at"]
        points.append(
            {
                "scenario": f"check-{number}",
                "at": arrived,
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
                "calls": calls,
                "check_class": check_class,
                "wait": wait,
                "follows_compaction": follows,
            }
        )
    equal(
        [point["follows_compaction"] for point in points],
        [False, False, False, True, False, True],
    )
    tests: list[RecordedTest] = [
        {
            "test": "scripted",
            "passed": True,
            "turns": len(points),
            "sessions": ["s"],
            "compound_commands": {},
            "decision_points": points,
            "compactions": compactions,
        }
    ]
    checks = record.checks_of(tests, 4_000)
    # A check after a completed compaction carries in what that compaction left,
    # 10,000, not the call before it: 13,000 - (10,000 - 4,000) and 21,000 - 6,000.
    equal([check["history"] for check in checks][3:], [10_000, 14_000, 10_000])
    equal([check["input"] for check in checks][3:], [7_000, 7_000, 15_000])
    figures, detail = record.compaction_figures(checks, 4_000, compactions, 150_000, 2_500)

    # By hand: the conversation starts at 4,000 and grows 4,000 a check; check c of the
    # first cycle reads 7,000 + 4,000c. It passes 150,000 after the 37th check (index
    # 36), is compacted (150,000) and restarts at 10,000; check j of a later cycle reads
    # 13,000 + 4,000j and the cycle compacts after 35 checks. Acting checks, at indexes
    # 9, 28, 47 and 66 of 76, make a second call at what they read.
    def cycle(first: int, count: int) -> int:
        return count * first + 4_000 * count * (count - 1) // 2

    def reads(index: int) -> int:
        return 7_000 + 4_000 * index if index <= 36 else 13_000 + 4_000 * ((index - 37) % 35)

    typical = (
        cycle(7_000, 37)
        + 150_000
        + cycle(13_000, 35)
        + 150_000
        + cycle(13_000, 4)
        + sum(reads(index) for index in (9, 28, 47, 66))
    )
    equal(figures["tokens_per_print"], float(typical))
    # Split by part: 76 checks, 4 of them making a second call, is 80 calls, each
    # reading the 4,000 base (2,500 of it the harness's) and adding 3,000 to it; two
    # compactions read 150,000 each; the rest is the conversation the calls carried,
    # 4,000 a check grown from the base: sum over calls of reads - 7,000.
    split = detail["tokens_per_print"]["split"]
    equal(split["base"], 80 * 4_000.0)
    equal(split["base_split"], {"harness": 80 * 2_500.0, "printobserver": 80 * 1_500.0})
    equal(split["checks"], 80 * 3_000.0)
    equal(split["compaction"], 2 * 150_000.0)
    carried = sum(reads(index) - 7_000 for index in range(76)) + sum(
        reads(index) - 7_000 for index in (9, 28, 47, 66)
    )
    equal(split["history"], float(carried))
    equal(sum(split[part] for part in ("base", "checks", "history", "compaction")), typical)
    equal(detail["tokens_per_print"]["projected_compactions"], 2)
    long_acting = [((2 * index + 1) * 281) // 30 for index in range(15)]
    later = 281 - 37
    long = (
        cycle(7_000, 37)
        + 150_000
        + (later // 35) * (cycle(13_000, 35) + 150_000)
        + cycle(13_000, later % 35)
        + sum(reads(index) for index in long_acting)
    )
    equal(figures["tokens_per_long_plate"], float(long))
    equal(detail["tokens_per_long_plate"]["projected_compactions"], 1 + later // 35)
    rates = detail["tokens_per_print"]["rates"]
    equal(
        (rates["quiet"], rates["growth"], rates["acting_calls"]),
        (7_000.0, 4_000.0, 2.0),
    )
    equal((rates["after_compaction"], rates["compaction_input"]), (10_000.0, 150_000.0))

    # The acting check's event arrived while the cancelled compaction ran, and the last
    # check's while a compaction ran that then completed: only that one is delayed.
    equal(figures["compaction_delayed_checks"], 1.0)
    delayed = detail["compaction_delayed_checks"]
    equal((delayed["started"], delayed["completed"], delayed["cancelled"]), (3, 2, 1))
    equal(delayed["longest_wait_while_cancelling_ms"], 30_000)
    equal(delayed["longest_scheduled_wait_ms"], 2_500)
