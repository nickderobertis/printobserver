#!/usr/bin/env python3
"""Resolve one system printer profile out of PrusaSlicer's vendor bundle.

PrusaSlicer 2.9.6's command line finds `--printer-profile` but then slices
under `- default FFF -` (no `M73` markers, a 200x200 bed), while the print and
material profiles apply. So the printer profile is resolved here exactly as
the preset bundle resolves it — every `inherits` parent first, in order, then
the section's own keys — and handed to the slicer with `--load`.

Usage: resolve_printer_profile.py VENDOR.ini "PROFILE NAME" OUT.ini
"""

import sys


def sections(path):
    found, name = {}, None
    with open(path, encoding="utf-8") as ini:
        for raw in ini:
            line = raw.rstrip("\n")
            if line.startswith("[") and line.endswith("]"):
                name = line[1:-1]
                found[name] = {}
            elif name and " = " in line and not line.startswith("#"):
                key, value = line.split(" = ", 1)
                found[name][key] = value
            elif name and line.endswith(" =") and not line.startswith("#"):
                found[name][line[:-2]] = ""
    return found


def resolve(all_sections, kind, name):
    own = all_sections[f"{kind}:{name}"]
    merged = {}
    for parent in [p.strip() for p in own.get("inherits", "").split(";") if p.strip()]:
        merged.update(resolve(all_sections, kind, parent))
    merged.update({k: v for k, v in own.items() if k != "inherits"})
    return merged


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    vendor, profile, out = sys.argv[1:]
    resolved = resolve(sections(vendor), "printer", profile)
    resolved.pop("compatible_printers_condition", None)
    with open(out, "w", encoding="utf-8") as ini:
        for key, value in resolved.items():
            ini.write(f"{key} = {value}\n")
