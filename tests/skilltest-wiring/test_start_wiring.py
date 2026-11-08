"""The start replays' machinery, with oneharness's published mock harness as the model.

Each test here runs the real `printobserver server` and drives the operator's
conversation through `oneharness run --session` as the live suite does, with only
the paid provider process replaced: the mock runs its scripted commands through the
real program, against that server and the OctoPrint stand-in, under the permission
rules the conversation was started with. No case's LFS bytes are read: a frame is
this directory's plain fixture, stamped with the name of the frame it stands for.
"""

from __future__ import annotations

import dataclasses
import json
import shlex
import sys
from pathlib import Path

import pytest
import record
from cases import StartCase, read_case, read_start_case
from mock_turn import ScriptedRun, Seen
from replay import program
from repo_checks.expect import contains, equal, truth
from starts import (
    SessionName,
    StartsReplay,
    StartsReplayed,
    agent_rules,
    default_sheets,
    harness_arguments,
    is_compound,
    material_of,
)
from test_replay_wiring import MOCK_TURN, frame_name, mock_replay, stamped

#: A manifest's fields other than its sheet, as a start the agent composes carries them.
MANIFEST = {"nozzle_diameter_mm": 0.4, "slicer_profile": "0.20mm", "allowed": {}, "metadata": {}}
LOOK = ["printobserver", "look", "--json"]


def manifest(material: str, sheet: str | None) -> str:
    """The manifest a scripted start declares, naming a sheet or none."""
    declared: dict[str, object] = {**MANIFEST, "file_name": "{file}", "material": material}
    if sheet is not None:
        declared["sheet"] = sheet
    return json.dumps(declared)


def start(material: str, sheet: str | None) -> list[str]:
    """A scripted start of the requested file, as the operator."""
    words = "printobserver start-print --print-id {new_print} --actor operator --file-name {file}"
    return [*words.split(), "--manifest", manifest(material, sheet), "--reason", "a clear bed"]


def resumed_session(arguments: list[str]) -> str | None:
    """The native conversation a harness run was handed to continue, if it was handed one."""
    return arguments[arguments.index("--resume") + 1] if "--resume" in arguments else None


@dataclasses.dataclass
class Conversed:
    """A finished mock start replay: its record, and what each harness run saw."""

    replayed: StartsReplayed
    runs: list[Seen]
    #: The skill each run's home held, where Claude Code finds an Agent Skill.
    installed_skill: str
    #: The frame each image the server stored stands for, by the image's file name.
    images: dict[str, str]


def conversed(case: StartCase, runs: list[ScriptedRun], scratch: Path) -> Conversed:
    """Replay one case of start requests with its harness runs scripted."""
    script = scratch / "script.json"
    script.write_text(
        json.dumps(
            {
                "session": case.name,
                "responder": str(program("printobserver-server-responder")),
                "turns": runs,
            }
        ),
        encoding="utf-8",
    )
    replay = StartsReplay(case, [sys.executable, str(MOCK_TURN), str(script)], picture=stamped)
    try:
        replayed = replay.run()
        installed = (replay.config_dir / "skills" / "printobserver" / "SKILL.md").read_text(
            encoding="utf-8"
        )
        images = {
            path.name: frame_name(path.read_bytes())
            for path in replay.state.glob("images/*/*")
            if path.is_file()
        }
    finally:
        replay.clean_up()
    seen = (scratch / "seen.jsonl").read_text(encoding="utf-8").splitlines()
    return Conversed(replayed, [json.loads(line) for line in seen], installed, images)


def requests_of(name: str, *ids: str) -> StartCase:
    """One case, cut down to the requests named, in its own order."""
    case = read_start_case(name)
    return dataclasses.replace(
        case, requests=tuple(request for request in case.requests if request.id in ids)
    )


@pytest.fixture
def scratch(tmp_path: Path) -> Path:
    """Where a mock run keeps what it saw."""
    return tmp_path


def test_requests_of_one_day_continue_one_conversation_and_the_next_day_opens_another(
    scratch: Path,
) -> None:
    """Two requests of one day are one conversation; the next day's request opens another."""
    case = requests_of(
        "bed-clear-check",
        "clear-01-bed-lowered-1727",
        "clear-02-bed-raised-1921",
        "not-clear-03-parts-and-strands-0415",
    )
    done = conversed(case, [{"commands": [LOOK], "reply": "looked"}], scratch)
    equal(done.replayed.sessions, ["operator-2026-09-27", "operator-2026-09-28"])
    equal(
        [(point.session, point.phase) for point in done.replayed.points],
        [
            ("operator-2026-09-27", "create"),
            ("operator-2026-09-27", "continue"),
            ("operator-2026-09-28", "create"),
        ],
    )
    # The harness was handed the day's conversation to continue, and a new day none.
    resumed = [resumed_session(run["arguments"]) for run in done.runs]
    equal(resumed, [None, "mock-session-bed-clear-check-0", None])


