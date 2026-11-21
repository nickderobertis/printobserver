"""The outside supervisor's watch lines, with oneharness's published mock harness as the model.

Each replay here runs the real `printobserver server` over a healthy case's real
G-code — the plain copy a crate's tests commit, byte for byte the case's LFS object —
with printobserver's turns and the outside supervisor's conversation both answered
by the mock at the harness's process boundary, every command it runs going through
the real program to that server. So the layer each watch line stands in is the one
the server's own G-code index reads there, and each line is judged from the
server's record of what the supervisor did.
"""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any

import pytest
import record
from cases import REPO, Case, Milestone, Moment, read_case
from mock_turn import ScriptedRun, Seen
from outside import SESSION_PREFIX
from replay import OutsidePoint
from repo_checks.checks_real_prints import pointer, printed_gcode
from repo_checks.expect import contains, equal, truth
from test_replay_wiring import PRINTER_LOOK, mock_replay

#: The plain copy of each healthy case's G-code a crate's tests commit.
PLAIN_GCODE = {
    "bubbler-guard-and-pad-pla-healthy": REPO
    / "crates/printobserver-server/tests/fixtures/real-prints/bubbler-guard-and-pad.gcode",
    "jar-top-petg-healthy": REPO
    / "crates/printobserver-core/tests/fixtures/real-prints/jar-top-petg.gcode",
}
#: The extrusion types the slicer lays support as.
SUPPORT_TYPES = ("Support material", "Support material interface")
OVERHANG = "Overhang perimeter"
CONTEXT = ["printobserver", "context", "--print-id", "{print}", "--json"]
PRINT_LOOK = ["printobserver", "look", "--print-id", "{print}", "--json"]
#: An outside supervisor's run that does what its line asks, reading the context first.
LOOKS_AND_OPENS: ScriptedRun = {"commands": [CONTEXT, PRINTER_LOOK], "reads": ["{looked}"]}
#: Printobserver's own quiet check.
QUIET: ScriptedRun = {"commands": [CONTEXT]}
#: The words a watch line opens with, which tell its run from a check's.
WATCH_LINE = "You started the print "
#: A real camera frame at the camera's own size, which a look shows scaled: a plain
#: copy the panels crate's tests commit.
FULL_SIZE_FRAME = (
    REPO
    / "crates/printobserver-panels/tests/fixtures/frames/jar-top-petg-healthy"
    / "agent-001-obs-120024-p00.0.jpg"
).read_bytes()
#: Where the server writes the images it shows, beside the frames it stores.
SHOWN = "check-images"


def full_size(name: str) -> bytes:
    """The full-size frame, stamped with the name of the frame it stands for.

    The stamp is a comment segment after the frame's own leading ones, so it is the
    last a reader of them finds, as `frame_name` reads a stamp.
    """
    at = 2
    while FULL_SIZE_FRAME[at : at + 2] == b"\xff\xfe":
        at += 2 + int.from_bytes(FULL_SIZE_FRAME[at + 2 : at + 4], "big")
    comment = name.encode()
    segment = b"\xff\xfe" + (len(comment) + 2).to_bytes(2, "big") + comment
    return FULL_SIZE_FRAME[:at] + segment + FULL_SIZE_FRAME[at:]


@dataclass(frozen=True)
class Layer:
    """One layer of a slice: where it starts and ends, its height, and what it lays."""

    number: int
    start: int
    end: int
    types: tuple[str, ...]
    z_mm: float | None = None


def layers_of(gcode: bytes) -> list[Layer]:
    """Every layer of a slice, read apart from the server's own index.

    A layer opens at its `;LAYER_CHANGE` line, counting the first as 1, stands at the
    height its `;Z:` line names, and lays material as each `;TYPE:` under which one
    of its moves feeds filament while it travels.
    """
    found: list[dict[str, Any]] = []
    offset, kind, relative, last_e = 0, b"", True, 0.0
    for line in gcode.splitlines(keepends=True):
        text = line.split(b";", 1)[0].strip()
        if line.startswith(b";LAYER_CHANGE"):
            found.append({"start": offset, "types": [], "z_mm": None})
        elif line.startswith(b";Z:") and found:
            found[-1]["z_mm"] = float(line[len(b";Z:") :])
        elif line.startswith(b";TYPE:"):
            kind = line[len(b";TYPE:") :].strip()
        elif text in {b"M83", b"M82"}:
            relative = text == b"M83"
        elif text.startswith(b"G92"):
            given = re.search(rb"\bE(-?[\d.]+)", text)
            last_e = float(given.group(1)) if given else last_e
        elif text[:3] in {b"G1 ", b"G2 ", b"G3 "}:
            given = re.search(rb"\bE(-?[\d.]+)", text)
            travels = re.search(rb"\b[XY]-?[\d.]", text) is not None
            if given and found and kind and travels:
                value = float(given.group(1))
                extruded = value if relative else value - last_e
                last_e = last_e if relative else value
                if extruded > 0 and kind.decode() not in found[-1]["types"]:
                    found[-1]["types"].append(kind.decode())
        offset += len(line)
    return [
        Layer(
            number=number,
            start=layer["start"],
            end=found[number]["start"] if number < len(found) else len(gcode),
            types=tuple(layer["types"]),
            z_mm=layer["z_mm"],
        )
        for number, layer in enumerate(found, start=1)
    ]


