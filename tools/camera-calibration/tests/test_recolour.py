"""The recolouring a colour-change case's `recolour-*` frames were made by, run as committed.

Each `recolour-*` frame under `tests/real-prints/` is a Git LFS object, and so is
the frame it was made from, and no gate test reads a case's LFS bytes. So the
reproduction runs where the gate has the bytes: over the plain copy of the source
frame the repository commits for tests, under
`crates/printobserver-panels/tests/fixtures/frames/`, first held to be the case's
own frame by its sha256, which is the object id the case's pointer names. The
output is then held, byte for byte, to the committed frame the same way: its
sha256 against the pointer's object id, or against the frame itself where the
cases were fetched. This table is the recipe each frame was made by.
"""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path

import cv2
import numpy as np
import pytest
from camera_calibration import recolour

TOOL = Path(__file__).resolve().parents[1]
ROOT = TOOL.parents[1]
CASES = ROOT / "tests" / "real-prints"
CALIBRATION = CASES / "cameras" / "core-one-2026-09-27.json"
FRAMES = ROOT / "crates" / "printobserver-panels" / "tests" / "fixtures" / "frames"
#: The plain copy of the bubbler print's slice. Planting a colour change moves no wall,
#: so the walls the derived cases' frames are recoloured over are this slice's.
BUBBLER_SLICE = (
    ROOT
    / "crates"
    / "printobserver-server"
    / "tests"
    / "fixtures"
    / "real-prints"
    / "bubbler-guard-and-pad.gcode"
)
#: How a recoloured frame's label names its source, as `checks_real_prints` reads it.
RECOLOURED_FROM = re.compile(r"recoloured afterwards from (?P<source>[^\s,;]+)")
POINTER_OID = re.compile(rb"\noid sha256:(?P<oid>[0-9a-f]{64})\n")


@dataclass(frozen=True)
class Recoloured:
    """One committed `recolour-*` frame, and the arguments it was made with."""

    case: str
    frame: str
    #: The plain copy of the camera frame it was made from.
    plain_source: Path
    from_z: str
    to_z: str
    color: str
    #: The toolhead's silhouette over the walls, left as it was.
    leave: tuple[str, ...]


RECOLOURED = (
    Recoloured(
        case="bubbler-colour-change-planned",
        frame="recolour-frame-180412-p65.7.jpg",
        plain_source=FRAMES
        / "bubbler-guard-and-pad-pla-healthy"
        / "agent-048-obs-180412-p65.7.jpg",
        from_z="50",
        to_z="58.8",
        color="#FF0000",
        leave=("875,60 1100,60 1100,188 1008,190 1005,150 905,165 875,165",),
    ),
    Recoloured(
        case="bubbler-colour-change-off-plan",
        frame="recolour-frame-170731-p13.8.jpg",
        plain_source=FRAMES
        / "bubbler-guard-and-pad-pla-healthy"
        / "agent-016-obs-170731-p13.8.jpg",
        from_z="6",
        to_z="10.8",
        color="#FF0000",
        leave=("925,60 1110,60 1110,135 1080,150 1050,178 1000,178 925,175",),
    ),
)


def equal(actual: object, expected: object, *, describing: str) -> None:
    """Fail unless two values are equal."""
    if actual != expected:
        message = f"{describing}: {actual!r} is not {expected!r}"
        raise AssertionError(message)


def contains(whole: str, part: str, *, describing: str) -> None:
    """Fail unless text carries a part."""
    if part not in whole:
        message = f"{describing} does not say {part!r}: {whole!r}"
        raise AssertionError(message)


def stands_for(path: Path) -> str:
    """The sha256 of the file a case's path holds: its pointer's object id, or its own bytes'."""
    data = path.read_bytes()
    named = POINTER_OID.search(data) if data.startswith(b"version https://git-lfs") else None
    return named["oid"].decode() if named else hashlib.sha256(data).hexdigest()


def arguments(made: Recoloured, out: Path, frame: Path | None = None) -> list[str]:
    """The command line one frame was made by, writing to `out`."""
    return [
        "--frame",
        str(frame or made.plain_source),
        "--calibration",
        str(CALIBRATION),
        "--gcode",
        str(BUBBLER_SLICE),
        "--from-z",
        made.from_z,
        "--to-z",
        made.to_z,
        "--color",
        made.color,
        *(word for polygon in made.leave for word in ("--leave", polygon)),
        "--out",
        str(out),
    ]


