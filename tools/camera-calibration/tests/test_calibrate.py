"""The calibration tool, driven as an operator runs it, over plain fixtures.

The committed marks (`tests/real-prints/cameras/core-one-2026-09-27.marks.json`)
name case frames and slices under `tests/real-prints/`, which are Git LFS
objects no test reads. Each test rewrites those paths onto the plain copies the
repository commits for tests — the frames under
`crates/printobserver-panels/tests/fixtures/frames/` and the slices under
`crates/printobserver-core/tests/fixtures/real-prints/` — and runs the real
`python -m camera_calibration` command on the result.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any

import cv2
import jsonschema
import numpy as np
import pytest
from camera_calibration import coverage, gcode, plate
from camera_calibration import fit as fitting
from camera_calibration.marks import load
from camera_calibration.model import Camera
from onebudgetspec_sdk import list_budgets

TOOL = Path(__file__).resolve().parents[1]
ROOT = TOOL.parents[1]
CAMERAS = ROOT / "tests" / "real-prints" / "cameras"
COMMITTED = CAMERAS / "core-one-2026-09-27.json"
MARKS = CAMERAS / "core-one-2026-09-27.marks.json"
FRAMES = ROOT / "crates" / "printobserver-panels" / "tests" / "fixtures" / "frames"
SLICES = ROOT / "crates" / "printobserver-core" / "tests" / "fixtures" / "real-prints"
SCHEMA = ROOT / "schemas" / "printobserver-panels" / "camera-calibration.json"
SERVICE_CONFIG = ROOT / "tests" / "real-prints" / "service-config.toml"

# The plain copy of each case's slice.
SLICE_OF = {
    "jar-top-wrong-material-first-layer-detach": "jar-top-first-layer-detach.gcode",
    "jar-top-wrong-material-detach-spaghetti": "jar-top-brim.gcode",
    "jar-top-petg-healthy": "jar-top-petg.gcode",
    "bubbler-guard-and-pad-pla-healthy": (
        "../../../../printobserver-server/tests/fixtures/real-prints/bubbler-guard-and-pad.gcode"
    ),
}


class expect:  # noqa: N801 - read as `expect.equal(...)`, the vocabulary of this repository's own suites
    """A few assertions raising `AssertionError` directly, as `assert` would under `-O` not."""

    @staticmethod
    def truth(condition: object, *, describing: str) -> None:
        """Fail unless a condition holds."""
        if not condition:
            raise AssertionError(describing)

    @staticmethod
    def equal(actual: object, expected: object, *, describing: str) -> None:
        """Fail unless two values are equal."""
        if actual != expected:
            message = f"{describing}: {actual!r} is not {expected!r}"
            raise AssertionError(message)

    @staticmethod
    def contains(whole: str, part: str, *, describing: str) -> None:
        """Fail unless text carries a part."""
        if part not in whole:
            message = f"{describing} does not say {part!r}: {whole!r}"
            raise AssertionError(message)

    @staticmethod
    def passing(result: subprocess.CompletedProcess[str], *, describing: str) -> None:
        """Fail unless a run exited zero."""
        if result.returncode != 0:
            message = f"{describing} exited {result.returncode}: {result.stderr}"
            raise AssertionError(message)


# A marks file as `json.loads` gives it back. Its values are `Any` because
# these tests write values of every wrong type into it on purpose, to prove
# the tool refuses each one; a narrower type would forbid the very documents
# under test.
Marks = dict[str, Any]

# The bounds the README's "What the fit is held to" states, and why: the RMS
# bound, the per-number bounds, and how far a refit may project a marked
# outline from the committed file.
RMS_BOUND_PX = 4.0
TOLERANCE = {
    "focal_px": 80.0,
    "centre_px": 50.0,
    "k1": 0.02,
    "rotation": 0.03,
    "translation_mm": 10.0,
}
PROJECTION_PX = 5.0

BED_MM = "250x220"
"""The CORE One's printable area, which every run here names."""


def _snapshot_url() -> str:
    with SERVICE_CONFIG.open("rb") as handle:
        return tomllib.load(handle)["camera"]["snapshot_url"]


def _fixture_marks(directory: Path, *, without_edges: bool = False) -> Path:
    doc = json.loads(MARKS.read_text(encoding="utf-8"))
    for frame in doc["frames"]:
        frame["frame"] = str(FRAMES / frame["case"] / frame["file"])
        frame["gcode"] = str((SLICES / SLICE_OF[frame["case"]]).resolve())
        if without_edges:
            frame["curves"] = []
    path = directory / "marks.json"
    path.write_text(json.dumps(doc), encoding="utf-8")
    return path


def _run(
    marks: Path,
    directory: Path,
    *extra: str,
    url: str | None = None,
    covered: bool = False,
) -> subprocess.CompletedProcess[str]:
    """One run of the tool over the CORE One's bed.

    The committed marks lie near the bed's centre and leave its corner cells
    uncovered, so every run over them passes `--allow-uncovered`, the README's
    opt-out for refitting an existing marks file; `covered` runs without it.
    """
    opt_out = () if covered else ("--allow-uncovered",)
    return subprocess.run(  # noqa: S603 - this interpreter, running this tool, with no shell
        [
            sys.executable,
            "-m",
            "camera_calibration",
            "--marks",
            str(marks),
            "--bed-mm",
            BED_MM,
            *opt_out,
            "--snapshot-url",
            url or _snapshot_url(),
            "--calibrated-at",
            "2026-09-27",
            "--out",
            str(directory / "calibration.json"),
            "--verification-image",
            str(directory / "verification.jpg"),
            *extra,
        ],
        cwd=TOOL,
        capture_output=True,
        text=True,
        check=False,
    )


@pytest.fixture(scope="module")
def fitted(
    tmp_path_factory: pytest.TempPathFactory,
) -> tuple[Path, subprocess.CompletedProcess[str]]:
    """One real run of the tool over the committed marks."""
    directory = tmp_path_factory.mktemp("fit")
    return directory, _run(_fixture_marks(directory), directory)