def milestone_layer(layers: list[Layer], milestone: Milestone) -> Layer:
    """The layer a milestone names, by the rule the case schema states for it."""
    match milestone:
        case Milestone.FIRST_LAYER:
            return next(layer for layer in layers if set(layer.types) - set(SUPPORT_TYPES))
        case Milestone.FIRST_OVERHANG:
            return next(layer for layer in layers if OVERHANG in layer.types)
        case Milestone.FIRST_SUPPORT:
            return next(layer for layer in layers if set(layer.types) & set(SUPPORT_TYPES))
        case Milestone.END:
            return layers[-1]


def recorded_layer(case: Case, frame: str, layers: list[Layer]) -> Layer:
    """The layer a frame shows printing, by the height the case recorded for it."""
    document = json.loads((case.directory / "case.json").read_text("utf-8"))
    height = next(raw["z_mm"] for raw in document["frames"] if raw["file"] == frame)
    return next(layer for layer in reversed(layers) if layer.z_mm == height)


def frame_owed(case: Case, moment: Moment, layers: list[Layer]) -> str | None:
    """Why a watch line's frame is not the one its milestone owes, or None when it is.

    A milestone owes the case's first frame taken at or after the line's position, so
    the supervisor's own look shows the layer it reports and never one before it. One
    exception stands, by manager ruling a28, until a healthy print is recorded with
    a frame of its last layer: an `end` line over a recording that ends before that
    layer is handed the recording's last frame, and names how many layers before the
    last layer that frame was taken. Any earlier frame, a gap that is not that frame's,
    or a gap named where a frame at or after the milestone exists, is refused.
    """
    times = case.frame_times()
    positions = [case.position_at(when) for when in times]
    at_or_after = next(
        (
            frame.file
            for frame, position in zip(case.frames, positions, strict=True)
            if position >= moment.file_position_bytes
        ),
        None,
    )
    if at_or_after is not None:
        if moment.event_image != at_or_after:
            return f"handed {moment.event_image}, not {at_or_after}, the first frame at or after it"
        if moment.frame_layers_before_milestone is not None:
            return "names a layer gap though a frame at or after its milestone exists"
        return None
    last = case.frames[-1].file
    if moment.milestone is not Milestone.END:
        return f"handed a frame before its milestone, which only an end line may be: {last}"
    if moment.event_image != last:
        return f"handed {moment.event_image}, earlier than {last}, the recording's last frame"
    gap = layers[-1].number - recorded_layer(case, last, layers).number
    if moment.frame_layers_before_milestone != gap:
        return (
            f"names a gap of {moment.frame_layers_before_milestone} layers where {last} "
            f"was taken {gap} before the last layer"
        )
    return None


def plain_gcode(case: Case) -> bytes:
    """The case's G-code, from the plain copy whose digest is the case's own.

    The case's file is the LFS pointer a checkout leaves, naming its object's
    digest, or that object itself once the real-print suite has fetched it.
    """
    gcode = printed_gcode(case.directory)
    if gcode is None:
        pytest.fail(f"{case.name} carries no G-code")
    held = gcode.read_bytes()
    stands_for = pointer(held[:2048])
    digest = stands_for.oid if stands_for is not None else hashlib.sha256(held).hexdigest()
    plain = PLAIN_GCODE[case.name].read_bytes()
    equal(
        hashlib.sha256(plain).hexdigest(),
        digest,
        describing=f"{PLAIN_GCODE[case.name].name} to be {case.name}'s G-code byte for byte",
    )
    return plain


def around_the_lines(case: Case) -> Case:
    """The case, its replay cut to each watch line and printobserver's turn before it."""
    kept: list[str] = []
    for index, moment in enumerate(case.moments):
        if not moment.is_outside:
            continue
        before = case.moments[index - 1]
        if not before.is_outside and before.id not in kept:
            kept.append(before.id)
        kept.append(moment.id)
    return read_case(case.name, replay=tuple(kept))


