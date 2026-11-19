"""The planned outlines a mark is matched to, read from a PrusaSlicer G-code.

A mark on a printed part's edge is compared with the outline the slicer planned
at the frame's layer: the outer walls (`External perimeter`, `Overhang
perimeter`) of the last layer at a height, or every extrusion of the first
layer that lays any — the brim included. Arcs are cut into chords of at most
half a millimetre. The printobserver server reads G-code in its own core; this
offline tool reads it only to fit a camera.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path

import numpy as np

OUTER_WALLS = frozenset({"External perimeter", "Overhang perimeter"})
"""The `;TYPE:` labels a part's outline is laid as."""

CHORD_MM = 0.5
"""The longest chord an arc is cut into."""

_WORD = re.compile(r"([XYZEIJ])(-?\d*\.?\d+)")


class Outline(StrEnum):
    """Which planned outline a mark lies on."""

    TOP = "top"
    """The outer walls of the layer at the mark's height."""
    FIRST = "first"
    """Every extrusion of the first layer laying any, the brim included."""


@dataclass
class Layer:
    """One layer: its height and its extrusion segments."""

    z: float
    segments: list[tuple[float, float, float, float]] = field(default_factory=list)
    outer: list[tuple[float, float, float, float]] = field(default_factory=list)


class GcodeError(ValueError):
    """A file that is not a layered slice."""


def _arc(
    start: tuple[float, float], end: tuple[float, float], offset: tuple[float, float], cw: bool
) -> list[tuple[float, float]]:
    cx, cy = start[0] + offset[0], start[1] + offset[1]
    radius = math.hypot(offset[0], offset[1])
    a0 = math.atan2(start[1] - cy, start[0] - cx)
    a1 = math.atan2(end[1] - cy, end[0] - cx)
    sweep = a1 - a0
    if cw and sweep >= 0:
        sweep -= 2 * math.pi
    if not cw and sweep <= 0:
        sweep += 2 * math.pi
    chords = max(1, math.ceil(abs(sweep) * radius / CHORD_MM))
    points = [
        (
            cx + radius * math.cos(a0 + sweep * k / chords),
            cy + radius * math.sin(a0 + sweep * k / chords),
        )
        for k in range(1, chords)
    ]
    points.append(end)
    return points


def parse(path: Path) -> list[Layer]:
    """Every layer of a slice, with its extrusion segments and outer walls.

    Raises:
        GcodeError: If the file marks no layer.
    """
    layers: list[Layer] = []
    pending = False
    kind = ""
    x = y = e = 0.0
    relative_axes = relative_e = False
    with path.open(encoding="utf-8", errors="replace") as handle:
        for raw in handle:
            line = raw.strip()
            if line.startswith(";"):
                if line == ";LAYER_CHANGE":
                    pending = True
                elif line.startswith(";Z:") and pending:
                    layers.append(Layer(float(line[3:])))
                    pending = False
                elif line.startswith(";TYPE:"):
                    kind = line[6:].strip()
                continue
            code = line.split(";", 1)[0].strip()
            head = code.split(" ", 1)[0]
            match head:
                case "G0" | "G1" | "G2" | "G3":
                    pass
                case "G90" | "G91":
                    relative_axes = head == "G91"
                    continue
                case "M82" | "M83":
                    relative_e = head == "M83"
                    continue
                case "G92":
                    e = float(dict(_WORD.findall(code)).get("E", e))
                    continue
                case _:
                    continue
            words = {k: float(v) for k, v in _WORD.findall(code[len(head) :])}
            if relative_axes:
                nx, ny = x + words.get("X", 0.0), y + words.get("Y", 0.0)
            else:
                nx, ny = words.get("X", x), words.get("Y", y)
            fed = 0.0
            if "E" in words:
                if relative_e or relative_axes:
                    fed = words["E"]
                    e += fed
                else:
                    fed = words["E"] - e
                    e = words["E"]
            arc = head in ("G2", "G3")
            if fed > 0 and layers and (arc or (nx, ny) != (x, y)):
                points = (
                    _arc((x, y), (nx, ny), (words.get("I", 0.0), words.get("J", 0.0)), head == "G2")
                    if arc
                    else [(nx, ny)]
                )
                previous = (x, y)
                for point in points:
                    segment = (previous[0], previous[1], point[0], point[1])
                    layers[-1].segments.append(segment)
                    if kind in OUTER_WALLS:
                        layers[-1].outer.append(segment)
                    previous = point
            x, y = nx, ny
    if not layers:
        msg = f"{path} marks no layer (`;LAYER_CHANGE` with `;Z:`): no slicer's layered output"
        raise GcodeError(msg)
    return layers


def outline(layers: list[Layer], what: Outline, z: float) -> np.ndarray:
    """The planned outline a mark is matched to, as world segments (N, 6).

    `first` is every extrusion of the first layer laying any, the brim included;
    `top` is the outer walls of the last layer at height `z`, or of the walled
    layer nearest it when no layer is at exactly `z` (a frame's height read
    off its file position can fall between layers). Each segment lies at its
    layer's height.

    Raises:
        GcodeError: If the slice has no such layer.
    """
    match what:
        case Outline.FIRST:
            layer = next((one for one in layers if one.segments), None)
            chosen = layer.segments if layer else []
        case Outline.TOP:
            walled = [one for one in layers if one.outer]
            exact = [one for one in walled if abs(one.z - z) < 1e-3]
            layer = (
                exact[-1] if exact else min(walled, key=lambda one: abs(one.z - z), default=None)
            )
            chosen = layer.outer if layer else []
    if layer is None or not chosen:
        msg = f"the slice has no {what} outline at z = {z} mm"
        raise GcodeError(msg)
    flat = np.asarray(chosen, dtype=float)
    height = np.full((len(flat), 1), layer.z)
    return np.hstack([flat[:, :2], height, flat[:, 2:], height])
