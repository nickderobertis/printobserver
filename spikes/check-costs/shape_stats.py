#!/usr/bin/env python3
"""Shape statistics of PrusaSlicer G-code, as JSON.

Usage: shape_stats.py NAME=PATH...

Per file: bytes, lines, layers (`;LAYER_CHANGE`), final Z (the last `;Z:`),
the count of each `;TYPE:` label, the most distinct types in one layer, the
normal-mode `M73 P.. R..` and silent-mode `M73 Q.. S..` markers, and the
slicer's estimated times; plus the densities a generator needs: bytes and lines
per layer, markers per layer and per estimated hour.
"""

import json
import re
import sys

NORMAL = re.compile(rb"^M73 P\d+ R\d+")
SILENT = re.compile(rb"^M73 Q\d+ S\d+")
ESTIMATE = re.compile(rb"^; estimated printing time \((normal|silent) mode\) = (.*)$")


def seconds(text):
    total = 0
    for value, unit in re.findall(r"(\d+)([dhms])", text):
        total += int(value) * {"d": 86400, "h": 3600, "m": 60, "s": 1}[unit]
    return total


def stats(path):
    types, layer_types, most = {}, set(), 0
    layers = lines = size = normal = silent = 0
    final_z = None
    estimates = {}
    with open(path, "rb") as gcode:
        for line in gcode:
            size += len(line)
            lines += 1
            if line.startswith(b";LAYER_CHANGE"):
                layers += 1
                most = max(most, len(layer_types))
                layer_types = set()
            elif line.startswith(b";Z:"):
                final_z = float(line[3:])
            elif line.startswith(b";TYPE:"):
                label = line[6:].strip().decode()
                types[label] = types.get(label, 0) + 1
                layer_types.add(label)
            elif NORMAL.match(line):
                normal += 1
            elif SILENT.match(line):
                silent += 1
            elif match := ESTIMATE.match(line.rstrip(b"\r\n")):
                estimates[match[1].decode()] = seconds(match[2].decode())
    most = max(most, len(layer_types))
    hours = estimates.get("normal", 0) / 3600 or None
    return {
        "bytes": size,
        "lines": lines,
        "layers": layers,
        "final_z_mm": final_z,
        "types": dict(sorted(types.items())),
        "most_types_in_one_layer": most,
        "m73_normal_markers": normal,
        "m73_silent_markers": silent,
        "estimated_s": estimates,
        "bytes_per_layer": round(size / layers, 1) if layers else None,
        "lines_per_layer": round(lines / layers, 1) if layers else None,
        "m73_normal_per_layer": round(normal / layers, 3) if layers else None,
        "m73_normal_per_estimated_hour": round(normal / hours, 1) if hours else None,
        "bytes_per_estimated_hour": round(size / hours) if hours else None,
    }


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    named = dict(arg.split("=", 1) for arg in sys.argv[1:])
    json.dump({name: stats(path) for name, path in named.items()}, sys.stdout, indent=2)
    print()
