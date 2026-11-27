"""Whether the marks reach the whole bed, and how well a fit holds across it.

Marks lying on parts near the bed's middle fix where those parts project, and
leave the lens's distortion and the pose's tilt free to trade against each other
away from them. So a fit is held to two things beyond its residuals:

- **coverage**: the bed's four corner cells and its centre cell, of a 3 x 3 grid
  over the printable area, each hold at least one matched mark — the point of
  its outline nearest the mark under the fitted camera;
- **the whole-bed report**: over an 11 x 11 grid across the bed's surface, and
  the same grid 40 mm below it, the largest pixel distance between where the fit
  projects a point and where any of its perturbed refits does. Each refit runs
  the same fit over the marks moved by a fixed, seeded fraction of a pixel, so
  equally good minima the marks cannot tell apart show up as a large figure.
"""

from __future__ import annotations

from dataclasses import replace

import numpy as np

from camera_calibration.marks import Curve, Marks
from camera_calibration.model import Camera
from camera_calibration.plate import Bed

CELLS = 3
"""The grid the printable area is cut into, along each side."""

REQUIRED_CELLS = {
    (0, 0): "front-left",
    (CELLS - 1, 0): "front-right",
    (0, CELLS - 1): "back-left",
    (CELLS - 1, CELLS - 1): "back-right",
    (CELLS // 2, CELLS // 2): "centre",
}
"""The cells, by (column from the left, row from the front), that must each hold a matched mark."""

REPORT_GRID = 11
"""The points along each side of the grid the whole-bed report projects."""

REPORT_BELOW_MM = 40.0
"""How far below the bed's surface the report's second grid lies: the bed lowered that far."""

PERTURBATION_PX = 0.01
"""How far a perturbed refit moves each mark: each pixel coordinate's standard deviation."""

PERTURBATION_SEEDS = (1, 2, 3)
"""The seeds of the perturbed refits the whole-bed report compares the fit with."""


def cell_of(point: tuple[float, float], bed: Bed) -> tuple[int, int] | None:
    """The grid cell a bed point lies in, or `None` for a point off the printable area."""
    x, y = point
    width, depth = bed.width_mm, bed.depth_mm
    if not (0 <= x <= width and 0 <= y <= depth):
        return None
    return (min(int(x / width * CELLS), CELLS - 1), min(int(y / depth * CELLS), CELLS - 1))


def uncovered(points: np.ndarray, bed: Bed) -> list[str]:
    """The names of the required cells no bed point (N, 2) lies in, in `REQUIRED_CELLS` order."""
    reached = {cell_of((float(x), float(y)), bed) for x, y in points}
    return [name for cell, name in REQUIRED_CELLS.items() if cell not in reached]


def perturbed(marks: Marks, seed: int) -> Marks:
    """The marks, every pixel moved by a seeded normal draw of `PERTURBATION_PX`."""
    generator = np.random.default_rng(seed)
    frames = []
    for frame in marks.frames:
        curves = []
        for curve in frame.curves:
            moved = np.asarray(curve.px, float).reshape(-1, 2)
            moved = moved + generator.normal(0.0, PERTURBATION_PX, moved.shape)
            curves.append(Curve(curve.what, curve.z, tuple((float(x), float(y)) for x, y in moved)))
        frames.append(replace(frame, curves=tuple(curves)))
    return replace(marks, frames=tuple(frames))


def grid(bed: Bed, z: float) -> np.ndarray:
    """The report's points (`REPORT_GRID` squared, 3) across the bed at world height `z`."""
    xs = np.linspace(0.0, bed.width_mm, REPORT_GRID)
    ys = np.linspace(0.0, bed.depth_mm, REPORT_GRID)
    x, y = np.meshgrid(xs, ys)
    return np.column_stack([x.ravel(), y.ravel(), np.full(x.size, z)])


def disagreement_px(camera: Camera, alternatives: list[Camera], points: np.ndarray) -> float:
    """The largest pixel distance between `camera`'s projection of a point and any alternative's.

    A point either camera sees behind it is infinitely far apart: a camera that
    cannot see the bed holds nowhere on it.
    """
    ours = camera.project(points)
    worst = 0.0
    for other in alternatives:
        apart = np.linalg.norm(other.project(points) - ours, axis=1)
        worst = max(worst, float(np.nan_to_num(apart, nan=np.inf).max()))
    return worst
