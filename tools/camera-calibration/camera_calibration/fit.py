"""Fit the camera to the marks: shared intrinsics, `k1`, and one bed pose.

Each mark's residual is its pixel distance to the nearest projected segment of
the outline it was marked on, at its frame's bed height. Nothing is known about
the camera beforehand, so the fit starts from a fixed grid of poses looking at
the marked outlines from above the bed's front, fits each against outlines
thinned to a few hundred segments, and refines the best against the whole
outline near each mark. The grid is fixed, so the same marks fit the same
camera.

Marks all lying on a few parts near the bed's middle can leave every start of
that grid in a minimum far from the camera. An earlier calibration of the same
camera is a start already near it, so a fit given one refines it alone, against
the whole outline near each mark.
"""

from __future__ import annotations

import functools
import itertools
import os
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from pathlib import Path

import numpy as np

from camera_calibration import gcode
from camera_calibration.marks import Marks
from camera_calibration.model import Camera, camera_position, rodrigues

COARSE_SEGMENTS = 300
"""The most segments of an outline the starting fits compare a mark with."""

COARSE_EVALUATIONS = 120
"""How long each starting fit runs: long enough to find which start converges."""

FINE_EVALUATIONS = 800
"""How long the refinement runs."""

K1_PRIOR_PX = 10.0
"""What each unit of `k1` costs, in pixels of one extra residual. Marks on parts
near the bed's middle barely see the lens's distortion, so `k1` alone trades
against the focal lengths and the pose along a valley the marks cannot tell
apart; this keeps it at the least distortion the marks ask for, so the edges of
the bed, where few parts are marked, are not left to that trade."""

GRID = tuple(itertools.product((-150.0, 0.0, 150.0), (0.0, 90.0, 180.0, 270.0)))
"""The fixed grid of starts: each sideways offset (mm) of the camera, at each roll (deg)."""

NEAR_PX = 80.0
"""How far from a mark, under the starting fit, an outline segment is still
the edge that mark was put on."""


@dataclass(frozen=True)
class Observation:
    """One marked pixel and the world segments of its outline at its bed height."""

    frame: int
    px: np.ndarray
    segments: np.ndarray


@dataclass(frozen=True)
class Fitted:
    """The fitted camera and how well each mark lies on its outline."""

    camera: Camera
    residuals_px: np.ndarray
    frames: tuple[int, ...]
    matched_mm: np.ndarray
    """Where on the bed (N, 2) each mark's outline lies nearest it, under the fitted camera."""
    grid_start: tuple[float, float] | None
    """The start of `GRID` whose fit was refined, or `None` for a fit refining a given start."""

    @property
    def rms_px(self) -> float:
        """The RMS reprojection error."""
        return float(np.sqrt(np.mean(self.residuals_px**2)))


@functools.cache
def _layers(path: Path, stamp: tuple[int, int]) -> list[gcode.Layer]:
    del stamp  # it keys the cache alone: a slice rewritten since is parsed again
    return gcode.parse(path)


def _parsed(path: Path) -> list[gcode.Layer]:
    """A slice's layers, parsed once for every fit a run makes of the same slice.

    The fit, the verification image and each of the whole-bed report's refits
    read the same slices; parsing one is most of a fit started near its camera.
    """
    try:
        status = path.stat()
    except OSError:
        return gcode.parse(path)  # which says what of the path it could not read
    return _layers(path, (status.st_mtime_ns, status.st_size))


def observations(marks: Marks) -> list[Observation]:
    """Every marked pixel with its outline, lowered by its frame's bed height."""
    parsed: dict[Path, list[gcode.Layer]] = {}
    found = []
    for number, frame in enumerate(marks.frames):
        if frame.gcode not in parsed:
            parsed[frame.gcode] = _parsed(frame.gcode)
        layers = parsed[frame.gcode]
        for curve in frame.curves:
            if not curve.px:
                continue
            world = gcode.outline(layers, curve.what, curve.z)
            world[:, 2] -= frame.zc_mm
            world[:, 5] -= frame.zc_mm
            found.extend(Observation(number, np.array(px, float), world) for px in curve.px)
    return found


