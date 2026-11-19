r"""Fit one printer's camera from marks on printed parts, and write its calibration.

    uv run --project tools/camera-calibration python -m camera_calibration \\
        --marks MARKS.json --snapshot-url URL --calibrated-at YYYY-MM-DD \\
        --out CALIBRATION.json --verification-image CHECK.jpg

Prints one line naming the fit's RMS reprojection error, its worst mark, how many
marks and frames it used and the two files written. Exits 2, naming what and why,
for an argument, marks or frames it cannot use, or a file it cannot write.
"""

from __future__ import annotations

import argparse
import base64
import datetime
import json
import math
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from urllib.parse import urlsplit

import cv2
import numpy as np

from camera_calibration import fit as fitting
from camera_calibration.gcode import GcodeError
from camera_calibration.marks import Marks, MarksError, load
from camera_calibration.model import cross_check

SCHEMA_VERSION = 1
PATCH_PX = 48
SEARCH_RADIUS_PX = 48
ALIGNMENT_MAX_OFFSET_PX = 30.0
CROSS_CHECK_PX = 1e-6
"""How far this tool's projection may lie from OpenCV's before the fit is refused."""


class CalibrationError(ValueError):
    """Frames the calibration cannot be cut from."""


@dataclass(frozen=True)
class LandmarkRecord:
    """One chassis landmark, as the calibration file carries it."""

    name: str
    center_px: list[float]
    patch_png_base64: str
    search_radius_px: int


@dataclass(frozen=True)
class IntrinsicsRecord:
    """The lens: focal lengths, principal point and `k1`-`k4`."""

    fx: float
    fy: float
    cx: float
    cy: float
    k: list[float]


@dataclass(frozen=True)
class BedPoseRecord:
    """The bed's pose seen from the camera, in OpenCV's convention."""

    rotation_rodrigues: list[float]
    translation_mm: list[float]


@dataclass(frozen=True)
class FitRecord:
    """How well the fit went."""

    rms_reprojection_px: float
    frames: int
    calibrated_at: str


@dataclass(frozen=True)
class CalibrationRecord:
    """The calibration file, as `schemas/printobserver-panels/camera-calibration.json` holds it."""

    schema_version: int
    motion: str
    image_size_px: list[int]
    intrinsics: IntrinsicsRecord
    bed_pose: BedPoseRecord
    landmarks: list[LandmarkRecord]
    drift_tolerance_px: float
    camera_snapshot_url: str
    alignment_max_offset_px: float
    fit: FitRecord


def _frames(marks: Marks) -> list[np.ndarray]:
    images = []
    for frame in marks.frames:
        image = cv2.imread(str(frame.frame), cv2.IMREAD_COLOR)
        if image is None:
            msg = f"{frame.frame} is not an image this tool can decode"
            raise CalibrationError(msg)
        images.append(image)
    sizes = {(image.shape[1], image.shape[0]) for image in images}
    for frame, image in zip(marks.frames, images, strict=True):
        height, width = image.shape[:2]
        for curve in frame.curves:
            for x, y in curve.px:
                if not (0 <= x < width and 0 <= y < height):
                    msg = f"{frame.frame} is {width}x{height}, and a mark on it is at ({x}, {y})"
                    raise CalibrationError(msg)
    if len(sizes) != 1:
        msg = (
            f"the marked frames are of {len(sizes)} sizes ({sorted(sizes)}); a calibration "
            f"is for one stream at one resolution, so mark frames of one size"
        )
        raise CalibrationError(msg)
    return images