def scripted(case: Case, lines: list[ScriptedRun]) -> list[ScriptedRun]:
    """Each harness run of the replay in order: a quiet check per turn, `lines` per line."""
    runs: list[ScriptedRun] = []
    told = 0
    for moment in case.moments:
        if moment.is_outside:
            runs.append(lines[told])
            told += 1
        else:
            runs.append(QUIET)
    return runs


def watch_runs(turns: list[Seen]) -> list[Seen]:
    """The harness runs a watch line was handed to, in order."""
    return [turn for turn in turns if turn["prompt"].startswith(WATCH_LINE)]


def resumed(seen: Seen) -> str | None:
    """The native conversation a run was handed to continue, if it was handed one."""
    arguments = seen["arguments"]
    return arguments[arguments.index("--resume") + 1] if "--resume" in arguments else None


@pytest.fixture
def scratch(tmp_path: Path) -> Path:
    """Where a mock run keeps what it saw."""
    return tmp_path


@pytest.mark.parametrize("name", sorted(PLAIN_GCODE))
def test_each_watch_line_stands_in_its_milestones_layer_as_the_servers_index_reads_it(
    name: str, scratch: Path
) -> None:
    """The context the supervisor reads at each line is its milestone's layer, on that frame.

    Its position is where the job stands as the line is handed; the server's index,
    read through the real `context`, names the layer there, which is the milestone's
    by a reading of the slice apart from that index. Its frame is the one
    `frame_owed` holds it to, and that is the frame the supervisor's own look is
    answered: at the camera's own size, so the look shows a scaled copy, which is
    what the supervisor opens.
    """
    case = read_case(name)
    gcode = plain_gcode(case)
    layers = layers_of(gcode)
    cut = around_the_lines(case)
    lines = [moment for moment in cut.moments if moment.is_outside]
    done = mock_replay(
        cut,
        scripted(cut, [LOOKS_AND_OPENS] * len(lines)),
        scratch,
        gcode=gcode,
        picture=full_size,
    )
    runs = watch_runs(done.turns)
    equal(len(runs), len(lines), describing="one run per watch line")
    for moment, seen in zip(lines, runs, strict=True):
        if moment.milestone is None:
            pytest.fail(f"{moment.id} names no milestone")
        layer = milestone_layer(layers, moment.milestone)
        truth(
            layer.start <= moment.file_position_bytes < layer.end,
            describing=f"{moment.id} at {moment.file_position_bytes} inside layer {layer}",
        )
        slicer = json.loads(seen["ran"][0]["out"])["context"]["slicer"]
        equal(slicer["layer"], layer.number, describing=f"the index's layer at {moment.id}")
        equal(set(slicer["types"]), set(layer.types), describing=f"what {moment.id}'s layer lays")
        if moment.milestone is Milestone.END:
            equal(slicer["z_mm"], slicer["final_z_mm"], describing="the last layer's height")
        equal(frame_owed(case, moment, layers), None, describing=f"{moment.id}'s frame")
        looked = json.loads(seen["ran"][1]["out"])
        truth(
            looked["event"].get("print_id") is None,
            describing=f"{moment.id}'s look to belong to no print",
        )
        equal(
            done.images.get(looked["frame"]["sha256"]),
            moment.event_image,
            describing=f"the frame {moment.id}'s look was answered",
        )
        equal(
            Path(looked["image_path"]).parent.name,
            SHOWN,
            describing=f"{moment.id}'s look to show a scaled copy of the full-size frame",
        )
    equal(
        [point.why_not for point in done.replayed.outside],
        [None] * len(lines),
        describing=done.replayed.failures(),
    )


@pytest.mark.parametrize("name", sorted(PLAIN_GCODE))
def test_a_watch_lines_frame_is_never_one_from_before_its_milestone(name: str) -> None:
    """Every frame a line is handed is at or after its milestone, but for the one exception.

    The committed lines hold to it; each healthy case's end line is that exception,
    over a recording that ends before its last layer, and is refused once handed any
    earlier frame or a gap that is not its frame's. A line before the end handed a
    frame from before its milestone is refused, and so is a gap named on it.
    """
    case = read_case(name)
    layers = layers_of(plain_gcode(case))
    lines = {moment.milestone: moment for moment in case.moments if moment.is_outside}
    for moment in lines.values():
        equal(frame_owed(case, moment, layers), None, describing=f"{moment.id}'s frame")
    end = lines[Milestone.END]
    truth(
        end.frame_layers_before_milestone is not None,
        describing=f"{name}'s end line to name how far before the last layer its frame is",
    )
    equal(end.event_image, case.frames[-1].file, describing=f"{end.id} to use the last frame")
    earlier = replace(end, event_image=case.frames[-2].file)
    contains(frame_owed(case, earlier, layers) or "", "earlier than")
    wrong_gap = replace(
        end, frame_layers_before_milestone=(end.frame_layers_before_milestone or 0) + 1
    )
    contains(frame_owed(case, wrong_gap, layers) or "", "names a gap of")
    first = lines[Milestone.FIRST_OVERHANG]
    files = [frame.file for frame in case.frames]
    before = files[files.index(first.event_image) - 1]
    contains(
        frame_owed(case, replace(first, event_image=before), layers) or "",
        "the first frame at or after it",
    )
    contains(
        frame_owed(case, replace(first, frame_layers_before_milestone=1), layers) or "",
        "names a layer gap",
    )
    beyond = replace(first, file_position_bytes=end.file_position_bytes)
    contains(frame_owed(case, beyond, layers) or "", "only an end line may be")