def test_the_fit_reproduces_the_committed_calibration(
    fitted: tuple[Path, subprocess.CompletedProcess[str]],
) -> None:
    """A refit of the committed marks is the committed file, within the stated tolerance."""
    directory, run = fitted
    expect.passing(run, describing="the calibration tool over the committed marks")
    expect.contains(run.stdout, "px RMS", describing="the fit's report")
    surface, below = _whole_bed_figures(run.stdout)
    expect.truth(
        min(surface, below) > COMMITTED_WHOLE_BED_PX,
        describing=f"the committed marks' whole-bed report, {surface} and {below} px, flags them",
    )
    expect.contains(
        run.stderr,
        "--allow-uncovered: the matched marks reach none of the front-left, front-right, "
        "back-left, back-right cells",
        describing="the coverage the committed marks leave, named under the opt-out",
    )
    written = json.loads((directory / "calibration.json").read_text(encoding="utf-8"))
    committed = json.loads(COMMITTED.read_text(encoding="utf-8"))
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    jsonschema.validate(written, schema)
    jsonschema.validate(committed, schema)

    for file in (written, committed):
        expect.equal(file["image_size_px"], [1920, 1080], describing="the image size")
        expect.equal(file["camera_snapshot_url"], _snapshot_url(), describing="the stream")
        expect.equal(file["alignment_max_offset_px"], 30, describing="the alignment threshold")
        expect.equal(file["intrinsics"]["k"][1:], [0.0, 0.0, 0.0], describing="k2-k4")
    expect.equal(written["landmarks"], committed["landmarks"], describing="the landmarks")
    expect.equal(written["fit"]["frames"], committed["fit"]["frames"], describing="the frames")
    rms = written["fit"]["rms_reprojection_px"]
    expect.truth(rms <= RMS_BOUND_PX, describing=f"the fit's RMS {rms} px is within the bound")

    ours, theirs = written["intrinsics"], committed["intrinsics"]
    for key, tolerance in (
        ("fx", "focal_px"),
        ("fy", "focal_px"),
        ("cx", "centre_px"),
        ("cy", "centre_px"),
    ):
        off = abs(ours[key] - theirs[key])
        expect.truth(off <= TOLERANCE[tolerance], describing=f"{key} is {off} from the file's")
    off = abs(ours["k"][0] - theirs["k"][0])
    expect.truth(off <= TOLERANCE["k1"], describing=f"k1 is {off} from the file's")
    for key, tolerance in (
        ("rotation_rodrigues", "rotation"),
        ("translation_mm", "translation_mm"),
    ):
        diff = np.abs(np.array(written["bed_pose"][key]) - np.array(committed["bed_pose"][key]))
        expect.truth(diff.max() <= TOLERANCE[tolerance], describing=f"{key} differs by {diff}")
    outlines = np.vstack(
        [one.segments[:, :3] for one in fitting.observations(load(_fixture_marks(directory)))]
    )
    moved = np.linalg.norm(
        _camera_of(written).project(outlines) - _camera_of(committed).project(outlines), axis=1
    )
    worst = float(np.nanmax(moved))
    expect.truth(
        worst <= PROJECTION_PX,
        describing=f"a marked outline lies {worst} px from the file's, over {PROJECTION_PX}",
    )
    image = cv2.imread(str(directory / "verification.jpg"))
    if image is None:
        message = "the verification image does not decode"
        raise AssertionError(message)
    frames = written["fit"]["frames"]
    expect.equal(
        image.shape[:2],
        (540 * ((frames + 1) // 2), 1920),
        describing="every marked frame tiled two to a row at half size",
    )
    blue, green, red = (image[..., channel].astype(int) for channel in range(3))
    marks = int(((red > 180) & (green < 90) & (blue < 90)).sum())
    outlines = int(((green > 150) & (red < 100) & (blue < 100)).sum())
    expect.truth(marks > 50, describing=f"the marks are drawn in red ({marks} pixels)")
    expect.truth(outlines > 1000, describing=f"the outlines are drawn in green ({outlines} pixels)")


def test_a_fit_started_from_the_committed_calibration_reproduces_it(tmp_path: Path) -> None:
    """`--start` naming the committed file fits the committed marks back to it."""
    run = _run(_fixture_marks(tmp_path), tmp_path, "--start", str(COMMITTED))
    expect.passing(run, describing="the calibration tool started from the committed file")
    written = json.loads((tmp_path / "calibration.json").read_text(encoding="utf-8"))
    committed = json.loads(COMMITTED.read_text(encoding="utf-8"))
    jsonschema.validate(written, json.loads(SCHEMA.read_text(encoding="utf-8")))
    rms = written["fit"]["rms_reprojection_px"]
    expect.truth(rms <= RMS_BOUND_PX, describing=f"the fit's RMS {rms} px is within the bound")
    outlines = np.vstack(
        [one.segments[:, :3] for one in fitting.observations(load(_fixture_marks(tmp_path)))]
    )
    moved = np.linalg.norm(
        _camera_of(written).project(outlines) - _camera_of(committed).project(outlines), axis=1
    )
    worst = float(np.nanmax(moved))
    expect.truth(
        worst <= PROJECTION_PX,
        describing=f"a marked outline lies {worst} px from the file's, over {PROJECTION_PX}",
    )


def _resized(calibration: dict[str, Any]) -> dict[str, Any]:
    calibration["image_size_px"] = [1280, 720]
    return calibration


def _poseless(calibration: dict[str, Any]) -> dict[str, Any]:
    del calibration["bed_pose"]
    return calibration


def _flat_pose(calibration: dict[str, Any]) -> dict[str, Any]:
    calibration["bed_pose"]["rotation_rodrigues"] = [0.0, 0.0]
    return calibration


@pytest.mark.parametrize(
    ("change", "refusal"),
    [
        (_resized, "is a calibration of 1280x720 frames, and the marked frames are 1920x1080"),
        (_poseless, "is no calibration file this tool reads"),
        (_flat_pose, "states no three-component bed pose"),
    ],
)
def test_a_start_that_is_no_calibration_of_the_stream_is_refused(
    tmp_path: Path, change: Callable[[dict[str, Any]], dict[str, Any]], refusal: str
) -> None:
    """`--start` naming a calibration of other frames, or no calibration, is refused with exit 2."""
    start = tmp_path / "start.json"
    start.write_text(
        json.dumps(change(json.loads(COMMITTED.read_text(encoding="utf-8")))), encoding="utf-8"
    )
    run = _run(_fixture_marks(tmp_path), tmp_path, "--start", str(start))
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, refusal, describing="the refusal")
    expect.truth(
        not (tmp_path / "calibration.json").exists(), describing="no calibration was written"
    )


def _camera_of(calibration: dict[str, Any]) -> Camera:
    lens, pose = calibration["intrinsics"], calibration["bed_pose"]
    return Camera(
        lens["fx"],
        lens["fy"],
        lens["cx"],
        lens["cy"],
        lens["k"][0],
        np.array(pose["rotation_rodrigues"]),
        np.array(pose["translation_mm"]),
    )


def test_the_readme_states_the_bounds_these_tests_hold() -> None:
    """Each bound held above is the one the README states, so neither copy drifts alone."""
    plate_grams = plate.grams(
        sum(one.volume_mm3 for one in plate.solids(PLATE.read_text(encoding="ascii")))
    )
    # Read as one line of words, so a bound wrapped across lines is still found.
    readme = " ".join((TOOL / "README.md").read_text(encoding="utf-8").split())
    stated = [
        f"**{RMS_BOUND_PX:g} px**",
        f"**{PROJECTION_PX:g} px**",
        f"**{TOLERANCE['focal_px']:g} px** for `fx` and `fy`",
        f"**{TOLERANCE['centre_px']:g} px** for `cx` and `cy`",
        f"**{TOLERANCE['k1']:g}** for `k1`",
        f"**{TOLERANCE['rotation']:g} rad**",
        f"**{TOLERANCE['translation_mm']:g} mm**",
        f"within **{PLATE_REFIT_PX:g} px** of that calibration",
        f"one frame at each of five bed heights, {PLATE_HEIGHTS_MM[0]:g} to "
        f"{PLATE_HEIGHTS_MM[-1]:g} mm",
        "`--allow-uncovered`",
        f"Its whole-bed report reads more than {COMMITTED_WHOLE_BED_PX:g} px",
        f"Its whole-bed report reads under {PLATE_WHOLE_BED_PX:g} px",
        f"Each corner's arms and each bar are {plate.ARM_MM:g} mm long and "
        f"{plate.WIDTH_MM:g} mm wide",
        f"about {plate_grams:.1f} g of PLA",
        f"plates/{PLATE.name}",
    ]
    for bound in stated:
        expect.contains(readme, bound, describing="the README's statement of the fit's bounds")


def test_the_marks_reproject_within_the_rms_bound(tmp_path: Path) -> None:
    """The committed calibration puts the marks within the RMS bound of their outlines."""
    camera = _camera_of(json.loads(COMMITTED.read_text(encoding="utf-8")))
    observed = fitting.observations(load(_fixture_marks(tmp_path)))
    near = [fitting.near_segments(camera, one) for one in observed]
    residuals = fitting.residuals_px(camera, observed, near)
    expect.equal(len(residuals), 50, describing="every committed mark")
    rms = float(np.sqrt(np.mean(residuals**2)))
    expect.truth(rms <= RMS_BOUND_PX, describing=f"the marks' RMS {rms} px is within the bound")


def test_marks_naming_no_part_edge_are_refused(tmp_path: Path) -> None:
    """Marks with no point on any part's edge fit nothing, and nothing is written."""
    run = _run(_fixture_marks(tmp_path, without_edges=True), tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, "marks no part edge", describing="the refusal")
    expect.truth(
        not (tmp_path / "calibration.json").exists(), describing="no calibration is written"
    )


def test_marks_that_are_not_utf8_are_refused(tmp_path: Path) -> None:
    """A marks file that is not UTF-8 text is refused with exit 2, naming it."""
    marks = tmp_path / "marks.json"
    marks.write_bytes(b'\xff\xfe{"frames": []}')
    run = _run(marks, tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, f"{marks} is not UTF-8 text", describing="the refusal")
    expect.truth(
        not (tmp_path / "calibration.json").exists(), describing="no calibration is written"
    )


def test_frames_of_two_sizes_are_refused(tmp_path: Path) -> None:
    """A calibration is for one resolution: marks on frames of two sizes are refused."""
    doc = json.loads(_fixture_marks(tmp_path).read_text(encoding="utf-8"))
    doc["frames"][0]["frame"] = str(FRAMES / "fan-cut-bridge" / "01-healthy-before-fault-p41.jpg")
    marks = tmp_path / "mixed.json"
    marks.write_text(json.dumps(doc), encoding="utf-8")
    run = _run(marks, tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, "of 2 sizes", describing="the refusal")


def _edited(directory: Path, change: Callable[[Marks], None], name: str = "edited") -> Path:
    doc = json.loads(_fixture_marks(directory).read_text(encoding="utf-8"))
    change(doc)
    path = directory / f"{name}.json"
    path.write_text(json.dumps(doc), encoding="utf-8")
    return path


def _first_curve(doc: Marks) -> Marks:
    return doc["frames"][0]["curves"][0]


def _repeat_marks(doc: Marks, *, frames: int) -> None:
    """Keep `frames` frames, each curve's marks repeated to stay above the least count."""
    doc["frames"] = doc["frames"][:frames]
    for frame in doc["frames"]:
        for curve in frame["curves"]:
            curve["px"] = curve["px"] * 4


def _set_heights(doc: Marks, heights: list[float]) -> None:
    for frame, height in zip(doc["frames"], heights, strict=True):
        frame["zc_mm"] = height


@pytest.mark.parametrize(
    ("change", "refusal"),
    [
        (lambda doc: doc.update(frames="none"), "carries no `frames` list"),
        (lambda doc: doc["frames"][0].update(frame="missing.jpg"), "which is not a file"),
        (lambda doc: doc["frames"][0].update(gcode="missing.gcode"), "which is not a file"),
        (lambda doc: doc["frames"][0].update(curves="none"), "not a list"),
        (lambda doc: _first_curve(doc).update(what="side"), "neither a `top` nor a `first`"),
        (lambda doc: _first_curve(doc).update(z="high"), "not a finite number"),
        (lambda doc: _first_curve(doc).update(z=10**400), "not a finite number"),
        (lambda doc: _first_curve(doc).update(what=["top"]), "neither a `top` nor a `first`"),
        (lambda doc: _first_curve(doc).update(px=[[1, 2, 3]]), "not an [x, y] pixel"),
        (lambda doc: doc["frames"][0].update(zc_mm=None), "not a finite number"),
        (lambda doc: doc["frames"][0].pop("case"), "names no `case`"),
        (lambda doc: doc["frames"].append("a frame"), "is not an object"),
        (lambda doc: doc.update(frames=doc["frames"][:4]), "a fit needs at least 30 points"),
        (lambda doc: _repeat_marks(doc, frames=4), "on 4 frames"),
        (lambda doc: _set_heights(doc, [0.4] * 7), "at 1 bed heights"),
        (lambda doc: _set_heights(doc, [1.0, 2, 3, 4, 5, 6, 7]), "spanning 6.0 mm"),
        (lambda doc: _first_curve(doc).update(px=[[5000, 10]]), "a mark on it is at"),
        (lambda doc: doc.update(landmarks=["rail"]), "landmarks[0] is not an object"),
        (lambda doc: doc["landmarks"][0].update(center_px=[1915, 5]), "too near the frame's edge"),
        (lambda doc: doc.pop("landmarks"), "names 0 chassis landmarks"),
        (lambda doc: doc.update(landmarks=doc["landmarks"][:1]), "names 1 chassis landmarks"),
        (
            lambda doc: doc["landmarks"][1].update(name=doc["landmarks"][0]["name"]),
            "each named once",
        ),
    ],
)
def test_marks_the_fit_cannot_use_are_refused_naming_why(
    tmp_path: Path, change: Callable[[Marks], None], refusal: str
) -> None:
    """Each kind of unusable marks file is refused with exit 2, naming what is wrong."""
    run = _run(_edited(tmp_path, change), tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, refusal, describing="the refusal")


@pytest.mark.parametrize(
    ("extra", "url", "refusal"),
    [
        (("--calibrated-at", "27/09/2026"), None, "not a YYYY-MM-DD date"),
        (("--drift-tolerance-px", "inf"), None, "not a positive number"),
        ((), "http://", "not an http(s) address"),
        ((), "http://[::1", "not an http(s) address"),
        ((), "http://localhost:99999/frame.jpeg", "not an http(s) address"),
        (("--bed-mm", "250"), None, "not a <W>x<D> printable area"),
        (("--bed-mm", "250x-1"), None, "not a <W>x<D> printable area"),
    ],
)
def test_arguments_the_tool_cannot_use_are_refused(
    tmp_path: Path, extra: tuple[str, ...], url: str | None, refusal: str
) -> None:
    """An argument that is no date, tolerance, address or bed is refused with exit 2."""
    run = _run(_fixture_marks(tmp_path), tmp_path, *extra, url=url)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, refusal, describing="the refusal")