def _landmarks(marks: Marks, images: list[np.ndarray]) -> list[LandmarkRecord]:
    """Each landmark's patch, cut from the per-pixel median of the marked frames."""
    grey = np.median(np.stack([cv2.cvtColor(image, cv2.COLOR_BGR2GRAY) for image in images]), 0)
    grey = grey.astype(np.uint8)
    half = PATCH_PX // 2
    out: list[LandmarkRecord] = []
    height, width = grey.shape
    for landmark in marks.landmarks:
        u, v = (round(value) for value in landmark.center_px)
        inside = half <= u <= width - half and half <= v <= height - half
        patch = grey[v - half : v + half, u - half : u + half] if inside else None
        if patch is None or patch.shape != (PATCH_PX, PATCH_PX):
            msg = f"landmark {landmark.name} at {landmark.center_px} is too near the frame's edge"
            raise CalibrationError(msg)
        ok, png = cv2.imencode(".png", patch)
        # No test reaches this: OpenCV encodes any 8-bit grey patch as PNG, so
        # it would take a broken OpenCV; the refit tests encode every landmark.
        if not ok:
            msg = f"landmark {landmark.name}'s patch could not be encoded"
            raise CalibrationError(msg)
        out.append(
            LandmarkRecord(
                name=landmark.name,
                center_px=[float(u), float(v)],
                patch_png_base64=base64.b64encode(png.tobytes()).decode("ascii"),
                search_radius_px=SEARCH_RADIUS_PX,
            )
        )
    return out


def _verification(
    fitted: fitting.Fitted, marks: Marks, images: list[np.ndarray], path: Path
) -> None:
    """Every frame, its fitted outlines in green and its marks in red, tiled."""
    observed = fitting.observations(marks)
    tiles = []
    for number, image in enumerate(images):
        drawn = image.copy()
        for one in (one for one in observed if one.frame == number):
            world = one.segments
            ends = np.hstack(
                [fitted.camera.project(world[:, :3]), fitted.camera.project(world[:, 3:])]
            )
            for x0, y0, x1, y1 in ends[~np.isnan(ends).any(axis=1)]:
                cv2.line(drawn, (round(x0), round(y0)), (round(x1), round(y1)), (0, 200, 0), 1)
        for one in (one for one in observed if one.frame == number):
            cv2.drawMarker(
                drawn, (round(one.px[0]), round(one.px[1])), (0, 0, 255), cv2.MARKER_CROSS, 18, 2
            )
        tiles.append(cv2.resize(drawn, (960, 540)))
    while len(tiles) % 2:
        tiles.append(np.zeros_like(tiles[0]))
    rows = [np.hstack(tiles[at : at + 2]) for at in range(0, len(tiles), 2)]
    try:
        written = cv2.imwrite(str(path), np.vstack(rows), [cv2.IMWRITE_JPEG_QUALITY, 85])
    except cv2.error as error:
        written = False
        reason = str(error).strip().splitlines()[-1] if str(error).strip() else "OpenCV refused it"
    else:
        reason = "OpenCV wrote nothing"
    if not written:
        msg = f"{path} could not be written: {reason}"
        raise CalibrationError(msg)


def calibration(
    fitted: fitting.Fitted,
    marks: Marks,
    images: list[np.ndarray],
    landmarks: list[LandmarkRecord],
    snapshot_url: str,
    calibrated_at: str,
    drift_tolerance_px: float,
) -> CalibrationRecord:
    """The calibration file's contents."""
    camera = fitted.camera
    height, width = images[0].shape[:2]
    return CalibrationRecord(
        schema_version=SCHEMA_VERSION,
        motion="bed-z",
        image_size_px=[width, height],
        intrinsics=IntrinsicsRecord(
            fx=camera.fx, fy=camera.fy, cx=camera.cx, cy=camera.cy, k=[camera.k1, 0.0, 0.0, 0.0]
        ),
        bed_pose=BedPoseRecord(
            rotation_rodrigues=[float(value) for value in camera.rvec],
            translation_mm=[float(value) for value in camera.tvec],
        ),
        landmarks=landmarks,
        drift_tolerance_px=drift_tolerance_px,
        camera_snapshot_url=snapshot_url,
        alignment_max_offset_px=ALIGNMENT_MAX_OFFSET_PX,
        fit=FitRecord(
            rms_reprojection_px=fitted.rms_px,
            frames=len(set(fitted.frames)),
            calibrated_at=calibrated_at,
        ),
    )


