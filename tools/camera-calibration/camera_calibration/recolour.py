r"""Recolour the layers a camera frame shows between two heights, as a colour change would.

    uv run --project tools/camera-calibration python -m camera_calibration.recolour \\
        --frame FRAME.jpg --calibration CALIBRATION.json --gcode SLICE.gcode \\
        --from-z 50 --to-z 58.8 --color '#FF0000' \\
        --leave '1005,60 1100,60 1100,188 1008,190' --out recolour-FRAME.jpg

A real-print case's frames show one filament throughout. A case about a colour
change is derived from one: every outer wall the slice lays from `--from-z` up to
`--to-z`, the height the frame was taken at, is projected through the camera's
calibration onto the frame, and what lies under it takes the new colour, its
shading kept. The bed has dropped by `--to-z` when the frame was taken, which is
where the projection puts each layer (`camera_calibration.model`).

The projection knows nothing of the toolhead standing over the top layer, so each
`--leave` names a polygon of the frame's pixels, `x,y` corners separated by
spaces, that is left as it was: the toolhead's own silhouette where it hides the
walls behind it.

The output is a pure function of the inputs: the same frame, calibration, slice
and arguments give the same JPEG bytes, which is what lets a test reproduce a
committed frame. Prints the output's path and the pixels recoloured; exits 2,
naming what and why, for an argument or file it cannot use.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from pathlib import Path

import cv2
import numpy as np

from camera_calibration.gcode import GcodeError, Layer, parse
from camera_calibration.model import Camera

COLOR = re.compile(r"#[0-9A-F]{6}")
"""A colour as the slicer writes one on a `;COLOR_CHANGE` line."""

JPEG_QUALITY = 75
"""The output's JPEG quality: the cases' frames are Git LFS objects every suite
run fetches against a bandwidth budget, so a derived one is kept no larger than
its legibility needs."""

SUBPIXEL_BITS = 4
"""The fixed-point bits OpenCV draws the projected walls at."""

WALL_PX = 3
"""How thick one layer's projected outer wall is drawn."""

CLOSE_PX = 7
"""The square that closes the gaps between successive layers' walls."""


class RecolourError(ValueError):
    """An argument or file the recolouring cannot use."""


def camera(path: Path) -> tuple[Camera, tuple[int, int]]:
    """The camera a calibration file states, and the frame size it was fitted to.

    Raises:
        RecolourError: If the file is no calibration this tool reads.
    """
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
        lens, pose = document["intrinsics"], document["bed_pose"]
        size = (int(document["image_size_px"][0]), int(document["image_size_px"][1]))
        stated = Camera(
            float(lens["fx"]),
            float(lens["fy"]),
            float(lens["cx"]),
            float(lens["cy"]),
            float(lens["k"][0]),
            np.array([float(value) for value in pose["rotation_rodrigues"]]),
            np.array([float(value) for value in pose["translation_mm"]]),
        )
    except (OSError, ValueError, KeyError, IndexError, TypeError) as error:
        msg = f"--calibration {path} is no calibration file this tool reads: {error!r}"
        raise RecolourError(msg) from error
    if stated.rvec.shape != (3,) or stated.tvec.shape != (3,):
        msg = f"--calibration {path} states no three-component bed pose"
        raise RecolourError(msg)
    return stated, size


def wall_mask(
    layers: list[Layer], lens: Camera, shape: tuple[int, int], from_z: float, to_z: float
) -> np.ndarray:
    """Where the outer walls laid from `from_z` to `to_z` fall on a frame taken at `to_z`.

    Each wall is projected with the bed dropped by `to_z`, drawn at fixed point so
    the same projection always draws the same pixels, and the bands between
    successive layers closed.
    """
    mask = np.zeros(shape, np.uint8)
    scale = 1 << SUBPIXEL_BITS
    for layer in layers:
        if not (from_z - 1e-6 <= layer.z <= to_z + 1e-6) or not layer.outer:
            continue
        walls = np.asarray(layer.outer, dtype=float)
        ends = np.concatenate([walls[:, :2], walls[:, 2:]])
        world = np.hstack([ends, np.full((len(ends), 1), layer.z - to_z)])
        pixels = lens.project(world)
        for start, end in zip(pixels[: len(walls)], pixels[len(walls) :], strict=True):
            if np.isnan(start).any() or np.isnan(end).any():
                continue
            a = np.round(start * scale).astype(np.int64)
            b = np.round(end * scale).astype(np.int64)
            cv2.line(
                mask,
                (int(a[0]), int(a[1])),
                (int(b[0]), int(b[1])),
                255,
                WALL_PX,
                cv2.LINE_8,
                SUBPIXEL_BITS,
            )
    return cv2.morphologyEx(mask, cv2.MORPH_CLOSE, np.ones((CLOSE_PX, CLOSE_PX), np.uint8))


