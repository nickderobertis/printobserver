r"""The calibration plate: single-layer marks reaching the whole printable area.

    uv run --project tools/camera-calibration --directory tools/camera-calibration \\
        python -m camera_calibration.plate --bed-mm 250x220 --out PLATE.stl

Writes an ASCII STL of nine flat marks, each one layer thick: an L-shaped
corner at each of the printable area's four corners, a bar at each edge's
midpoint and a cross at its centre. Its first layer is the whole print, so every
extrusion the slicer plans is an outline the fit's `first` curves match. Prints
one line naming the file, the marks and the PLA they take. Exits 2, naming what
and why, for a bed it cannot lay the marks on or a file it cannot write.

The plate is laid in the bed's own coordinates, from the printable area's
front-left corner, and its bounding box is centred on the bed, so a slicer that
centres an imported model leaves every mark where this module put it.
"""

from __future__ import annotations

import argparse
import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import NamedTuple

from onebudgetspec_sdk import report

INSET_MM = 5.0
"""How far each mark's outer edge sits inside the printable area's edge, so a
skirt or the nozzle's own margin never runs off the bed."""

ARM_MM = 40.0
"""How long each corner's two arms, each bar and the cross's two bars are."""

WIDTH_MM = 6.0
"""How wide each arm and bar is. The committed CORE One calibration sees about
2.5 px per millimetre at the bed's far corners and about 4 px per millimetre at
its front, so a 6 mm bar is at least 15 px across: two edges a person can mark
apart, with the 0.45 mm extrusion lines inside them under a pixel or two."""

LAYER_MM = 0.2
"""The plate's one layer: the first-layer height it is sliced at."""

PLA_G_PER_CM3 = 1.24
"""PLA's density, which the plate's mass is reckoned at."""

COMMITTED = Path(__file__).resolve().parents[1] / "plates" / "calibration-plate-250x220.stl"
"""The plate committed for a 250 x 220 mm printable area, generated with these defaults."""

LEAST_BED_MM = 2 * (INSET_MM + ARM_MM) + ARM_MM
"""The smallest bed side the marks fit along without the corner, edge and
centre marks overlapping."""


class Bed(NamedTuple):
    """A printable area, in millimetres from its front-left corner."""

    width_mm: float
    depth_mm: float


class PlateError(ValueError):
    """A bed the plate's marks cannot be laid on."""


@dataclass(frozen=True)
class Mark:
    """One flat mark: its name, its outline counter-clockwise, and its faces' fan point.

    Every vertex of the outline is visible from `kernel`, so its faces are fanned from it.
    """

    name: str
    outline: tuple[tuple[float, float], ...]
    kernel: tuple[float, float]


def bed_mm(text: str) -> Bed:
    """A `<W>x<D>` printable area in millimetres, refused unless both sides are positive.

    Raises:
        argparse.ArgumentTypeError: If the text is no `<W>x<D>` of two positive numbers.
    """
    parts = text.lower().split("x")
    try:
        width, depth = (float(part) for part in parts)
    except ValueError:
        width = depth = math.nan
    if not all(math.isfinite(side) and side > 0 for side in (width, depth)):
        msg = f"{text!r} is not a <W>x<D> printable area in millimetres, such as 250x220"
        raise argparse.ArgumentTypeError(msg)
    return Bed(width, depth)


def _counter_clockwise(points: list[tuple[float, float]]) -> tuple[tuple[float, float], ...]:
    area = sum(
        x0 * y1 - x1 * y0
        for (x0, y0), (x1, y1) in zip(points, points[1:] + points[:1], strict=True)
    )
    return tuple(points if area > 0 else points[::-1])


def _corner(name: str, x: float, y: float, sx: float, sy: float) -> Mark:
    """An L whose outer corner is at (x, y), its arms running along `sx` and `sy`."""
    a, w = ARM_MM, WIDTH_MM
    points = [
        (x, y),
        (x + sx * a, y),
        (x + sx * a, y + sy * w),
        (x + sx * w, y + sy * w),
        (x + sx * w, y + sy * a),
        (x, y + sy * a),
    ]
    return Mark(name, _counter_clockwise(points), (x + sx * w / 2, y + sy * w / 2))


def _bar(name: str, x: float, y: float, *, along_x: bool) -> Mark:
    """A bar centred on (x, y), running along x or along y."""
    half_long, half_wide = ARM_MM / 2, WIDTH_MM / 2
    hx, hy = (half_long, half_wide) if along_x else (half_wide, half_long)
    points = [(x - hx, y - hy), (x + hx, y - hy), (x + hx, y + hy), (x - hx, y + hy)]
    return Mark(name, _counter_clockwise(points), (x, y))