def _date(text: str) -> str:
    try:
        return datetime.date.fromisoformat(text).isoformat()
    except ValueError as error:
        msg = f"--calibrated-at is {text!r}, not a YYYY-MM-DD date"
        raise argparse.ArgumentTypeError(msg) from error


def _positive(text: str) -> float:
    value = float(text)
    if not (math.isfinite(value) and value > 0):
        msg = f"{text} is not a positive number of pixels"
        raise argparse.ArgumentTypeError(msg)
    return value


def main(argv: list[str] | None = None) -> int:
    """Fit, write the calibration and its verification image, and report the fit."""
    parser = argparse.ArgumentParser(prog="camera_calibration", description=__doc__.split("\n")[0])
    parser.add_argument("--marks", type=Path, required=True, help="the marks file")
    parser.add_argument(
        "--snapshot-url", required=True, help="the [camera] snapshot_url the frames were taken from"
    )
    parser.add_argument(
        "--calibrated-at",
        type=_date,
        default=datetime.datetime.now(tz=datetime.UTC).date().isoformat(),
        help="the date the frames were taken (default: today)",
    )
    parser.add_argument("--drift-tolerance-px", type=_positive, default=6.0)
    parser.add_argument("--out", type=Path, required=True, help="where the calibration is written")
    parser.add_argument("--verification-image", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        address = urlsplit(args.snapshot_url)
        # Reading the port refuses one that is no number or past 65535.
        named = address.scheme in ("http", "https") and bool(address.hostname)
        named = named and (address.port is None or address.port > 0)
    except ValueError:
        named = False
    if not named:
        print(f"--snapshot-url {args.snapshot_url!r} is not an http(s) address", file=sys.stderr)
        return 2
    for output in (args.out, args.verification_image):
        if not output.parent.is_dir():
            print(f"camera_calibration: {output.parent} is not a directory", file=sys.stderr)
            return 2
    if args.out.resolve() == args.verification_image.resolve():
        print("camera_calibration: --out and --verification-image name one file", file=sys.stderr)
        return 2
    if args.verification_image.suffix.lower() not in (".jpg", ".jpeg"):
        print(f"camera_calibration: {args.verification_image} is not a .jpg path", file=sys.stderr)
        return 2
    try:
        marks = load(args.marks)
        images = _frames(marks)
        landmarks = _landmarks(marks, images)
        height, width = images[0].shape[:2]
        fitted = fitting.fit(marks, (width, height))
        probe = np.array([[0, 0, 0], [250, 220, -50], [125, 110, -100], [10, 200, 0]], float)
        differs = cross_check(fitted.camera, probe)
        # No test reaches this refusal: it fires only if this tool's projection
        # stopped being OpenCV's fisheye, which would take a code change, and
        # every fit the tests run passes through this same cross-check.
        if differs > CROSS_CHECK_PX:
            msg = f"this tool's projection differs from OpenCV's by {differs} px"
            raise CalibrationError(msg)
        written = calibration(
            fitted,
            marks,
            images,
            landmarks,
            args.snapshot_url,
            args.calibrated_at,
            args.drift_tolerance_px,
        )
        _verification(fitted, marks, images, args.verification_image)
        # Both outputs' directories and the image's format are checked before
        # the fit; a write failing after them is answered by the handlers
        # below, which `test_an_output_that_cannot_be_written_after_the_fit_is_refused`
        # drives with a directory standing at each output path.
        args.out.write_text(json.dumps(asdict(written), indent=2) + "\n", encoding="utf-8")
    except (MarksError, GcodeError, CalibrationError) as error:
        print(f"camera_calibration: {error}", file=sys.stderr)
        return 2
    except OSError as error:
        print(f"camera_calibration: {error}", file=sys.stderr)
        return 2
    print(
        f"fitted {len(fitted.residuals_px)} marks on {len(set(fitted.frames))} frames: "
        f"{fitted.rms_px:.2f} px RMS, worst {fitted.residuals_px.max():.2f} px; "
        f"wrote {args.out} and {args.verification_image}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