def _bubbler_lines(scratch: Path) -> tuple[list[OutsidePoint], list[Seen], dict[str, Any]]:
    """The bubbler's watch lines, each run scripted one way, and the record they fold into."""
    case = around_the_lines(read_case("bubbler-guard-and-pad-pla-healthy"))
    lines: list[ScriptedRun] = [
        LOOKS_AND_OPENS,
        # Reads the context and never looks.
        {"commands": [CONTEXT]},
        # Looks, and never opens what the look named.
        {"commands": [PRINTER_LOOK]},
        # Looks with the print named, and opens that.
        {"commands": [PRINT_LOOK], "reads": ["{looked}"]},
    ]
    done = mock_replay(case, scripted(case, lines), scratch)
    run_directory = scratch / "run"
    run_directory.mkdir()
    path = run_directory / f"{case.name}.json"
    path.write_text(json.dumps(done.replayed.as_record()), encoding="utf-8")
    return done.replayed.outside, watch_runs(done.turns), json.loads(path.read_text("utf-8"))


def test_the_watch_lines_of_one_replay_continue_one_conversation_and_each_is_judged(
    scratch: Path,
) -> None:
    """Four lines, one conversation; skipping the look, its frame, or naming the print fails.

    The first line opens the outside supervisor's conversation and every later one
    continues it, handed the native conversation the first opened. A line passes
    only when the supervisor looked at the printer with no print named and opened
    that frame; a line it only read the context for, one whose frame it never opened
    and one it looked at with the print named each fail, naming why.
    """
    points, runs, written = _bubbler_lines(scratch)
    equal(
        [point.moment.milestone for point in points],
        [
            Milestone.FIRST_LAYER,
            Milestone.FIRST_SUPPORT,
            Milestone.FIRST_OVERHANG,
            Milestone.END,
        ],
    )
    equal([point.phase for point in points], ["create", "continue", "continue", "continue"])
    equal({point.session for point in points}, {f"{SESSION_PREFIX}{written['test']}"})
    opened = resumed(runs[1])
    truth(opened is not None, describing="the second line to continue a conversation")
    equal([resumed(run) for run in runs], [None, opened, opened, opened])
    passed, unlooked, unopened, named = (point.why_not for point in points)
    equal(passed, None)
    contains(unlooked or "", "took no look of its own at the printer")
    contains(unopened or "", "opened none of the frames its looks named")
    contains(named or "", "looked with the print named")
    truth(
        "took no look of its own" in (named or ""),
        describing="a look naming the print to be no look of the supervisor's own",
    )
    contains(runs[0]["prompt"], "printobserver's agent's latest verdict: still clean")


def test_the_watch_lines_fold_into_the_record_beside_the_prints_checks(scratch: Path) -> None:
    """Each line is a decision point of its test, in the case's order, under its own session."""
    points, _, written = _bubbler_lines(scratch)
    case = around_the_lines(read_case("bubbler-guard-and-pad-pla-healthy"))
    equal(
        [point["scenario"] for point in written["decision_points"]],
        [moment.id for moment in case.moments],
    )
    lines = [
        point for point in written["decision_points"] if point["trigger"] != "periodic_observation"
    ]
    equal([point["trigger"] for point in lines], ["outside_supervision"] * len(points))
    equal([point["passed"] for point in lines], [True, False, False, False])
    contains(written["sessions"], f"{SESSION_PREFIX}{written['test']}")
    equal(written["turns"], len(written["decision_points"]))
    equal(written["passed"], False)
    run_directory = scratch / "run"
    folded = record._test_record(run_directory / f"{written['test']}.json")
    equal(len(folded["decision_points"]), len(case.moments))
