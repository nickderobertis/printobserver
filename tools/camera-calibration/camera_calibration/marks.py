"""The marks file: operator-marked points on printed parts' edges, validated.

It keeps the shape of the frame-panels spike's `marks.json` — a `frames` list,
each frame naming its `case` and `file` and carrying `curves`, each curve a
`what` (`top` or `first`), the height `z` of the outline it lies on and its
marked pixels `px` — and adds what an operator's own frames need said: the
frame's image (`frame`), the slice it printed (`gcode`), both relative to the
marks file, and the bed's height when it was taken (`zc_mm`, the height the
print had reached). `landmarks` names the fixed chassis features the drift check
locates. Sheet corners, holes and printed sheet marks are not anchors: a sheet
sits up to several millimetres apart between prints.
"""

from __future__ import annotations

import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from camera_calibration.gcode import Outline

LEAST_MARKS = 30
"""The fewest part-edge marks a fit is given: the README's minimum."""

LEAST_FRAMES = 5
"""The fewest frames those marks lie on."""

LEAST_HEIGHTS = 3
"""The fewest distinct bed heights the frames were taken at."""

LEAST_HEIGHT_SPAN_MM = 20.0
"""How far apart the lowest and highest of those heights are, at the least."""


class MarksError(ValueError):
    """A marks file the fit cannot use."""


@dataclass(frozen=True)
class Curve:
    """Marks on one planned outline."""

    what: Outline
    z: float
    px: tuple[tuple[float, float], ...]


@dataclass(frozen=True)
class MarkedFrame:
    """One frame's marks."""

    case: str
    file: str
    frame: Path
    gcode: Path
    zc_mm: float
    curves: tuple[Curve, ...]


@dataclass(frozen=True)
class LandmarkSpec:
    """A chassis feature the drift check locates."""

    name: str
    center_px: tuple[float, float]


@dataclass(frozen=True)
class Marks:
    """A whole marks file."""

    frames: tuple[MarkedFrame, ...]
    landmarks: tuple[LandmarkSpec, ...]


def _number(value: object, where: str) -> float:
    number = math.nan
    if isinstance(value, int | float) and not isinstance(value, bool):
        try:
            number = float(value)
        except OverflowError:
            number = math.nan
    if not math.isfinite(number):
        msg = f"{where} is {value!r:.40}, not a finite number"
        raise MarksError(msg)
    return number


def _list(entry: dict[str, Any], key: str, where: str) -> list[Any]:
    value = entry.get(key, [])
    if not isinstance(value, list):
        msg = f"{where}'s `{key}` is {value!r}, not a list"
        raise MarksError(msg)
    return value


def _point(value: object, where: str) -> tuple[float, float]:
    if not isinstance(value, list) or len(value) != 2:
        msg = f"{where} is {value!r}, not an [x, y] pixel"
        raise MarksError(msg)
    return (_number(value[0], where), _number(value[1], where))


def _text(entry: dict[str, Any], key: str, where: str) -> str:
    value = entry.get(key)
    if not isinstance(value, str) or not value:
        msg = f"{where} names no `{key}`"
        raise MarksError(msg)
    return value


def load(path: Path) -> Marks:
    """Read and validate a marks file.

    Raises:
        MarksError: If the file is not JSON of this shape, names a frame or
            slice that is not there, or marks no part edge at all.
    """
    try:
        doc = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        msg = f"{path}: {error}"
        raise MarksError(msg) from error
    if not isinstance(doc, dict) or not isinstance(doc.get("frames"), list):
        msg = f"{path} carries no `frames` list"
        raise MarksError(msg)
    frames = []
    for number, entry in enumerate(doc["frames"]):
        where = f"{path.name} frames[{number}]"
        if not isinstance(entry, dict):
            msg = f"{where} is not an object"
            raise MarksError(msg)
        frame = path.parent / _text(entry, "frame", where)
        gcode = path.parent / _text(entry, "gcode", where)
        for named in (frame, gcode):
            if not named.is_file():
                msg = f"{where} names {named}, which is not a file"
                raise MarksError(msg)
        curves = []
        for index, curve in enumerate(_list(entry, "curves", where)):
            spot = f"{where} curves[{index}]"
            what = curve.get("what") if isinstance(curve, dict) else None
            if not isinstance(what, str) or what not in set(Outline):
                msg = f"{spot} marks neither a `top` nor a `first` outline"
                raise MarksError(msg)
            points = _list(curve, "px", spot)
            curves.append(
                Curve(
                    Outline(what),
                    _number(curve.get("z"), f"{spot} z"),
                    tuple(_point(point, f"{spot} px") for point in points),
                )
            )
        frames.append(
            MarkedFrame(
                _text(entry, "case", where),
                _text(entry, "file", where),
                frame,
                gcode,
                _number(entry.get("zc_mm"), f"{where} zc_mm"),
                tuple(curves),
            )
        )
    _enough(path, frames)
    landmarks = []
    for number, entry in enumerate(_list(doc, "landmarks", path.name)):
        where = f"{path.name} landmarks[{number}]"
        if not isinstance(entry, dict):
            msg = f"{where} is not an object"
            raise MarksError(msg)
        landmarks.append(
            LandmarkSpec(_text(entry, "name", where), _point(entry.get("center_px"), where))
        )
    return Marks(tuple(frames), tuple(landmarks))


def _enough(path: Path, frames: list[MarkedFrame]) -> None:
    """Refuse marks too few, or on too few frames or heights, to fix a camera."""
    marked = [frame for frame in frames if any(curve.px for curve in frame.curves)]
    count = sum(len(curve.px) for frame in marked for curve in frame.curves)
    if count == 0:
        msg = f"{path} marks no part edge: a fit needs points on printed parts' edges"
        raise MarksError(msg)
    heights = sorted({round(frame.zc_mm, 3) for frame in marked})
    if (
        count < LEAST_MARKS
        or len(marked) < LEAST_FRAMES
        or len(heights) < LEAST_HEIGHTS
        or heights[-1] - heights[0] < LEAST_HEIGHT_SPAN_MM
    ):
        msg = (
            f"{path} marks {count} part-edge points on {len(marked)} frames at "
            f"{len(heights)} bed heights spanning {heights[-1] - heights[0]:.1f} mm; a fit "
            f"needs at least {LEAST_MARKS} points on {LEAST_FRAMES} frames at "
            f"{LEAST_HEIGHTS} heights spanning {LEAST_HEIGHT_SPAN_MM:.0f} mm"
        )
        raise MarksError(msg)
