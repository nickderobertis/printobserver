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

## The calibration plate

`python -m camera_calibration.plate` writes a plate of nine flat marks, each one
layer thick, laid across a printer's whole printable area:

```console
uv run --project tools/camera-calibration --directory tools/camera-calibration \
  python -m camera_calibration.plate --bed-mm 250x220 --out plate.stl
```

- an L-shaped corner at each of the area's four corners,
- a bar at each edge's midpoint,
- a cross at its centre, whose long arm points to the bed's back. Without it
  the plate would look the same turned half a turn, and the fit could not tell
  the bed's front from its back.

Each corner's arms and each bar are 40 mm long and 6 mm wide; the cross is
40 mm across and 60 mm from front to back. 6 mm is at least 15 px across in a
1920x1080 frame of a CORE One's bed, and the whole plate is about 0.8 g of PLA
at a 0.2 mm layer. Its one layer is its whole print, so every extrusion the
slicer plans is an outline the fit's `first` curves match. The plate for a
250 x 220 mm bed (the CORE One's) is committed as
[`plates/calibration-plate-250x220.stl`](plates/calibration-plate-250x220.stl),
which downloads with no checkout from
`https://raw.githubusercontent.com/nickderobertis/printobserver/main/tools/camera-calibration/plates/calibration-plate-250x220.stl`.
`--bed-mm` takes any other printer's printable area.

Slice it as one 0.2 mm layer, in a colour that stands out against the sheet, and
leave it where the slicer places it. The fit reads where each mark is from the
sliced G-code, not from the STL.

## The operator's procedure, for any printer

1. **Print the plate** for your printer's printable area, and leave it on the
   sheet.
2. **Take frames with the camera the service uses**, with `printobserver look`
   and no print id. Lower the bed from the printer's own screen to **five or
   more heights** spanning **at least 20 mm** — 0, 10, 20, 30 and 40 mm, say —
   and take **one frame at each**. Note each frame's height, the `Z` the
   printer's screen shows, as its `zc_mm`. That meets the tool's least **5
   frames** at **3 distinct heights** spanning 20 mm. Every frame must be the
   same size, from the same `[camera] snapshot_url`.
3. **Mark the plate's edges**: points exactly on the outer edge of each mark
   that frame shows, as one `first` curve per frame at the layer's height
   (`z`: 0.2). At least **30 marks** in all. The fit refuses marks that leave
   any of the bed's four corner cells or its centre cell, of a 3 x 3 grid over
   the printable area, with no mark. A mark at the back of the bed that is out
   of frame at the bed's highest is usually in frame once the bed is lowered.
   Marks on printed parts' edges work the same way: `top` with that layer's
   height, or `first` on a first layer or brim. But spread them into the same
   five cells.
4. **Write the marks file** (shape below), naming the sliced plate as each
   frame's `gcode`. Name chassis landmarks too: fixed features that never move
   with the bed or the gantry, with texture — screws, brackets, labels. At
   least two must be visible in every frame for the service's drift check to
   rule; four is better. The tool writes each landmark's `search_radius_px`
   as 96; the service searches every landmark at 96 px whatever a file
   records, so a calibration written at the old 48 needs no refit for it.
5. **Run the fit**, naming the printable area:

   ```console
   uv run --project tools/camera-calibration --directory tools/camera-calibration \
     python -m camera_calibration --marks MARKS.json --bed-mm 250x220 \
     --snapshot-url http://127.0.0.1:1984/api/frame.jpeg?src=buddy \
     --calibrated-at 2026-09-27 --out core-one-2026-09-27.json \
     --verification-image check.jpg
   ```

   It prints the RMS reprojection error and the worst mark. Open the
   verification image: every red mark should sit on its green outline.
6. **Check the whole-bed report**, the fit's second line:

   ```text
   whole bed, 11 x 11 grid: the fit and its 3 refits over marks moved 0.01 px disagree by at most 0.01 px on the bed's surface and 0.01 px 40 mm below it
   ```

   The tool refits the same marks three times, each time with every mark moved
   by a seeded fraction of a pixel, from the start the fit refined: the best of
   its fixed grid of starts, or `--start`. It reports the furthest any refit
   projects a point of an 11 x 11 grid across the bed from where the fit does, on the
   bed's surface and with the bed lowered 40 mm. Marks that fix the camera give
   a figure of a pixel or less. A figure of tens of pixels means the marks
   leave equally good cameras that disagree away from them, and a part printed
   there may be drawn that far off its place. Mark more of the plate's edges,
   in every frame, and fit again.

   The fit starts from a fixed grid of cameras looking at the marks from above
   the bed's front. When every mark lies on a few parts near the bed's middle,
   each of those starts can settle far from the camera: a high RMS, a focal
   length far from the other, or `k1` at its bound. Pass `--start` naming an
   earlier calibration of the same camera, at the same resolution, and the fit
   starts from it alone, and so do its refits. `core-one-2026-10-10.json` was
   fitted that way, from
   `core-one-2026-09-27.json`, and `core-one-2026-10-09.json` from it: one
   camera position, but each fit trustworthy only near its own print's parts.
   `core-one-2026-09-27-1280x720.json`, for the camera's 1280x720 stream of
   27 September and 8 October, was fitted from its own generic starts on the
   27 September marks, then on every mark started from that fit.

   Those files' marks were made before the plate, on parts near the bed's
   middle, and the coverage check refuses them. `--allow-uncovered` refits such
   a marks file anyway. The fit then prints which cells it leaves uncovered, and
   that it holds only near its marks. Use it only to refit an existing marks
   file; a new calibration is made from the plate.
7. **Put it in place**: name the file in the service configuration's
   `[camera] calibration` and restart the service, which refuses an invalid
   file at start.
8. **Re-run the whole procedure whenever the camera moves far** — knocked,
   remounted or re-aimed. A small shift is recovered on its own: once a print's
   first layer is clearly found on a check's frame, its offset becomes that
   print's correction, and later frames are drawn through it while their
   landmarks agree with the frame it was taken on. A first layer more than
   `alignment_max_offset_px` from its projection is never clearly found, and a
   camera whose landmarks move again mid-print is refused until a fresh
   correction is taken, so neither repairs a calibration the camera has left
   behind.

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

- an RMS reprojection error of at most **4 px** (the committed fit: 3.41 px);
- every point of every marked outline projected within **5 px** of where the
  committed file projects it;
- the committed file, number by number, within **80 px** for `fx` and `fy`,
  **50 px** for `cx` and `cy`, **0.02** for `k1`, **0.03 rad** for each
  rotation component and **10 mm** for each translation component, with
  `k2`–`k4` written as 0.

The marks fix where the marked parts project far more tightly than they fix
any one number. Refits whose inputs differ only in floating-point rounding, as
one platform's arithmetic differs from another's, settle in minima at most
0.013 px RMS apart. Those minima differ by up to 62 px in a focal length,
40 px in the principal point, 0.009 in `k1`, 0.020 rad and 7.2 mm, and they
project every marked outline within 3.7 px of each other. The per-number bounds
admit that spread, and the projection bound is what holds a refit to the
committed file. Marks spread towards the bed's edges, as step 3 asks, narrow it.

**The committed calibration holds only near its marks**, which leave the bed's
four corner cells uncovered, so its refit passes `--allow-uncovered`. Its
whole-bed report reads more than 100 px, both on the bed's surface and 40 mm
below it (245 px and 254 px on the host it was first run on): away from the
marked parts, refits whose marks moved 0.01 px disagree by that much. A part
printed far from the marked parts' region may be drawn that far off its place,
and the first-layer alignment check is what refuses to draw such a print.

**What a plate fit is held to.** The tests generate the 250 x 220 mm plate and
project its first-layer outlines through `core-one-2026-09-27.json` onto one
frame at each of five bed heights, 0 to 40 mm. A fit of those marks from the
fixed grid of starts, given no `--start`, projects every point of an 11 x 11
grid on the bed's surface within **10 px** of that calibration. Its whole-bed
report reads under 1 px (0.01 px on the host it was first run on). Marks
leaving out any one corner mark, or the centre mark, or every mark but the
centre one, are refused naming the cells they leave uncovered.

`alignment_max_offset_px` is written as **30**: on the four verified cases a
healthy first layer scored at most 23 px and every empty bed or a projection
moved 49 px at least 47 px.