def _cross(name: str, x: float, y: float) -> Mark:
    """A cross centred on (x, y), its arms `ARM_MM / 2` long but its back arm `ARM_MM`."""
    a, w = ARM_MM / 2, WIDTH_MM / 2
    back = ARM_MM
    points = [
        (x + w, y - a),
        (x + w, y - w),
        (x + a, y - w),
        (x + a, y + w),
        (x + w, y + w),
        (x + w, y + back),
        (x - w, y + back),
        (x - w, y + w),
        (x - a, y + w),
        (x - a, y - w),
        (x - w, y - w),
        (x - w, y - a),
    ]
    return Mark(name, _counter_clockwise(points), (x, y))


def marks(bed: Bed) -> tuple[Mark, ...]:
    """The plate's nine marks for a printable area `bed` (width, depth) in millimetres.

    Raises:
        PlateError: If a side is too short for the marks to lie apart.
    """
    width, depth = bed.width_mm, bed.depth_mm
    if min(width, depth) < LEAST_BED_MM:
        msg = (
            f"a {width:g} x {depth:g} mm bed is too small for the plate: each side needs at "
            f"least {LEAST_BED_MM:g} mm for its corner, edge and centre marks to lie apart"
        )
        raise PlateError(msg)
    i = INSET_MM
    half = WIDTH_MM / 2
    return (
        _corner("front-left corner", i, i, 1, 1),
        _corner("front-right corner", width - i, i, -1, 1),
        _corner("back-left corner", i, depth - i, 1, -1),
        _corner("back-right corner", width - i, depth - i, -1, -1),
        _bar("front edge", width / 2, i + half, along_x=True),
        _bar("back edge", width / 2, depth - i - half, along_x=True),
        _bar("left edge", i + half, depth / 2, along_x=False),
        _bar("right edge", width - i - half, depth / 2, along_x=False),
        _cross("centre", width / 2, depth / 2),
    )


Vertex = tuple[float, float, float]
Triangle = tuple[Vertex, Vertex, Vertex]


def triangles(mark: Mark) -> list[Triangle]:
    """A closed, outward-facing mesh of one mark, one layer thick."""
    ring = mark.outline
    kx, ky = mark.kernel
    out: list[Triangle] = []
    for (x0, y0), (x1, y1) in zip(ring, ring[1:] + ring[:1], strict=True):
        out.append(((kx, ky, 0.0), (x1, y1, 0.0), (x0, y0, 0.0)))
        out.append(((kx, ky, LAYER_MM), (x0, y0, LAYER_MM), (x1, y1, LAYER_MM)))
        out.append(((x0, y0, 0.0), (x1, y1, 0.0), (x1, y1, LAYER_MM)))
        out.append(((x0, y0, 0.0), (x1, y1, LAYER_MM), (x0, y0, LAYER_MM)))
    return out


def _number(value: float) -> str:
    # `+ 0.0` turns a negative zero positive, so one geometry is one text.
    return f"{round(value, 6) + 0.0:.6f}"


def _normal(triangle: Triangle) -> Vertex:
    (ax, ay, az), (bx, by, bz), (cx, cy, cz) = triangle
    ux, uy, uz = bx - ax, by - ay, bz - az
    vx, vy, vz = cx - ax, cy - ay, cz - az
    nx, ny, nz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
    length = math.sqrt(nx * nx + ny * ny + nz * nz)
    return (nx / length, ny / length, nz / length)


def stl(bed: Bed) -> bytes:
    """The plate for a printable area `bed`, as ASCII STL with LF line endings.

    Raises:
        PlateError: If a side is too short for the marks to lie apart.
    """
    name = f"printobserver-calibration-plate-{bed.width_mm:g}x{bed.depth_mm:g}"
    lines = [f"solid {name}"]
    for mark in marks(bed):
        for triangle in triangles(mark):
            lines.append("  facet normal " + " ".join(_number(n) for n in _normal(triangle)))
            lines.append("    outer loop")
            lines.extend(
                "      vertex " + " ".join(_number(c) for c in vertex) for vertex in triangle
            )
            lines.append("    endloop")
            lines.append("  endfacet")
    lines.append(f"endsolid {name}")
    return ("\n".join(lines) + "\n").encode("ascii")


@dataclass(frozen=True)
class Solid:
    """One connected piece of an STL mesh and how much it encloses."""

    triangles: int
    volume_mm3: float
    centre_mm: tuple[float, float]
    """The middle of the piece's footprint on the bed."""