@dataclass(frozen=True)
class Pairs:
    """Every mark beside every segment of its outline, laid out to be measured in one pass.

    A fit measures the same marks against the same outlines hundreds of times,
    so the layout is built once: each distinct outline's segments stacked, and
    for each mark in turn the rows of its outline's segments.
    """

    points: np.ndarray
    """Each distinct end of a segment (Q, 3) in the bed frame."""
    start: np.ndarray
    """Each pair's segment's start, a row of `points`."""
    end: np.ndarray
    """Each pair's segment's end, a row of `points`."""
    px: np.ndarray
    """Each pair's mark (2, P), in pixels: every pair's column, then every pair's row."""
    first: np.ndarray
    """Where each mark's run of pairs begins."""

    @classmethod
    def of(cls, observed: list[Observation], outlines: list[np.ndarray]) -> Pairs:
        """The layout of each mark against its own outline, `outlines[i]` being mark i's."""
        rows: dict[int, np.ndarray] = {}
        stacked: list[np.ndarray] = []
        count = 0
        for world in outlines:
            if id(world) not in rows:
                rows[id(world)] = np.arange(count, count + len(world))
                stacked.append(world)
                count += len(world)
        # Neighbouring marks' outlines share segments, and an outline's
        # segments share their ends: each distinct point is projected once.
        world, row = np.unique(np.vstack(stacked), axis=0, return_inverse=True)
        points, end = np.unique(
            np.vstack([world[:, :3], world[:, 3:]]), axis=0, return_inverse=True
        )
        end = end.reshape(-1)
        segment = row.reshape(-1)[np.concatenate([rows[id(one)] for one in outlines])]
        lengths = np.array([len(one) for one in outlines])
        return cls(
            points,
            end[: len(world)][segment],
            end[len(world) :][segment],
            np.repeat(
                np.array([one.px for one in observed]).reshape(-1, 2), lengths, axis=0
            ).T.copy(),
            np.concatenate([[0], np.cumsum(lengths)[:-1]]),
        )

    def residuals(self, camera: Camera) -> np.ndarray:
        """Each mark's pixel distance from the nearest of its outline segments under `camera`.

        A segment either end of which `camera` sees behind it is 1e6 px away.
        """
        # Column by column, which numpy runs faster than along a two-wide axis.
        u, v = self.px[0], self.px[1]
        x, y = camera.project(self.points).T
        ax, ay = x[self.start], y[self.start]
        abx, aby = x[self.end] - ax, y[self.end] - ay
        length = abx * abx + aby * aby
        length[length == 0] = 1e-9
        t = np.clip(((u - ax) * abx + (v - ay) * aby) / length, 0, 1)
        dx, dy = u - (ax + t * abx), v - (ay + t * aby)
        distance = np.sqrt(dx * dx + dy * dy)
        distance[np.isnan(distance)] = 1e6
        return np.minimum.reduceat(distance, self.first)


def residuals_px(
    camera: Camera, observed: list[Observation], outlines: list[np.ndarray]
) -> np.ndarray:
    """Each mark's pixel distance from the nearest of its outline segments."""
    return Pairs.of(observed, outlines).residuals(camera)


def matched_points(
    camera: Camera, observed: list[Observation], outlines: list[np.ndarray]
) -> np.ndarray:
    """Where on the bed (N, 2) each mark lies: the point of its outline nearest it under `camera`.

    The nearest point is found in the image and carried back to the bed along
    its segment, which is straight in the bed and nearly so in the image.
    """
    out = np.empty((len(observed), 2))
    for index, (one, world) in enumerate(zip(observed, outlines, strict=True)):
        a, b = camera.project(world[:, :3]), camera.project(world[:, 3:])
        ab = b - a
        length = (ab * ab).sum(1)
        length[length == 0] = 1e-9
        t = np.clip(((one.px - a) * ab).sum(1) / length, 0, 1)
        distance = np.linalg.norm(a + t[:, None] * ab - one.px, axis=1)
        nearest = int(np.argmin(np.nan_to_num(distance, nan=np.inf)))
        out[index] = world[nearest, :2] + t[nearest] * (world[nearest, 3:5] - world[nearest, :2])
    return out


def _bounds(size: tuple[int, int]) -> tuple[np.ndarray, np.ndarray]:
    # A real lens: positive focal lengths in a wide range, the principal point
    # on the sensor, and a mild single distortion term. Without these, nearest-
    # segment residuals admit mirror-image cameras that fit as well.
    lower = np.array([300.0, 300.0, 0.0, 0.0, -1.0, *([-np.inf] * 6)])
    upper = np.array([4000.0, 4000.0, size[0], size[1], 1.0, *([np.inf] * 6)])
    return lower, upper


def _solve(
    start: Camera,
    observed: list[Observation],
    outlines: list[np.ndarray],
    size: tuple[int, int],
    evaluations: int,
) -> Camera:
    # Imported where a fit solves rather than with this module: SciPy's
    # optimiser takes most of a second to load, and a run refusing its
    # arguments or its marks before any fit should not wait for it.
    from scipy.optimize import least_squares

    pairs = Pairs.of(observed, outlines)

    def residuals(x: np.ndarray) -> np.ndarray:
        marks = np.minimum(pairs.residuals(Camera.of_vector(x)), 1e3)
        return np.append(marks, K1_PRIOR_PX * x[4])

    lower, upper = _bounds(size)
    solved = least_squares(
        residuals,
        np.clip(start.vector(), lower, upper),
        bounds=(lower, upper),
        x_scale="jac",
        max_nfev=evaluations,
    )
    return Camera.of_vector(solved.x)


