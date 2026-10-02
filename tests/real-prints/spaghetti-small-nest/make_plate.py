"""Write the two-stage spaghetti plate as one ASCII STL of axis-aligned boxes.

Stage 1 (small): a short block with a small slab floating 5 mm above it, so a
few layers are laid into the air and land as a small nest of strands.
Stage 2 (large): a taller block with a wide slab floating 8 mm above it, the
same shape that made Obico warn and then pause on 2026-09-27.
"""
import sys

BOXES = {
    # name: (x0, y0, z0, x1, y1, z1)
    "main block": (0, 0, 0, 40, 25, 22),
    "main floating slab (stage 2, large)": (-3, -5, 30, 43, 30, 33),
    "small block": (-38, 5, 0, -18, 20, 5),
    "small floating slab (stage 1, small)": (-36, 6, 10, -20, 19, 11.6),
}
FACES = [(0, 2, 1), (0, 3, 2), (4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4),
         (1, 2, 6), (1, 6, 5), (2, 3, 7), (2, 7, 6), (3, 0, 4), (3, 4, 7)]


def normal(a, b, c):
    u = [b[i] - a[i] for i in range(3)]
    v = [c[i] - a[i] for i in range(3)]
    n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
    length = sum(k * k for k in n) ** 0.5
    return [k / length for k in n]


with open(sys.argv[1], "w", encoding="ascii") as f:
    f.write("solid spaghetti_two_stage\n")
    for x0, y0, z0, x1, y1, z1 in BOXES.values():
        v = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
             (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]
        for tri in FACES:
            a, b, c = (v[i] for i in tri)
            f.write("  facet normal {:.6f} {:.6f} {:.6f}\n    outer loop\n".format(*normal(a, b, c)))
            for p in (a, b, c):
                f.write("      vertex {:.4f} {:.4f} {:.4f}\n".format(*p))
            f.write("    endloop\n  endfacet\n")
    f.write("endsolid spaghetti_two_stage\n")
for name, box in BOXES.items():
    print(f"{name}: {box}")
