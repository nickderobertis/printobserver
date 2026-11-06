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
import shutil
import sys
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import classify
import pytest
import record
from cases import CASES, REPO, Case, read_case, replayed_case_names
from replay import Replay, Replayed, program, service_config
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
    turns: list[dict[str, Any]]
    images: dict[str, str]

    def seen_frame(self, turn: int, look: int) -> str:
        """The frame the `look`th scripted look of one turn answered."""
        ran = [entry for entry in self.turns[turn]["ran"] if entry["words"][1] == "look"]
        return self.images[json.loads(ran[look]["out"])["image_path"]]


def mock_replay(case: Case, turns: list[dict[str, Any]], scratch: Path) -> Scripted:
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
    replay = Replay(case, [sys.executable, str(MOCK_TURN), str(script)], picture=stamped)
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
    turn = {"commands": [LOOK, LOOK, LOOK]}
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