def test_a_frame_no_decoder_reads_and_a_slice_with_no_layers_are_refused(tmp_path: Path) -> None:
    """A frame that is no image, and a slice marking no layer, are refused naming the file."""
    broken = tmp_path / "broken.jpg"
    broken.write_bytes(b"not an image")
    run = _run(_edited(tmp_path, lambda doc: doc["frames"][0].update(frame=str(broken))), tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, "not an image this tool can decode", describing="the refusal")
    bare = tmp_path / "bare.gcode"
    bare.write_text("G1 X1 Y1 E1\n", encoding="utf-8")
    run = _run(_edited(tmp_path, lambda doc: doc["frames"][0].update(gcode=str(bare))), tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, "marks no layer", describing="the refusal")


def test_unreadable_marks_and_slices_lacking_an_outline_are_refused(tmp_path: Path) -> None:
    """Unreadable marks, unreadable heights and a missing outline are refused with exit 2.

    Marks that are no JSON, a slice whose heights are no numbers, and a slice
    with no outline where a mark says one lies.
    """
    garbled = tmp_path / "garbled.json"
    garbled.write_text("{ not json", encoding="utf-8")
    cases = [(garbled, "garbled.json")]
    bad_height = tmp_path / "bad-height.gcode"
    bad_height.write_text(";LAYER_CHANGE\n;Z:high\n", encoding="utf-8")
    cases.append(
        (
            _edited(
                tmp_path, lambda doc: doc["frames"][0].update(gcode=str(bad_height)), "heights"
            ),
            "no number",
        )
    )
    no_walls = tmp_path / "no-walls.gcode"
    no_walls.write_text(
        ";LAYER_CHANGE\n;Z:0.2\nG90\nM83\n;TYPE:Internal infill\nG1 X1 Y1\nG1 X9 Y1 E1\n",
        encoding="utf-8",
    )
    top_marked = next(
        number
        for number, frame in enumerate(json.loads(MARKS.read_text(encoding="utf-8"))["frames"])
        if any(curve["what"] == "top" for curve in frame["curves"])
    )
    cases.append(
        (
            _edited(
                tmp_path,
                lambda doc: doc["frames"][top_marked].update(gcode=str(no_walls)),
                "walls",
            ),
            "no top outline",
        )
    )
    for marks, refusal in cases:
        run = _run(marks, tmp_path)
        expect.equal(run.returncode, 2, describing=f"the exit status for {marks.name}")
        expect.contains(run.stderr, refusal, describing="the refusal")