def _looking_at(
    position: np.ndarray, target: np.ndarray, roll_deg: float, focal: float, size: tuple[int, int]
) -> Camera:
    forward = (target - position) / np.linalg.norm(target - position)
    up = np.array([0.0, 0.0, 1.0])
    right = np.cross(forward, up)
    right /= np.linalg.norm(right)
    down = np.cross(forward, right)
    roll = np.radians(roll_deg)
    rx = np.cos(roll) * right + np.sin(roll) * down
    ry = -np.sin(roll) * right + np.cos(roll) * down
    rotation = np.vstack([rx, ry, forward])
    angle = np.arccos(np.clip((np.trace(rotation) - 1) / 2, -1, 1))
    if angle < 1e-9:
        rvec = np.zeros(3)
    else:
        axis = np.array(
            [
                rotation[2, 1] - rotation[1, 2],
                rotation[0, 2] - rotation[2, 0],
                rotation[1, 0] - rotation[0, 1],
            ]
        )
        norm = np.linalg.norm(axis)
        if norm < 1e-9:  # a half turn: the axis is the matrix's own fixed direction
            values, vectors = np.linalg.eigh((rotation + np.eye(3)) / 2)
            axis = vectors[:, np.argmax(values)]
            norm = 1.0
        rvec = axis / norm * angle
    tvec = -rodrigues(rvec) @ position
    return Camera(focal, focal, size[0] / 2, size[1] / 2, 0.0, rvec, tvec)


def _thinned(world: np.ndarray) -> np.ndarray:
    step = max(1, len(world) // COARSE_SEGMENTS)
    return world[::step]


def near_segments(camera: Camera, one: Observation) -> np.ndarray:
    """The segments of a mark's outline that, under `camera`, lie near enough to be its edge."""
    projected = np.hstack(
        [camera.project(one.segments[:, :3]), camera.project(one.segments[:, 3:])]
    )
    middle = (projected[:, :2] + projected[:, 2:]) / 2
    distance = np.linalg.norm(middle - one.px, axis=1)
    kept = one.segments[np.nan_to_num(distance, nan=1e9) < NEAR_PX]
    return kept if len(kept) >= 3 else one.segments


def fit(
    marks: Marks,
    image_size: tuple[int, int],
    start: Camera | None = None,
    grid: tuple[tuple[float, float], ...] = GRID,
) -> Fitted:
    """Fit the camera to every mark, refining `start` alone when one is given.

    With no `start`, the fit starts from each of `grid`'s starts, `GRID` unless
    a caller names fewer, and refines the best.
    """
    observed = observations(marks)
    if start is not None:
        best, chosen = start, None
    else:
        best, chosen = _best_start(observed, image_size, grid)
    near = [near_segments(best, one) for one in observed]
    refined = _solve(best, observed, near, image_size, FINE_EVALUATIONS)
    near = [near_segments(refined, one) for one in observed]
    final = _solve(refined, observed, near, image_size, FINE_EVALUATIONS)
    return Fitted(
        final,
        residuals_px(final, observed, near),
        tuple(one.frame for one in observed),
        matched_points(final, observed, near),
        chosen,
    )


@functools.cache
def _pool() -> ProcessPoolExecutor:
    """The processes the grid of starts runs in, started once and kept for every fit a run makes."""
    return ProcessPoolExecutor(max_workers=min(len(GRID), os.process_cpu_count() or 1))


def _best_start(
    observed: list[Observation],
    image_size: tuple[int, int],
    grid: tuple[tuple[float, float], ...],
) -> tuple[Camera, tuple[float, float]]:
    """The best of the grid's starts, each fitted against thinned outlines, and which it was."""
    points = np.vstack([one.segments[:, :3] for one in observed])
    target = points.mean(axis=0)
    thinned: dict[int, np.ndarray] = {}
    coarse = [thinned.setdefault(id(one.segments), _thinned(one.segments)) for one in observed]
    # The camera watches the bed from above its front: the starts look at the
    # marked outlines from the front-left, front and front-right, above them,
    # at each quarter turn of roll.
    starts = [
        _looking_at(target + np.array([dx, -150.0, 150.0]), target, roll, 1000.0, image_size)
        for dx, roll in grid
    ]
    # Each start is fitted on its own, so they run side by side, one process
    # each: this grid is most of a fit's time. Each start's result is the one
    # it would reach alone, so the same marks still fit the same camera. The
    # starts are sent only the thinned outlines they are fitted against.
    thin = [
        Observation(one.frame, one.px, outline)
        for one, outline in zip(observed, coarse, strict=True)
    ]
    fitted = list(
        _pool().map(
            _solve,
            starts,
            itertools.repeat(thin),
            itertools.repeat(coarse),
            itertools.repeat(image_size),
            itertools.repeat(COARSE_EVALUATIONS),
        )
    )
    above = [
        at for at, camera in enumerate(fitted) if camera_position(camera)[2] > target[2]
    ] or list(range(len(fitted)))
    best = min(above, key=lambda at: float(np.sum(residuals_px(fitted[at], observed, coarse) ** 2)))
    return fitted[best], grid[best]
