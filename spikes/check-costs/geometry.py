#!/usr/bin/env python3
"""Generic test geometry for the check-costs spike, written as a binary STL.

One specimen carries every feature a real plate's G-code labels with a
`;TYPE:`: a hollow-able block (perimeters, internal and solid infill, top
skin), an inverted frustum flaring outward at about 60 degrees from vertical
(overhang perimeters), a shelf sticking out of the block's side with nothing
under it (automatic support and its interface), and a beam spanning two
pillars (bridge infill). Every solid is a convex hexahedron; PrusaSlicer unions
overlapping shells of one object per layer.

Usage: geometry.py OUT.stl [HEIGHT_FACTOR]

HEIGHT_FACTOR (default 1) stretches every solid along Z, so a plate reaches
its target print time with the same footprint.
"""

import struct
import sys


def hexahedron(bottom, top):
    """Twelve triangles of the solid between two quads at two heights.

    `bottom` and `top` are (z, [(x, y) * 4]) with the corners counter-clockwise
    seen from above.
    """
    zb, qb = bottom
    zt, qt = top
    b = [(x, y, zb) for x, y in qb]
    t = [(x, y, zt) for x, y in qt]
    faces = [
        (b[0], b[2], b[1]), (b[0], b[3], b[2]),  # bottom, facing down
        (t[0], t[1], t[2]), (t[0], t[2], t[3]),  # top, facing up
    ]
    for i in range(4):
        j = (i + 1) % 4
        faces.append((b[i], b[j], t[j]))
        faces.append((b[i], t[j], t[i]))
    return faces


def rect(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def box(x0, y0, z0, x1, y1, z1):
    return hexahedron((z0, rect(x0, y0, x1, y1)), (z1, rect(x0, y0, x1, y1)))


def specimen():
    tris = []
    # The block: walls, infill, top skin.
    tris += box(0, 0, 0, 40, 30, 18)
    # The flare: an inverted frustum on top, 8 mm out over 5 mm up on every side.
    tris += hexahedron((17.9, rect(4, 4, 36, 26)), (23, rect(-4, -4, 44, 34)))
    # A cap over the flare, so the flare's top is a skin and not the last layer.
    tris += box(-4, -4, 22.9, 44, 34, 26)
    # The shelf: 12 mm out of the block's side, nothing under it.
    tris += box(39.9, 6, 9, 52, 24, 12)
    # The bridge: two pillars and a beam spanning the 16 mm between them.
    tris += box(58, 0, 0, 66, 10, 14)
    tris += box(82, 0, 0, 90, 10, 14)
    tris += box(58, 0, 13.9, 90, 10, 17)
    return tris


def normal(a, b, c):
    ux, uy, uz = (b[i] - a[i] for i in range(3))
    vx, vy, vz = (c[i] - a[i] for i in range(3))
    n = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
    length = sum(v * v for v in n) ** 0.5 or 1.0
    return tuple(v / length for v in n)


def write_stl(path, tris):
    with open(path, "wb") as out:
        out.write(b"printobserver check-costs spike specimen".ljust(80, b" "))
        out.write(struct.pack("<I", len(tris)))
        for a, b, c in tris:
            out.write(struct.pack("<3f", *normal(a, b, c)))
            for v in (a, b, c):
                out.write(struct.pack("<3f", *v))
            out.write(b"\0\0")


def stretched(tris, factor):
    return [tuple((x, y, z * factor) for x, y, z in tri) for tri in tris]


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    factor = float(sys.argv[2]) if len(sys.argv) == 3 else 1.0
    write_stl(sys.argv[1], stretched(specimen(), factor))
