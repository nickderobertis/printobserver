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
from pathlib import Path

import jsonschema
import numpy as np
import pytest
from camera_calibration import fit as fitting
from camera_calibration.marks import load
from camera_calibration.model import Camera
from repo_checks import expect
from repo_checks.shell import run

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


def _run(marks: Path, directory: Path) -> subprocess.CompletedProcess[str]:
    return run(
        [
            sys.executable,
            "-m",
            "camera_calibration",
            "--marks",
            str(marks),
            "--snapshot-url",
            _snapshot_url(),
            "--calibrated-at",
            "2026-09-27",
            "--out",
            str(directory / "calibration.json"),
            "--verification-image",
            str(directory / "verification.jpg"),
        ],
        cwd=TOOL,
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


def test_every_mark_reprojects_onto_its_outline_within_the_bound(tmp_path: Path) -> None:
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