def recoloured(frame: np.ndarray, mask: np.ndarray, color: str) -> tuple[np.ndarray, int]:
    """The frame with what lies under `mask` in `color`, and how many pixels changed.

    A pixel keeps its own brightness as the new filament's shade: a shadow stays
    dark and a highlight light, so the part reads as one moulded in that colour.
    Integer arithmetic throughout, so no platform's rounding moves a pixel.
    """
    blue, green, red = (int(color[index : index + 2], 16) for index in (5, 3, 1))
    target = np.array([blue, green, red], np.int32)
    pixels = frame.astype(np.int32)
    # Rec. 601 luma, 0 to 255.
    luma = (pixels[..., 2] * 299 + pixels[..., 1] * 587 + pixels[..., 0] * 114) // 1000
    # The shade, in thousandths of the colour: a tenth of it in black, rising by
    # 1.6 times the luma, with white mixed in above three quarters of full luma.
    shade = 100 + luma * 1600 // 255
    shaded = target[None, None, :] * shade[..., None] // 1000
    white = np.maximum(luma - 191, 0)[..., None] * 4
    painted = np.clip(shaded + white, 0, 255).astype(np.uint8)
    chosen = mask > 0
    out = frame.copy()
    out[chosen] = painted[chosen]
    return out, int(chosen.sum())


def _height(text: str) -> float:
    value = float(text)
    if not (math.isfinite(value) and value >= 0):
        msg = f"{text} is not a height in millimetres"
        raise argparse.ArgumentTypeError(msg)
    return value


def _color(text: str) -> str:
    if COLOR.fullmatch(text) is None:
        msg = f"{text!r} is not a colour written #RRGGBB in upper case"
        raise argparse.ArgumentTypeError(msg)
    return text


def _polygon(text: str) -> np.ndarray:
    try:
        corners = [tuple(int(value) for value in corner.split(",")) for corner in text.split()]
    except ValueError:
        corners = []
    if len(corners) < 3 or any(len(corner) != 2 for corner in corners):
        msg = f"{text!r} is not a polygon of three or more `x,y` pixel corners"
        raise argparse.ArgumentTypeError(msg)
    return np.array(corners, np.int32)


def main(argv: list[str] | None = None) -> int:
    """Write the recoloured frame, and say where and how much it recoloured."""
    parser = argparse.ArgumentParser(
        prog="camera_calibration.recolour", description=__doc__.split("\n")[0]
    )
    parser.add_argument("--frame", type=Path, required=True, help="the camera frame, a JPEG")
    parser.add_argument("--calibration", type=Path, required=True, help="the camera's calibration")
    parser.add_argument("--gcode", type=Path, required=True, help="the slice the frame printed")
    parser.add_argument("--from-z", type=_height, required=True, help="the first recoloured layer")
    parser.add_argument("--to-z", type=_height, required=True, help="the height the frame is at")
    parser.add_argument("--color", type=_color, required=True, help="the new colour, #RRGGBB")
    parser.add_argument(
        "--leave",
        type=_polygon,
        action="append",
        default=[],
        help="a polygon of pixels left as they were, 'x,y x,y x,y ...'; repeatable",
    )
    parser.add_argument("--out", type=Path, required=True, help="where the JPEG is written")
    args = parser.parse_args(argv)
    if args.from_z > args.to_z:
        print(
            f"camera_calibration.recolour: --from-z {args.from_z} is above --to-z {args.to_z}",
            file=sys.stderr,
        )
        return 2
    if args.out.suffix.lower() not in (".jpg", ".jpeg"):
        print(f"camera_calibration.recolour: {args.out} is not a .jpg path", file=sys.stderr)
        return 2
    try:
        lens, size = camera(args.calibration)
        frame = cv2.imread(str(args.frame), cv2.IMREAD_COLOR)
        if frame is None:
            msg = f"--frame {args.frame} is no image OpenCV reads"
            raise RecolourError(msg)
        if (frame.shape[1], frame.shape[0]) != size:
            msg = (
                f"--frame {args.frame} is {frame.shape[1]}x{frame.shape[0]}, and the calibration "
                f"was fitted to {size[0]}x{size[1]} frames"
            )
            raise RecolourError(msg)
        mask = wall_mask(parse(args.gcode), lens, frame.shape[:2], args.from_z, args.to_z)
        for polygon in args.leave:
            cv2.fillPoly(mask, [polygon], 0)
        out, changed = recoloured(frame, mask, args.color)
        if changed == 0:
            msg = (
                f"no outer wall from Z {args.from_z} to {args.to_z} mm falls on {args.frame} "
                "outside the polygons left as they were: nothing would change"
            )
            raise RecolourError(msg)
        written, data = cv2.imencode(".jpg", out, [cv2.IMWRITE_JPEG_QUALITY, JPEG_QUALITY])
        # OpenCV answers false only for an encoder it lacks; this build carries
        # libjpeg-turbo, so no test reaches it, and a test that could would have
        # to build OpenCV without JPEG support.
        if not written:
            msg = f"OpenCV could not encode {args.out}"
            raise RecolourError(msg)
        args.out.write_bytes(data.tobytes())
    except (RecolourError, GcodeError) as error:
        print(f"camera_calibration.recolour: {error}", file=sys.stderr)
        return 2
    except OSError as error:
        where = f"{error.filename}: " if error.filename is not None else ""
        print(f"camera_calibration.recolour: {where}{error.strerror or error}", file=sys.stderr)
        return 2
    print(f"recoloured {changed} pixels of {args.frame} to {args.color}; wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
