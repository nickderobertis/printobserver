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
from dataclasses import dataclass
from pathlib import Path
from typing import Any

OUTLINES = ("top", "first")


class MarksError(ValueError):
    """A marks file the fit cannot use."""


@dataclass(frozen=True)
class Curve:
    """Marks on one planned outline."""

    what: str
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
    if isinstance(value, bool) or not isinstance(value, int | float):
        msg = f"{where} is {value!r}, not a number"
        raise MarksError(msg)
    return float(value)


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
        for index, curve in enumerate(entry.get("curves", [])):
            spot = f"{where} curves[{index}]"
            if not isinstance(curve, dict) or curve.get("what") not in OUTLINES:
                msg = f"{spot} marks neither a `top` nor a `first` outline"
                raise MarksError(msg)
            points = curve.get("px")
            if not isinstance(points, list):
                msg = f"{spot} carries no `px` list"
                raise MarksError(msg)
            curves.append(
                Curve(
                    curve["what"],
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
    if not any(curve.px for frame in frames for curve in frame.curves):
        msg = f"{path} marks no part edge: a fit needs points on printed parts' edges"
        raise MarksError(msg)
    landmarks = tuple(
        LandmarkSpec(
            _text(entry, "name", f"landmarks[{number}]"),
            _point(entry.get("center_px"), f"landmarks[{number}] center_px"),
        )
        for number, entry in enumerate(doc.get("landmarks", []))
        if isinstance(entry, dict)
    )
    return Marks(tuple(frames), landmarks)