def test_an_output_it_cannot_write_is_refused(tmp_path: Path) -> None:
    """A calibration path in a directory that is not there is refused before any fit."""
    run = subprocess.run(  # noqa: S603 - this interpreter, running this tool, with no shell
        [
            sys.executable,
            "-m",
            "camera_calibration",
            "--marks",
            str(_fixture_marks(tmp_path)),
            "--bed-mm",
            BED_MM,
            "--snapshot-url",
            _snapshot_url(),
            "--out",
            str(tmp_path / "absent" / "calibration.json"),
            "--verification-image",
            str(tmp_path / "verification.jpg"),
        ],
        cwd=TOOL,
        capture_output=True,
        text=True,
        check=False,
    )
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, "absent", describing="the refusal")
    image = _run(
        _fixture_marks(tmp_path), tmp_path, "--verification-image", str(tmp_path / "check.bmp")
    )
    expect.equal(image.returncode, 2, describing="the exit status for a .bmp image")
    expect.contains(image.stderr, "not a .jpg path", describing="the refusal")
    same = _run(
        _fixture_marks(tmp_path),
        tmp_path,
        "--verification-image",
        str(tmp_path / "calibration.json"),
    )
    expect.equal(same.returncode, 2, describing="the exit status for one file named twice")
    expect.contains(same.stderr, "name one file", describing="the refusal")


def test_an_output_that_cannot_be_written_after_the_fit_is_refused(tmp_path: Path) -> None:
    """A directory standing at either output path passes the checks before the fit, then fails.

    Each run is a whole fit, started from the committed calibration: where the
    fit starts is no part of what is written, and a fit from the grid of starts
    costs ten times as much. The two runs are independent, each over its own
    directory, so they are made at once.
    """
    marks = _fixture_marks(tmp_path)
    image_blocked = tmp_path / "image"
    (image_blocked / "verification.jpg").mkdir(parents=True)
    out_blocked = tmp_path / "out"
    (out_blocked / "calibration.json").mkdir(parents=True)
    start = ("--start", str(COMMITTED))
    with ThreadPoolExecutor(max_workers=2) as runs:
        image_run = runs.submit(_run, marks, image_blocked, *start)
        out_run = runs.submit(_run, marks, out_blocked, *start)
        image, out = image_run.result(), out_run.result()
    expect.equal(image.returncode, 2, describing="the exit status for an unwritable image")
    expect.contains(image.stderr, "could not be written", describing="the image refusal")
    expect.truth(
        not (image_blocked / "calibration.json").exists(),
        describing="no calibration written beside an image that failed",
    )
    expect.equal(out.returncode, 2, describing="the exit status for an unwritable calibration")
    # The path as the operator wrote it, never quoted as a Python literal,
    # which on Windows would double every separator.
    expect.contains(
        out.stderr, f"{out_blocked / 'calibration.json'}: ", describing="the calibration refusal"
    )


