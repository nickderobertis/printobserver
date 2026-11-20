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

The panels crate's recorded-frame tests and `tools/camera-calibration`'s tests
read these too, beside the bubbler plate's slice the server's tests carry.