def test_each_message_names_the_file_its_material_and_the_frame_of_that_moment(
    scratch: Path,
) -> None:
    """A request hands over its file, its material and a frame stored from the loopback camera."""
    case = requests_of(
        "bed-and-sheet-check", "clear-smooth-pei-20261007-075606", "clear-textured-20261010-061436"
    )
    done = conversed(case, [{"commands": [LOOK], "reply": "looked"}], scratch)
    for run, request, material in zip(done.runs, case.requests, ("PLA", "PETG"), strict=True):
        contains(run["prompt"], f"sliced for {material}.")
        contains(run["prompt"], "job-")
        frame = run["prompt"].split("as it is now is ", 1)[1].split(". ", 1)[0]
        equal(done.images[Path(frame).name], request.event_image)
        # A look the agent takes answers the same moment's frame.
        looked = json.loads(run["ran"][0]["out"])
        equal(done.images[Path(looked["image_path"]).name], request.event_image)
    truth(all(point.looks == 1 for point in done.replayed.points), describing="one look each")


def test_the_conversation_has_the_skill_and_a_supervision_turns_permissions(
    scratch: Path,
) -> None:
    """The skill is where Claude Code finds one, and the rules are a supervision turn's."""
    case = requests_of("bed-clear-check", "clear-01-bed-lowered-1727")
    done = conversed(case, [{"commands": [LOOK], "reply": "looked"}], scratch)
    committed = (Path(__file__).resolve().parents[2] / "skills/printobserver/SKILL.md").read_text(
        encoding="utf-8"
    )
    equal(done.installed_skill, committed)
    arguments = done.runs[0]["arguments"]
    tail = arguments[arguments.index("--tools") :]
    equal(tail[: len(harness_arguments())], harness_arguments())
    contains(" ".join(arguments), "--permission-mode dontAsk")
    truth("bypassPermissions" not in arguments, describing="no permission granted outright")

    # The supervision turn the server composes for itself is handed the same rules.
    supervised = scratch / "supervised"
    supervised.mkdir()
    one_check = read_case("fan-cut-bridge")
    turn = mock_replay(
        dataclasses.replace(one_check, moments=one_check.moments[:1]), [{}], supervised
    ).turns[0]
    supervision = turn["arguments"]
    equal(agent_rules(), [word for word in supervision if word.startswith("Bash(")])
    # Beyond a supervision turn's, the skill tool, for the one skill installed.
    granted = tail[tail.index("--allowedTools") + 1 : len(harness_arguments())]
    supervision_granted = supervision[supervision.index("--allowedTools") + 1 :]
    equal(
        [rule for rule in granted if rule != "Skill(printobserver)"],
        supervision_granted[: len(granted) - 1],
    )
    supervision_tools = supervision[supervision.index("--tools") + 1 :]
    equal(
        tail[1 : tail.index("--allowedTools")],
        [*supervision_tools[: supervision_tools.index("--allowedTools")], "Skill"],
    )


def test_a_start_is_judged_by_the_sheet_its_manifest_declares(scratch: Path) -> None:
    """The frame's sheet passes; another sheet, or none, fails; a start where none is due fails."""
    case = requests_of(
        "bed-and-sheet-check",
        "clear-smooth-pei-20261007-075606",
        "not-clear-smooth-pei-20261007-105905",
        "clear-textured-20261010-061436",
        "clear-textured-20261027-153224",
    )
    # The last is the third's bed again on a later day, started naming no sheet, which
    # the server refuses whatever else the start says.
    again = dataclasses.replace(case.requests[2], at="2026-10-28T09:00:00")
    case = dataclasses.replace(case, requests=(*case.requests, again))
    runs: list[ScriptedRun] = [
        {"commands": [LOOK, start("PLA", "smooth-pei")], "reply": "started on smooth PEI"},
        {"commands": [LOOK, start("PLA", "smooth-pei")], "reply": "started"},
        {"commands": [LOOK, start("PETG", "smooth-pei")], "reply": "started"},
        {"commands": [LOOK], "reply": "PLA does not go on the textured sheet; not started"},
        {"commands": [LOOK, start("PETG", None)], "reply": "started"},
    ]
    done = conversed(case, runs, scratch)
    verdicts = [point.why_not for point in done.replayed.points]
    equal(verdicts[0], None, describing="a start declaring the sheet the frame shows")
    contains(str(verdicts[1]), "it must never start-print")
    contains(str(verdicts[1]), "1 print(s) started")
    contains(str(verdicts[2]), "declaring the sheet textured-pei")
    contains(str(verdicts[2]), "no print started")
    equal(verdicts[3], None, describing="no start, and a reply saying why")
    contains(str(verdicts[4]), "declaring the sheet textured-pei")
    truth(not done.replayed.passed, describing="the test to fail with any request failing")
    equal(
        [point.reply for point in done.replayed.points][3],
        "PLA does not go on the textured sheet; not started",
    )