def test_a_slice_is_read_in_every_positioning_mode_and_past_any_machine_is_refused(
    tmp_path: Path,
) -> None:
    """Every positioning mode is followed, and a word too large for any machine is refused.

    Relative axes, absolute extrusion and a reset extruder are each read as a
    slicer means them.
    """
    slice_path = tmp_path / "modes.gcode"
    slice_path.write_text(
        ";LAYER_CHANGE\n;Z:0.2\n;TYPE:External perimeter\nG90\nM82\nG92 E0\n"
        "G1 X10 Y10\nG1 X20 Y10 E1\nG91\nG1 X0 Y10 E1\nG1 X5 Y0 E0\nG90\nM83\nG1 X30 Y30 E-1\n",
        encoding="utf-8",
    )
    layers = gcode.parse(slice_path)
    expect.equal(
        layers[0].outer,
        [(10.0, 10.0, 20.0, 10.0), (20.0, 10.0, 20.0, 20.0)],
        describing="the extruding moves, absolute then relative, and no travel or retraction",
    )
    huge = tmp_path / "huge.gcode"
    huge.write_text(";LAYER_CHANGE\n;Z:0.2\nG1 X" + "9" * 400 + " Y1 E1\n", encoding="utf-8")
    garbled = tmp_path / "garbled.gcode"
    garbled.write_text(";LAYER_CHANGE\n;Z:0.2\nG1 Xnan Y1 E1\n", encoding="utf-8")
    try:
        gcode.parse(garbled)
    except gcode.GcodeError as error:
        expect.contains(str(error), "no finite number", describing="the refusal of X nan")
    else:
        message = "a slice with a word that is no number was read"
        raise AssertionError(message)
    try:
        gcode.parse(huge)
    except gcode.GcodeError as error:
        expect.contains(str(error), "no finite number", describing="the refusal")
    else:
        message = "a slice with an infinite coordinate was read"
        raise AssertionError(message)
    wide = tmp_path / "wide.gcode"
    wide.write_text(";LAYER_CHANGE\n;Z:0.2\nG1 X1 Y1\nG2 X2 Y1 I1e308 J0 E1\n", encoding="utf-8")
    try:
        gcode.parse(wide)
    except gcode.GcodeError as error:
        expect.contains(
            str(error), "plans an arc of radius", describing="the refusal of a wide arc"
        )
    else:
        message = "a slice planning an arc wider than any machine was read"
        raise AssertionError(message)


def test_the_workspace_lock_holds_this_tools_requirements() -> None:
    """The root uv.lock, where this member's requirements resolve, is current with them."""
    uv = shutil.which("uv")
    expect.truth(uv, describing="uv is on PATH")
    run = subprocess.run(  # noqa: S603 - uv, by its resolved path, with no shell
        [str(uv), "lock", "--check"], cwd=ROOT, capture_output=True, text=True, check=False
    )
    expect.passing(run, describing="`uv lock --check` at the workspace root")


PLATE = TOOL / "plates" / "calibration-plate-250x220.stl"
TELEMETRY = TOOL / "telemetry" / "plate.json"
"""Where the plate's test records the generated mesh, for the filament budget to read."""
PLATE_MASS_BOUND_G = next(
    one.threshold
    for one in list_budgets(paths=[TOOL / "budgets.yaml"]).budgets
    if one.id == "calibration-plate-filament-grams"
)
"""The `calibration-plate-filament-grams` budget's target, as `budgets.yaml` registers it."""

PLATE_HEIGHTS_MM = (0.0, 10.0, 20.0, 30.0, 40.0)
"""The bed lowered to five heights spanning 40 mm, one frame at each, as the README asks."""

PLATE_REFIT_PX = 10.0
"""How far a refit of plate marks may project any point of the bed from the camera they were
projected through: the README's statement of what a plate fit is held to."""

PLATE_LAYER_MM = 0.2

PLATE_WHOLE_BED_PX = 1.0
"""What a plate fit's whole-bed report reads under, as the README states."""

COMMITTED_WHOLE_BED_PX = 100.0
"""What the committed marks' whole-bed report reads over, as the README states."""


def _generate_plate(directory: Path, bed: str = BED_MM) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603 - this interpreter, running this tool, with no shell
        [
            sys.executable,
            "-m",
            "camera_calibration.plate",
            "--bed-mm",
            bed,
            "--out",
            str(directory / "plate.stl"),
        ],
        cwd=TOOL,
        capture_output=True,
        text=True,
        check=False,
    )


def _pieces(stl: str) -> list[np.ndarray]:
    """Each connected piece of an ASCII STL, as its triangles (N, 3, 3), read here alone."""
    corners = [
        tuple(float(word) for word in line.split()[1:])
        for line in stl.splitlines()
        if line.split()[:1] == ["vertex"]
    ]
    triangles = [corners[at : at + 3] for at in range(0, len(corners), 3)]
    label: dict[tuple[float, ...], int] = {}
    pieces: list[set[int]] = []
    for number, triangle in enumerate(triangles):
        touching = {label[vertex] for vertex in triangle if vertex in label}
        merged = set().union(*(pieces[one] for one in touching), {number})
        pieces.append(merged)
        for one in merged:
            for vertex in triangles[one]:
                label[vertex] = len(pieces) - 1
    final = {label[triangle[0]] for triangle in triangles}
    return [np.array([triangles[one] for one in sorted(pieces[at])]) for at in sorted(final)]


def _volume_mm3(piece: np.ndarray) -> float:
    return float(np.einsum("ij,ij->i", piece[:, 0], np.cross(piece[:, 1], piece[:, 2])).sum() / 6)