@pytest.mark.parametrize("made", RECOLOURED, ids=lambda made: made.case)
def test_each_recoloured_frame_is_its_source_recoloured_byte_for_byte(
    made: Recoloured, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The case's label names the frame it was made from, and the command remakes it exactly."""
    document = json.loads((CASES / made.case / "case.json").read_text(encoding="utf-8"))
    label = next(frame["label"] for frame in document["frames"] if frame["file"] == made.frame)
    named = RECOLOURED_FROM.search(label)
    source = named["source"] if named else None
    if source is None:
        message = f"{made.case}'s {made.frame} names no frame it was made from: {label!r}"
        raise AssertionError(message)
    contains(label, f"Z {made.from_z} to the frame's Z {made.to_z}", describing=label)
    contains(label, made.color, describing=label)
    equal(
        hashlib.sha256(made.plain_source.read_bytes()).hexdigest(),
        stands_for(CASES / made.case / source),
        describing=f"the plain copy {made.plain_source.name} as {made.case}'s {source}",
    )
    out = tmp_path / made.frame
    code = recolour.main(arguments(made, out))
    said = capsys.readouterr()
    equal(code, 0, describing=said.err)
    contains(said.out, f"wrote {out}", describing="what the command said")
    equal(
        hashlib.sha256(out.read_bytes()).hexdigest(),
        stands_for(CASES / made.case / made.frame),
        describing=f"the remade {made.frame} as the committed one",
    )


def redness(path: Path) -> np.ndarray:
    """How much redder than its other two channels each pixel of a JPEG is."""
    picture = cv2.imread(str(path))
    if picture is None:
        message = f"{path} is no image OpenCV reads"
        raise AssertionError(message)
    channels = picture.astype(int)
    return channels[..., 2] - channels[..., :2].max(axis=-1)


def test_the_new_colour_is_where_the_walls_are_and_the_toolhead_is_left(tmp_path: Path) -> None:
    """Inside the walls' band the frame turns red; under a `--leave` polygon it stays as it was.

    The heater block stands in front of the rim's back, so the walls' band covers
    it: recoloured without the polygon, and left as it was with it.
    """
    made = RECOLOURED[0]
    kept, painted = tmp_path / "kept.jpg", tmp_path / "painted.jpg"
    equal(recolour.main(arguments(made, kept)), 0, describing="the frame with its polygon")
    unbounded = arguments(made, painted)
    del unbounded[unbounded.index("--leave") : unbounded.index("--leave") + 2]
    equal(recolour.main(unbounded), 0, describing="the frame without it")
    before, after, over = redness(made.plain_source), redness(kept), redness(painted)
    rim, block = (slice(270, 280), slice(950, 960)), (slice(165, 175), slice(1015, 1025))
    equal(bool(np.median(after[rim]) > 60), True, describing="the rim's front turned red")
    equal(bool(np.median(over[block]) > 60), True, describing="the block under the walls' band")
    equal(
        bool(np.median(after[block]) < np.median(before[block]) + 20),
        True,
        describing="the heater block, left as it was",
    )


@pytest.mark.parametrize(
    ("change", "why"),
    [
        ({"--from-z": "60"}, "is above --to-z"),
        ({"--color": "#ff0000"}, "upper case"),
        ({"--color": "red"}, "#RRGGBB"),
        ({"--to-z": "-1"}, "not a height"),
        ({"--leave": "1,2 3,4"}, "three or more"),
        ({"--leave": "a,b c,d e,f"}, "three or more"),
        ({"--out": "out.png"}, "is not a .jpg path"),
        ({"--from-z": "150", "--to-z": "151"}, "nothing would change"),
        ({"--calibration": "slice"}, "no calibration file"),
        ({"--frame": "slice"}, "no image OpenCV reads"),
        ({"--frame": "small"}, "the calibration was fitted to 1920x1080"),
        ({"--gcode": "missing"}, "missing.gcode"),
    ],
)
def test_arguments_and_files_it_cannot_use_are_refused(
    change: dict[str, str], why: str, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Exit 2, naming what and why, and nothing written."""
    small = tmp_path / "small.jpg"
    cv2.imwrite(str(small), np.zeros((720, 1280, 3), np.uint8))
    stand_ins = {
        "slice": str(BUBBLER_SLICE),
        "small": str(small),
        "missing": str(tmp_path / "missing.gcode"),
        "out.png": str(tmp_path / "out.png"),
    }
    argv = arguments(RECOLOURED[0], tmp_path / "out.jpg")
    for option, value in change.items():
        argv[argv.index(option) + 1] = stand_ins.get(value, value)
    try:
        code = recolour.main(argv)
    except SystemExit as exited:
        code = exited.code
    equal(code, 2, describing=f"the exit for {change}")
    contains(capsys.readouterr().err, why, describing=f"the refusal of {change}")
    equal(list(tmp_path.glob("out.*")), [], describing="what was written")