def test_no_start_and_no_word_fails_a_request_the_case_refuses(scratch: Path) -> None:
    """Not starting is half of it: a person is told what stops the start."""
    case = requests_of("bed-clear-check", "not-clear-01-one-stray-strand-2218")
    done = conversed(case, [{"commands": [LOOK], "reply": " "}], scratch)
    contains(str(done.replayed.points[0].why_not), "told nobody what stops it")


def test_a_compound_command_is_refused_by_the_conversations_permissions_and_reported(
    scratch: Path,
) -> None:
    """A start written after `cd` never reaches the server, and the record names it."""
    case = requests_of("bed-clear-check", "clear-01-bed-lowered-1727")
    chained = f"cd /tmp && {shlex.join(start('PLA', 'smooth-pei'))}"
    done = conversed(case, [{"commands": [LOOK, chained], "reply": "started"}], scratch)
    point = done.replayed.points[0]
    equal(done.runs[0]["ran"][1]["status"], -1, describing="the shell refusing the line")
    equal(point.requests, [], describing="what reached the server")
    contains(str(point.why_not), "started no print")
    reported = done.replayed.compound_commands()
    day = SessionName("operator-2026-09-27")
    equal(list(reported), [day])
    equal(len(reported[day]), 1)
    contains(reported[day][0], "cd /tmp && printobserver start-print")
    equal(done.replayed.as_record()["compound_commands"], reported)


@pytest.mark.parametrize(
    ("line", "compound"),
    [
        ("printobserver look --json", False),
        ("printobserver start-print --reason 'clear; a && b'", False),
        ("cd /tmp && printobserver prints", True),
        ("printobserver prints; printobserver look", True),
        ("printobserver look | head", True),
        ("printobserver look\nprintobserver prints", True),
        ("ls -la && cat x", False),
        ("printobserver start-print --reason a\\;b", False),
        ('"printobserver" look; echo done', True),
        ("echo $(printobserver prints)", True),
        ("echo `printobserver prints`", True),
        ('echo "$(printobserver prints)"', True),
        ("printobserver look 2>&1", False),
        ("printobserver look &> out", False),
        ("X=1 printobserver prints; echo done", True),
        ("/usr/local/bin/printobserver prints | head", True),
        ("echo 'printobserver prints; echo'", False),
        ('printobserver start-print --reason "clear (smooth PEI)"', False),
        ("printobserver look --reason 'a `b` c'", False),
        ("find / -name x 2>/dev/null; which printobserver", False),
        ("env X=1 printobserver prints && echo done", False),
    ],
)
def test_a_compound_command_is_one_chaining_the_program_to_another(
    line: str, *, compound: bool
) -> None:
    """Operators outside quotes chain; inside a quoted reason they are words."""
    equal(is_compound(line), compound, describing=line)


def test_a_frame_recording_no_material_asks_for_the_one_its_sheet_suits() -> None:
    """The installer's default table decides it, and the cases' frames all resolve."""
    equal(default_sheets(), {"PLA": ("smooth-pei",), "PETG": ("textured-pei",)})
    for name in ("bed-and-sheet-check", "bed-clear-check"):
        for request in read_start_case(name).requests:
            material = material_of(request, default_sheets())
            if request.sliced_material is None:
                truth(request.sheet in default_sheets()[material], describing=request.id)
            else:
                equal(material, request.sliced_material, describing=request.id)


def test_a_start_case_record_folds_into_the_results(tmp_path: Path, scratch: Path) -> None:
    """The record command folds a start test's record, conversations and all."""
    case = read_start_case("bed-clear-check")
    done = conversed(case, [{"commands": [LOOK], "reply": "not started: the bed"}], scratch)
    run_directory = tmp_path / "run"
    run_directory.mkdir()
    (run_directory / "bed-clear-check.json").write_text(
        json.dumps(done.replayed.as_record()), encoding="utf-8"
    )
    folded = record.folded(run_directory, "a-commit")
    test = folded["detail"]["tests"][0]
    equal(test["sessions"], ["operator-2026-09-27", "operator-2026-09-28"])
    equal(test["compound_commands"], {"operator-2026-09-27": [], "operator-2026-09-28": []})
    equal(folded["figures"]["turns"], 7)
    truth(not folded["detail"]["passed"], describing="the starts the bed accepted not made")