@pytest.mark.parametrize(("width", "depth"), [(250.0, 220.0), (180.0, 320.0)])
def test_the_plate_marks_every_corner_edge_and_the_centre_in_one_layer(
    tmp_path: Path, width: float, depth: float
) -> None:
    """The generated plate is nine closed single-layer marks reaching the whole printable area.

    The CORE One's bed and one of other proportions: the marks lie apart, and
    together they are centred on the bed, as a slicer centring the plate leaves them.
    """
    run = _generate_plate(tmp_path, f"{width:g}x{depth:g}")
    expect.passing(run, describing=f"the plate generator for a {width:g} x {depth:g} mm bed")
    expect.contains(run.stdout, "9 marks", describing="the generator's report")
    pieces = _pieces((tmp_path / "plate.stl").read_text(encoding="ascii"))
    expect.equal(len(pieces), 9, describing="the plate's marks")
    boxes = []
    for piece in pieces:
        heights = sorted({round(float(z), 6) for z in piece[..., 2].ravel()})
        expect.equal(heights, [0.0, PLATE_LAYER_MM], describing="a mark's one layer")
        edges: dict[tuple[tuple[float, ...], tuple[float, ...]], int] = {}
        for triangle in piece:
            for a, b in ((0, 1), (1, 2), (2, 0)):
                one, other = tuple(triangle[a]), tuple(triangle[b])
                edges[(one, other)] = edges.get((one, other), 0) + 1
        closed = all(edges.get((b, a)) == count == 1 for (a, b), count in edges.items())
        expect.truth(closed, describing="every edge of a mark is shared by one facing triangle")
        expect.truth(_volume_mm3(piece) > 0, describing="a mark's faces point outward")
        low, high = piece[..., :2].reshape(-1, 2).min(0), piece[..., :2].reshape(-1, 2).max(0)
        expect.truth(
            (low >= 0).all() and high[0] <= width and high[1] <= depth,
            describing=f"a mark from {low} to {high} lies on the printable area",
        )
        boxes.append((low, high))

    def reached(x: float, y: float, within: float) -> bool:
        return any(
            low[0] - within <= x <= high[0] + within and low[1] - within <= y <= high[1] + within
            for low, high in boxes
        )

    places = {
        "front-left corner": (0, 0),
        "front-right corner": (width, 0),
        "back-left corner": (0, depth),
        "back-right corner": (width, depth),
        "front edge's midpoint": (width / 2, 0),
        "back edge's midpoint": (width / 2, depth),
        "left edge's midpoint": (0, depth / 2),
        "right edge's midpoint": (width, depth / 2),
    }
    for name, (x, y) in places.items():
        expect.truth(reached(x, y, 10.0), describing=f"a mark within 10 mm of the {name}")
    expect.truth(reached(width / 2, depth / 2, 0.0), describing="a mark over the centre")
    for at, (low, high) in enumerate(boxes):
        for other_low, other_high in boxes[at + 1 :]:
            apart = (high < other_low).any() or (other_high < low).any()
            expect.truth(apart, describing=f"the mark from {low} to {high} lies apart")
    lowest = np.min([low for low, _ in boxes], axis=0)
    highest = np.max([high for _, high in boxes], axis=0)
    expect.truth(
        np.allclose((lowest + highest) / 2, [width / 2, depth / 2]),
        describing=f"the marks, from {lowest} to {highest}, are centred on the bed",
    )
    grams = sum(_volume_mm3(piece) for piece in pieces) / 1000 * 1.24
    expect.truth(grams <= PLATE_MASS_BOUND_G, describing=f"the plate takes {grams:.2f} g of PLA")


def test_the_committed_plate_is_what_the_generator_writes(tmp_path: Path) -> None:
    """The committed 250 x 220 mm plate is byte for byte the generator's output.

    The generated mesh's volume, mark count and each mark's volume are then
    recorded to `TELEMETRY`, which the `calibration-plate-filament-grams`
    budget reads.
    """
    expect.passing(_generate_plate(tmp_path), describing="the plate generator")
    generated = (tmp_path / "plate.stl").read_bytes()
    expect.equal(
        generated == PLATE.read_bytes(),
        True,
        describing=f"{PLATE.name} as the generator writes it",
    )
    pieces = plate.solids(generated.decode("ascii"))
    TELEMETRY.parent.mkdir(parents=True, exist_ok=True)
    TELEMETRY.write_text(plate.telemetry(pieces) + "\n", encoding="utf-8")
    recorded = json.loads(TELEMETRY.read_text(encoding="utf-8"))
    expect.equal(recorded["mark_count"], 9, describing="the recorded mark count")
    volumes = [_volume_mm3(piece) for piece in _pieces(generated.decode("ascii"))]
    expect.truth(
        abs(recorded["mesh_volume_mm3"] - sum(volumes)) < 1e-6,
        describing=f"the recorded {recorded['mesh_volume_mm3']} mm3 is the mesh's {sum(volumes)}",
    )
    expect.equal(
        sorted(round(one["volume_mm3"], 6) for one in recorded["marks"]),
        sorted(round(one, 6) for one in volumes),
        describing="each recorded mark's volume",
    )


@pytest.mark.parametrize(
    ("bed", "refusal"),
    [("100x220", "too small for the plate"), ("250by220", "not a <W>x<D> printable area")],
)
def test_a_bed_the_plate_cannot_be_laid_on_is_refused(
    tmp_path: Path, bed: str, refusal: str
) -> None:
    """A bed too small for the marks, or no bed at all, is refused with exit 2 and no file."""
    run = _generate_plate(tmp_path, bed)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, refusal, describing="the refusal")
    expect.truth(not (tmp_path / "plate.stl").exists(), describing="no plate is written")


def test_plate_mass_reports_the_committed_plates_pla(tmp_path: Path) -> None:
    """`plate-mass` reports the committed plate's PLA where onebudgetspec reads a result."""
    result = tmp_path / "result.json"
    run = subprocess.run(
        [sys.executable, "-m", "camera_calibration", "plate-mass"],
        cwd=TOOL,
        capture_output=True,
        text=True,
        check=False,
        env={**os.environ, "ONEBUDGETSPEC_RESULT": str(result)},
    )
    expect.passing(run, describing="plate-mass")
    reported = json.loads(result.read_text(encoding="utf-8"))
    pieces = _pieces(PLATE.read_text(encoding="ascii"))
    grams = sum(_volume_mm3(piece) for piece in pieces) / 1000 * 1.24
    expect.truth(
        abs(reported["value"] - grams) < 1e-9,
        describing=f"the reported {reported['value']} g is the mesh's {grams} g",
    )
    expect.contains(reported["detail"], "9 marks", describing="the reported detail")
    # Every facet's three vertices in the reverse order: the same mesh turned inside out.
    committed = PLATE.read_text(encoding="ascii").splitlines()
    lines, at = [], 0
    while at < len(committed):
        step = 3 if "vertex" in committed[at] else 1
        lines += committed[at : at + step][::-1]
        at += step
    triangle = "  vertex 0 0 0\n  vertex 1 0 0\n  vertex 0 1 0\n"
    for name, text, refusal in (
        ("short.stl", "solid x\n  vertex 1 2\nendsolid x\n", "no three numbers"),
        ("nan.stl", "solid x\n" + triangle.replace("1 0 0", "nan 0 0") + "endsolid x\n", "finite"),
        ("empty.stl", "solid x\nendsolid x\n", "carries 0 vertices"),
        ("open.stl", f"solid x\n{triangle}endsolid x\n", "is not closed"),
        ("inward.stl", "\n".join(lines) + "\n", "its faces point inward"),
        ("absent.stl", None, "absent.stl"),
    ):
        broken = tmp_path / name
        if text is not None:
            broken.write_text(text, encoding="ascii")
        refused = subprocess.run(  # noqa: S603 - this interpreter, running this tool, with no shell
            [sys.executable, "-m", "camera_calibration", "plate-mass", "--stl", str(broken)],
            cwd=TOOL,
            capture_output=True,
            text=True,
            check=False,
        )
        expect.equal(refused.returncode, 2, describing=f"the exit status for {name}")
        expect.contains(refused.stderr, refusal, describing=f"the refusal of {name}")


