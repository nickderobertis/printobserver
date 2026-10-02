"""Write an ASCII STL of a closed rectangular box, the lace test's part.

Wide along X so it faces the camera, shallow along Y, and tall enough that the
under-extruded band and the recovery above it both have room to show.
"""
import sys

W, D, H = (float(v) for v in sys.argv[1:4])
out = sys.argv[4]

v = [(x, y, z) for z in (0, H) for y in (0, D) for x in (0, W)]
# Two triangles per face, wound counter-clockwise seen from outside.
faces = [
    (0, 2, 3), (0, 3, 1),  # bottom (z=0), normal -z
    (4, 5, 7), (4, 7, 6),  # top, normal +z
    (0, 1, 5), (0, 5, 4),  # front (y=0), normal -y
    (2, 6, 7), (2, 7, 3),  # back, normal +y
    (0, 4, 6), (0, 6, 2),  # left (x=0), normal -x
    (1, 3, 7), (1, 7, 5),  # right, normal +x
]


def normal(a, b, c):
    ux, uy, uz = (b[i] - a[i] for i in range(3))
    vx, vy, vz = (c[i] - a[i] for i in range(3))
    n = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
    length = sum(k * k for k in n) ** 0.5
    return tuple(k / length for k in n)


with open(out, "w", encoding="ascii") as f:
    f.write("solid lace_test_box\n")
    for tri in faces:
        a, b, c = (v[i] for i in tri)
        f.write("  facet normal {:.6f} {:.6f} {:.6f}\n    outer loop\n".format(*normal(a, b, c)))
        for p in (a, b, c):
            f.write("      vertex {:.4f} {:.4f} {:.4f}\n".format(*p))
        f.write("    endloop\n  endfacet\n")
    f.write("endsolid lace_test_box\n")
print(f"wrote {out}: {W} x {D} x {H} mm")
