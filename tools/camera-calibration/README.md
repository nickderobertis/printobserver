# camera-calibration

Fits one printer's camera — a fisheye (equidistant) lens with one distortion
term, `k1`, and the bed's pose — from points an operator marks on printed parts'
edges, with no calibration target. It writes the calibration file the service's
`[camera] calibration` key names, whose schema is
`schemas/printobserver-panels/camera-calibration.json`, and a verification
image.

Each mark is matched to the outline the slicer planned at the frame's layer: the
outer walls of the layer at a height (`top`), or every first-layer extrusion,
the brim included (`first`). The bed drops as the print rises, so frames at
several heights give points off one plane, which is what fixes the pose. Sheet
corners, holes and printed sheet marks are not used: a sheet sits up to several
millimetres apart between prints.

## The operator's procedure, for any printer

1. **Take frames with the camera the service uses**, during prints, with
   `printobserver look` and no print id. Note each frame's print height (the
   `Z` the print had reached) and the G-code it was printing. Every frame must
   be the same size, from the same `[camera] snapshot_url`.
2. **Take them at several bed heights.** At least **3 distinct heights**, at
   least 20 mm apart from lowest to highest, on at least **5 frames**; first
   layers are good frames, because a first layer's whole toolpath is an
   outline.
3. **Mark part edges**, not the sheet: points exactly on a printed part's
   outer edge at the top of what is printed (`top`, with that layer's height),
   or on the edge of a first layer or brim (`first`). At least **30 marks** in
   all. **Spread them across the bed, towards its edges and corners**: parts
   near the bed's centre leave the lens's distortion and the pose's tilt poorly
   constrained at the edges (the committed fit's marks lie near the centre, and
   its intrinsics are loosely determined for that reason).
4. **Write the marks file** (shape below), and name chassis landmarks: fixed
   features that never move with the bed or the gantry, with texture — screws,
   brackets, labels. At least two must be visible in every frame for the
   service's drift check to rule; four is better.
5. **Run the fit**:

   ```console
   uv run --project tools/camera-calibration --directory tools/camera-calibration \
     python -m camera_calibration --marks MARKS.json \
     --snapshot-url http://127.0.0.1:1984/api/frame.jpeg?src=buddy \
     --calibrated-at 2026-09-27 --out core-one-2026-09-27.json \
     --verification-image check.jpg
   ```

   It prints the RMS reprojection error and the worst mark. Open the
   verification image: every red mark should sit on its green outline.
6. **Put it in place**: name the file in the service configuration's
   `[camera] calibration` and restart the service, which refuses an invalid
   file at start.
7. **Re-run the whole procedure whenever the camera moves** — knocked, remounted
   or re-aimed. The service's drift check refuses to draw from a calibration
   whose landmarks have moved, and each print's first-layer alignment check
   refuses one whose projection misses the printed first layer, but neither
   repairs it.

## The marks file

```json
{
  "frames": [
    {
      "case": "a name for the print",
      "file": "the frame's file name",
      "frame": "relative/path/to/frame.jpg",
      "gcode": "relative/path/to/print.gcode",
      "zc_mm": 0.4,
      "curves": [{ "what": "top", "z": 0.4, "px": [[848, 165], [999, 72]] }]
    }
  ],
  "landmarks": [{ "name": "rail-end-right", "center_px": [1830, 190] }]
}
```

Paths are relative to the marks file. `zc_mm` is the height the print had
reached when the frame was taken.

## What the fit is held to

The committed calibration, `tests/real-prints/cameras/core-one-2026-09-27.json`,
is this tool's fit of `core-one-2026-09-27.marks.json` beside it: 50 marks on 7
frames of four prints, at heights from 0.2 to 141.6 mm. Its tests refit those
marks from plain copies of the frames and slices and hold the result to:

- an RMS reprojection error of at most **4.0 px** (the committed fit: 3.41 px);
- the committed file, number by number, within **2 px** for `fx`, `fy`, `cx`
  and `cy`, **0.01** for `k1`, **0.002 rad** for each rotation component and
  **0.5 mm** for each translation component, with `k2`–`k4` written as 0.

`alignment_max_offset_px` is written as **30**: on the four verified cases a
healthy first layer scored at most 23 px and every empty bed or a projection
moved 49 px at least 47 px.