def test_plate_mass_reports_a_recorded_plates_pla(tmp_path: Path) -> None:
    """`plate-mass --telemetry` reports the PLA of the record the plate's test writes.

    The record is made here from the committed plate, in the shape that test
    writes, so this does not hang on which test ran first; a record whose
    figures disagree, or that is no record, is refused with exit 2.
    """
    record = tmp_path / "plate.json"
    record.write_text(plate.telemetry(plate.solids(PLATE.read_text(encoding="ascii"))))
    result = tmp_path / "result.json"
    run = subprocess.run(  # noqa: S603 - this interpreter, running this tool, with no shell
        [sys.executable, "-m", "camera_calibration", "plate-mass", "--telemetry", str(record)],
        cwd=TOOL,
        capture_output=True,
        text=True,
        check=False,
        env={**os.environ, "ONEBUDGETSPEC_RESULT": str(result)},
    )
    expect.passing(run, describing="plate-mass over a recorded plate")
    reported = json.loads(result.read_text(encoding="utf-8"))
    grams = sum(_volume_mm3(piece) for piece in _pieces(PLATE.read_text(encoding="ascii")))
    grams = grams / 1000 * 1.24
    expect.truth(
        abs(reported["value"] - grams) < 1e-9,
        describing=f"the reported {reported['value']} g is the mesh's {grams} g",
    )
    expect.contains(reported["detail"], "9 marks; mark at (25, 25) mm", describing="the detail")
    short = json.loads(record.read_text(encoding="utf-8"))
    short["mark_count"] = 8
    negative = json.loads(record.read_text(encoding="utf-8"))
    negative["mesh_volume_mm3"] = -negative["mesh_volume_mm3"]
    for one in negative["marks"]:
        one["volume_mm3"] = -one["volume_mm3"]
    boolean = json.loads(record.read_text(encoding="utf-8"))
    boolean["marks"][0]["volume_mm3"] = True
    lettered = json.loads(record.read_text(encoding="utf-8"))
    lettered["marks"][0]["centre_mm"] = "12"
    nothing = {"mesh_volume_mm3": 0, "mark_count": 0, "marks": []}
    for name, text, refusal in (
        ("short.json", json.dumps(short), "names 8 marks"),
        ("counted.json", json.dumps({**short, "mark_count": 9.0}), "it needs a mark_count"),
        ("negative.json", json.dumps(negative), "a mark of no volume"),
        ("empty.json", json.dumps(nothing), "carries no mark"),
        ("boolean.json", json.dumps(boolean), "is no volume_mm3 number"),
        ("lettered.json", json.dumps(lettered), "centre_mm [x, y] pair"),
        ("infinite.json", json.dumps({**short, "mesh_volume_mm3": "inf"}), "not a finite number"),
        ("unbounded.json", '{"mesh_volume_mm3": Infinity, "mark_count": 0, "marks": []}', "finite"),
        ("bare.json", "{}", "no plate telemetry record"),
        ("unparsed.json", "{", "no plate telemetry record"),
        ("absent.json", None, "absent.json"),
    ):
        broken = tmp_path / name
        if text is not None:
            broken.write_text(text, encoding="utf-8")
        refused = subprocess.run(  # noqa: S603 - this interpreter, running this tool, with no shell
            [sys.executable, "-m", "camera_calibration", "plate-mass", "--telemetry", str(broken)],
            cwd=TOOL,
            capture_output=True,
            text=True,
            check=False,
        )
        expect.equal(refused.returncode, 2, describing=f"the exit status for {name}")
        expect.contains(refused.stderr, refusal, describing=f"the refusal of {name}")


def test_a_plate_path_that_cannot_be_written_is_refused(tmp_path: Path) -> None:
    """A plate written over a directory is refused with exit 2, naming the path."""
    (tmp_path / "plate.stl").mkdir()
    run = _generate_plate(tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, str(tmp_path / "plate.stl"), describing="the refusal")


def _plate_marks(
    directory: Path, keep: Callable[[str], bool] = lambda _: True, *, seed: int | None = None
) -> Path:
    """Marks on the plate's first-layer outlines, projected through the committed calibration.

    One frame at each of `PLATE_HEIGHTS_MM`, a mark at the middle of every edge
    of every kept mark that projects into the frame. The slice is the plate's
    first layer planned as its outlines alone. With `seed`, every pixel is moved
    by a normal draw of `coverage.PERTURBATION_PX`, as the README states the
    whole-bed report's refits are, drawn here in file order.
    """
    camera = _camera_of(json.loads(COMMITTED.read_text(encoding="utf-8")))
    committed = json.loads(MARKS.read_text(encoding="utf-8"))
    image = FRAMES / committed["frames"][0]["case"] / committed["frames"][0]["file"]
    slice_lines = [";LAYER_CHANGE", f";Z:{PLATE_LAYER_MM}", "G90", "M83"]
    kept = [mark for mark in plate.marks(plate.Bed(250.0, 220.0)) if keep(mark.name)]
    for mark in plate.marks(plate.Bed(250.0, 220.0)):
        ring = [*mark.outline, mark.outline[0]]
        slice_lines += [";TYPE:External perimeter", f"G1 X{ring[0][0]} Y{ring[0][1]}"]
        slice_lines += [f"G1 X{x} Y{y} E0.05" for x, y in ring[1:]]
    slice_path = directory / "plate.gcode"
    slice_path.write_text("\n".join(slice_lines) + "\n", encoding="utf-8")
    generator = np.random.default_rng(seed) if seed is not None else None
    frames = []
    for height in PLATE_HEIGHTS_MM:
        middles = [
            ((x0 + x1) / 2, (y0 + y1) / 2, PLATE_LAYER_MM - height)
            for mark in kept
            for (x0, y0), (x1, y1) in zip(
                mark.outline, mark.outline[1:] + mark.outline[:1], strict=True
            )
        ]
        px = camera.project(np.array(middles))
        px = px[(px[:, 0] >= 0) & (px[:, 0] < 1920) & (px[:, 1] >= 0) & (px[:, 1] < 1080)]
        if generator is not None:
            px = px + generator.normal(0.0, coverage.PERTURBATION_PX, px.shape)
        frames.append(
            {
                "case": "calibration-plate",
                "file": f"lowered-{height:g}mm.jpg",
                "frame": str(image),
                "gcode": str(slice_path),
                "zc_mm": height,
                "curves": [{"what": "first", "z": PLATE_LAYER_MM, "px": px.tolist()}],
            }
        )
    path = directory / ("plate-marks.json" if seed is None else f"plate-marks-{seed}.json")
    path.write_text(
        json.dumps({"frames": frames, "landmarks": committed["landmarks"]}), encoding="utf-8"
    )
    return path


def _bed_grid(z: float) -> np.ndarray:
    """An 11 x 11 grid over the 250 x 220 mm bed at world height `z`, built here alone."""
    return np.array([[x, y, z] for x in np.linspace(0, 250, 11) for y in np.linspace(0, 220, 11)])


