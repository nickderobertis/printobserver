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
import subprocess
import sys
import tomllib
from collections.abc import Callable
from pathlib import Path
from typing import Any

import jsonschema
import numpy as np
import pytest
from camera_calibration import fit as fitting
from camera_calibration.marks import load
from camera_calibration.model import Camera

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


# What the README states: the RMS bound a fit of these marks meets, and how far
# a refit may land from the committed file, per number.
RMS_BOUND_PX = 4.0
TOLERANCE = {"focal_px": 2.0, "centre_px": 2.0, "k1": 0.01, "rotation": 2e-3, "translation_mm": 0.5}


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
    marks: Path, directory: Path, *extra: str, url: str | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603 - this interpreter, running this tool, with no shell
        [
            sys.executable,
            "-m",
            "camera_calibration",
            "--marks",
            str(marks),
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
    expect.truth(
        (directory / "verification.jpg").stat().st_size > 0,
        describing="the verification image is written",
    )


def test_the_marks_reproject_within_the_rms_bound(tmp_path: Path) -> None:
    """The committed calibration puts the marks within the RMS bound of their outlines."""
    calibration = json.loads(COMMITTED.read_text(encoding="utf-8"))
    lens, pose = calibration["intrinsics"], calibration["bed_pose"]
    camera = Camera(
        lens["fx"],
        lens["fy"],
        lens["cx"],
        lens["cy"],
        lens["k"][0],
        np.array(pose["rotation_rodrigues"]),
        np.array(pose["translation_mm"]),
    )
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


def test_frames_of_two_sizes_are_refused(tmp_path: Path) -> None:
    """A calibration is for one resolution: marks on frames of two sizes are refused."""
    doc = json.loads(_fixture_marks(tmp_path).read_text(encoding="utf-8"))
    doc["frames"][0]["frame"] = str(FRAMES / "fan-cut-bridge" / "01-healthy-before-fault-p41.jpg")
    marks = tmp_path / "mixed.json"
    marks.write_text(json.dumps(doc), encoding="utf-8")
    run = _run(marks, tmp_path)
    expect.equal(run.returncode, 2, describing="the exit status")
    expect.contains(run.stderr, "of 2 sizes", describing="the refusal")


def _edited(
    directory: Path, change: Callable[[dict[str, Any]], None], name: str = "edited"
) -> Path:
    doc = json.loads(_fixture_marks(directory).read_text(encoding="utf-8"))
    change(doc)
    path = directory / f"{name}.json"
    path.write_text(json.dumps(doc), encoding="utf-8")
    return path


def _first_curve(doc: dict[str, Any]) -> dict[str, Any]:
    return doc["frames"][0]["curves"][0]


def _repeat_marks(doc: dict[str, Any], *, frames: int) -> None:
    """Keep `frames` frames, each curve's marks repeated to stay above the least count."""
    doc["frames"] = doc["frames"][:frames]
    for frame in doc["frames"]:
        for curve in frame["curves"]:
            curve["px"] = curve["px"] * 4


def _set_heights(doc: dict[str, Any], heights: list[float]) -> None:
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
        (lambda doc: _first_curve(doc).update(px=[[1, 2, 3]]), "not an [x, y] pixel"),
        (lambda doc: doc["frames"][0].update(zc_mm=None), "not a finite number"),
        (lambda doc: doc["frames"][0].pop("case"), "names no `case`"),
        (lambda doc: doc.update(frames=doc["frames"][:4]), "a fit needs at least 30 points"),
        (lambda doc: _repeat_marks(doc, frames=4), "on 4 frames"),
        (lambda doc: _set_heights(doc, [0.4] * 7), "at 1 bed heights"),
        (lambda doc: _set_heights(doc, [1.0, 2, 3, 4, 5, 6, 7]), "spanning 6.0 mm"),
        (lambda doc: _first_curve(doc).update(px=[[5000, 10]]), "a mark on it is at"),
        (lambda doc: doc.update(landmarks=["rail"]), "landmarks[0] is not an object"),
        (lambda doc: doc["landmarks"][0].update(center_px=[1915, 5]), "too near the frame's edge"),
    ],
)
def test_marks_the_fit_cannot_use_are_refused_naming_why(
    tmp_path: Path, change: Callable[[dict[str, Any]], None], refusal: str
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
    ],
)
def test_arguments_the_tool_cannot_use_are_refused(
    tmp_path: Path, extra: tuple[str, ...], url: str | None, refusal: str
) -> None:
    """An argument that is no date, tolerance or address is refused with exit 2."""
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
