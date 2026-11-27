# Real slices, as plain fixtures

Plain copies of recorded cases' `PrusaSlicer` G-code, so the core's tests read
real files without the Git LFS objects under `tests/real-prints/`, which no
gate test reads.

- `jar-top-petg.gcode` — `tests/real-prints/jar-top-petg-healthy/3423-TOP-REV F petg.gcode`,
  whole: one labelled object, with support, bridge and overhang moves.
- `seven-object-plate-first-layers.gcode` —
  `tests/real-prints/petg-printer-reset-nozzle-on-part/plate1-side-bins-screws-clip.gcode`
  cut before its seventh `;LAYER_CHANGE` (byte 567,680 of 26 MB): its seven
  `M486`-labelled objects and their first six layers.
- `jar-top-first-layer-detach.gcode` —
  `tests/real-prints/jar-top-wrong-material-first-layer-detach/3423-TOP-REV F.gcode`, whole.
- `jar-top-brim.gcode` —
  `tests/real-prints/jar-top-wrong-material-detach-spaghetti/3423-TOP-REV F brim.gcode`, whole.
- `fan-cut-test.gcode` — `tests/real-prints/fan-cut-bridge/fan-cut-test.gcode`, whole.
- `air-print-plate-three-layers.gcode` —
  `tests/real-prints/petg-spool-jam-air-print/plate2-pan-bin-scraper-clips.gcode`
  (37 MB) cut to three of its layers, each whole and unchanged: lines 1 to 5,245
  (its header, start G-code and first layer, Z 0.2), 22,937 to 32,191 (Z 0.8)
  and 166,943 to 171,464 (Z 5.8), the heights its recorded frames of the parts
  growing and of the parts carried down after extrusion stopped were taken at.
- `two-colour-box.gcode` —
  `tests/real-prints/two-colour-planned-change/two-colour-box.gcode`, whole:
  `PrusaSlicer` 2.9.6's slice with two planned colour changes, as `OctoPrint`
  printed it after Obico's plugin dropped its blank lines and wrote an `M117`
  at each layer, some with CRLF endings. `.gitattributes` keeps it byte for
  byte, because the history's offsets are those bytes.

The panels crate's recorded-frame tests and `tools/camera-calibration`'s tests
read these too, beside the bubbler plate's slice the server's tests carry.