def solids(text: str) -> list[Solid]:
    """Every connected piece of an ASCII STL mesh, with its enclosed volume, in file order.

    Raises:
        PlateError: If the text is no ASCII STL of whole triangles.
    """
    corners: list[Vertex] = []
    for line in text.splitlines():
        words = line.split()
        if words[:1] == ["vertex"]:
            try:
                x, y, z = (float(word) for word in words[1:])
            except ValueError as error:
                msg = f"an STL vertex line {line.strip()!r} is no three numbers"
                raise PlateError(msg) from error
            corners.append((x, y, z))
    if not corners or len(corners) % 3:
        msg = f"the STL carries {len(corners)} vertices: no mesh of whole triangles"
        raise PlateError(msg)
    found = [(corners[at], corners[at + 1], corners[at + 2]) for at in range(0, len(corners), 3)]
    parent: dict[Vertex, Vertex] = {}

    def root(vertex: Vertex) -> Vertex:
        parent.setdefault(vertex, vertex)
        while parent[vertex] != vertex:
            parent[vertex] = parent[parent[vertex]]
            vertex = parent[vertex]
        return vertex

    for a, b, c in found:
        parent[root(b)] = root(a)
        parent[root(c)] = root(a)
    pieces: dict[Vertex, list[Triangle]] = {}
    for triangle in found:
        pieces.setdefault(root(triangle[0]), []).append(triangle)
    return [
        Solid(
            len(piece),
            sum(_signed_volume(triangle) for triangle in piece),
            _footprint_centre(piece),
        )
        for piece in pieces.values()
    ]


def _footprint_centre(piece: list[Triangle]) -> tuple[float, float]:
    xs = [vertex[0] for triangle in piece for vertex in triangle]
    ys = [vertex[1] for triangle in piece for vertex in triangle]
    return ((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2)


def _signed_volume(triangle: Triangle) -> float:
    (ax, ay, az), (bx, by, bz), (cx, cy, cz) = triangle
    return (ax * (by * cz - bz * cy) - ay * (bx * cz - bz * cx) + az * (bx * cy - by * cx)) / 6


def grams(volume_mm3: float) -> float:
    """The PLA a volume of plate takes."""
    return volume_mm3 / 1000 * PLA_G_PER_CM3


def main(argv: list[str] | None = None) -> int:
    """Write the calibration plate for one printable area."""
    parser = argparse.ArgumentParser(
        prog="camera_calibration.plate", description=__doc__.split("\n")[0]
    )
    parser.add_argument(
        "--bed-mm", type=bed_mm, required=True, help="the printable area, <W>x<D> in millimetres"
    )
    parser.add_argument("--out", type=Path, required=True, help="where the STL is written")
    args = parser.parse_args(argv)
    try:
        written = stl(args.bed_mm)
        args.out.write_bytes(written)
    except PlateError as error:
        print(f"camera_calibration.plate: {error}", file=sys.stderr)
        return 2
    except OSError as error:
        where = f"{error.filename}: " if error.filename is not None else ""
        print(f"camera_calibration.plate: {where}{error.strerror or error}", file=sys.stderr)
        return 2
    pieces = solids(written.decode("ascii"))
    volume = sum(piece.volume_mm3 for piece in pieces)
    print(
        f"wrote {args.out}: {len(pieces)} marks, {volume:.0f} mm3, "
        f"{grams(volume):.2f} g of PLA at a {LAYER_MM:g} mm layer"
    )
    return 0


def mass_main(argv: list[str]) -> int:
    """Report the PLA a plate takes, as the `calibration-plate-filament-grams` budget's figure.

    The figure is the mesh's volume times PLA's density, in grams; the detail
    names the volume, the marks and each mark's volume. Under `onebudgetspec
    check` it is written where that check reads it; elsewhere it is printed.
    """
    parser = argparse.ArgumentParser(
        prog="camera_calibration plate-mass",
        description="Report the PLA a calibration plate takes.",
    )
    parser.add_argument(
        "--stl", type=Path, default=COMMITTED, help="the plate (default: the committed plate)"
    )
    args = parser.parse_args(argv)
    try:
        pieces = solids(args.stl.read_text(encoding="ascii"))
    except (OSError, UnicodeDecodeError, PlateError) as error:
        print(f"camera_calibration plate-mass: {args.stl}: {error}", file=sys.stderr)
        return 2
    volume = sum(piece.volume_mm3 for piece in pieces)
    each = "; ".join(
        f"mark at ({piece.centre_mm[0]:g}, {piece.centre_mm[1]:g}) mm: {piece.volume_mm3:.2f} mm3"
        for piece in pieces
    )
    detail = f"mesh volume {volume:.2f} mm3; {len(pieces)} marks; {each}"
    report(grams(volume), detail)
    print(f"{args.stl}: {grams(volume):.3f} g of PLA; {detail}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