def test_plate_marks_refit_the_camera_across_the_whole_bed(tmp_path: Path) -> None:
    """Plate marks projected through the committed camera refit it, from no start, bed-wide."""
    run = _run(_plate_marks(tmp_path), tmp_path, covered=True)
    expect.passing(run, describing="the calibration tool over plate marks")
    fitted = _camera_of(json.loads((tmp_path / "calibration.json").read_text(encoding="utf-8")))
    truth = _camera_of(json.loads(COMMITTED.read_text(encoding="utf-8")))
    apart = np.linalg.norm(fitted.project(_bed_grid(0.0)) - truth.project(_bed_grid(0.0)), axis=1)
    worst = float(apart.max())
    expect.truth(
        worst <= PLATE_REFIT_PX,
        describing=f"a point of the bed projects {worst:.2f} px from the committed camera",
    )
    surface, below = _whole_bed_figures(run.stdout)
    expect.truth(
        max(surface, below) < PLATE_WHOLE_BED_PX,
        describing=f"the plate fit's whole-bed report, {surface} and {below} px, holds bed-wide",
    )


def _whole_bed_figures(stdout: str) -> tuple[float, float]:
    found = re.search(
        r"at most ([0-9.]+) px on the bed's surface and ([0-9.]+) px 40 mm below it", stdout
    )
    if found is None:
        message = f"the fit printed no whole-bed report: {stdout!r}"
        raise AssertionError(message)
    return float(found.group(1)), float(found.group(2))


def test_the_whole_bed_report_is_the_fits_largest_disagreement_with_its_refits(
    tmp_path: Path,
) -> None:
    """Each printed figure is the largest grid disagreement between the fit and its refits.

    The refits are made here from marks perturbed here, each fitted as the
    tool fits, from the same start, and compared over a grid built here.
    """
    marks = _plate_marks(tmp_path)
    run = _run(marks, tmp_path, "--start", str(COMMITTED), covered=True)
    expect.passing(run, describing="the calibration tool over plate marks")
    surface, below = _whole_bed_figures(run.stdout)
    fitted = _camera_of(json.loads((tmp_path / "calibration.json").read_text(encoding="utf-8")))
    start = _camera_of(json.loads(COMMITTED.read_text(encoding="utf-8")))
    refits = [
        fitting.fit(load(_plate_marks(tmp_path, seed=seed)), (1920, 1080), start).camera
        for seed in coverage.PERTURBATION_SEEDS
    ]
    for printed, z in ((surface, 0.0), (below, -40.0)):
        points = _bed_grid(z)
        largest = max(
            float(np.linalg.norm(refit.project(points) - fitted.project(points), axis=1).max())
            for refit in refits
        )
        expect.truth(
            abs(printed - largest) <= 0.005,
            describing=f"the printed {printed} px at z = {z} is the largest, {largest} px",
        )


def test_a_refit_that_sees_the_bed_behind_it_disagrees_without_bound() -> None:
    """A grid point an alternative camera sees behind it is an infinite disagreement.

    Driven on the report's own measure rather than through a fit: no marks a
    person can make refit to a camera behind the bed, so no run reaches this
    path, and the report must still say such a camera holds nowhere on it.
    """
    camera = _camera_of(json.loads(COMMITTED.read_text(encoding="utf-8")))
    behind = camera.vector()
    behind[10] -= 1e5
    points = _bed_grid(0.0)
    bed = plate.Bed(250.0, 220.0)
    expect.equal(
        coverage.disagreement_px(camera, [camera], coverage.grid(bed, 0.0)),
        0.0,
        describing="the fit against itself",
    )
    expect.equal(
        coverage.disagreement_px(camera, [camera, Camera.of_vector(behind)], points),
        float("inf"),
        describing="the fit against a camera seeing the whole bed behind it",
    )


@pytest.mark.parametrize(
    ("omitted", "bed", "uncovered"),
    [
        ("front-left corner", BED_MM, "front-left cell"),
        ("front-right corner", BED_MM, "front-right cell"),
        ("back-left corner", BED_MM, "back-left cell"),
        ("back-right corner", BED_MM, "back-right cell"),
        ("centre", BED_MM, "centre cell"),
        # Every mark kept, but a bed named shallower than the plate: the back
        # half of the plate lies off the area named, in no cell of it, and the
        # centre cell, 33 to 67 mm deep, holds no mark.
        (None, "250x100", "centre cell"),
    ],
)
def test_plate_marks_missing_a_required_cell_are_refused_naming_it(
    tmp_path: Path, omitted: str | None, bed: str, uncovered: str
) -> None:
    """Plate marks meeting every minimum but one required cell are refused, naming that cell."""
    marks = _plate_marks(tmp_path, lambda name: name != omitted)
    run = _run(marks, tmp_path, "--start", str(COMMITTED), "--bed-mm", bed, covered=True)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(
        run.stderr, f"reach none of the {uncovered} of a 3 x 3 grid", describing="the refusal"
    )
    expect.truth(
        not (tmp_path / "calibration.json").exists(), describing="no calibration is written"
    )


def test_marks_only_near_the_centre_are_refused_naming_every_uncovered_cell(
    tmp_path: Path,
) -> None:
    """Marks only near the centre are refused naming all four corner cells.

    That is the plate's centre mark alone, and the committed marks, which lie
    near the bed's centre, when no opt-out is passed.
    """
    corners = "front-left, front-right, back-left, back-right cells"
    centre_only = _run(
        _plate_marks(tmp_path, lambda name: name == "centre"),
        tmp_path,
        "--start",
        str(COMMITTED),
        covered=True,
    )
    committed = _run(_fixture_marks(tmp_path), tmp_path, "--start", str(COMMITTED), covered=True)
    for run in (centre_only, committed):
        expect.equal(run.returncode, 2, describing="the exit status")
        expect.contains(run.stderr, f"reach none of the {corners}", describing="the refusal")
    expect.truth(
        not (tmp_path / "calibration.json").exists(), describing="no calibration is written"
    )


def test_a_slice_rewritten_between_fits_is_read_again(tmp_path: Path) -> None:
    """The slices a run parses once are read again when a slice has changed on disk."""
    marks = _plate_marks(tmp_path)
    before = fitting.observations(load(marks))
    expect.equal(
        [one.segments.tolist() for one in fitting.observations(load(marks))],
        [one.segments.tolist() for one in before],
        describing="the outlines read twice from one unchanged slice",
    )
    slice_path = tmp_path / "plate.gcode"
    moved = [
        re.sub(r"X([0-9.]+)", lambda word: f"X{float(word.group(1)) + 1:g}", line)
        for line in slice_path.read_text(encoding="utf-8").splitlines()
    ]
    slice_path.write_text("\n".join(moved) + "\n; moved 1 mm along X\n", encoding="utf-8")
    after = fitting.observations(load(marks))
    shift = after[0].segments - before[0].segments
    expect.equal(
        [shift[:, 0].min(), shift[:, 0].max()],
        [1.0, 1.0],
        describing="every outline segment's start, read from the rewritten slice",
    )
