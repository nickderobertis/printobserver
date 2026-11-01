# check-costs spike (throwaway, never landed)

This spike measures the cost of four planned changes at realistic levels: the
per-print G-code index, the facts that index adds to the agent's context, a
scheduled check that fetches its frame before its event is appended, and a
detector status read from Obico.

## The one command

From the repository root:

```console
python3 spikes/check-costs/run.py --turns 6 --obico --out /tmp/check-costs.json
```

It prints one JSON summary on stdout, the same shape as `results.json`.
`--turns N` runs N real model turns (at most 6, alternating the two variants),
and `--obico` brings the self-hosted Obico up, measures it and brings it down.
Leave both out and nothing spends model quota or starts Obico. `results.json`
holds the summary of the run the report cites, `--turns 4 --obico`. One earlier
invocation failed before Claude Code opened a session, so the spike spent 4 real
turns and 5 invocations.

What it needs on the host: Docker (for the slicer image and for Obico), cargo,
uv, `gh`, curl, and the scripted OctoPrint. If no OctoPrint instance is recorded,
`run.py` starts it with `just octoprint-up`. Leave it running and stop it with
`just octoprint-down`. go2rtc 1.9.15 and a static ffmpeg are fetched into
`~/.cache/printobserver-check-costs-spike` (`SPIKE_TOOLS_DIR`), outside the tree.

| Piece | What it does |
| --- | --- |
| `geometry.py` | Writes a generic specimen as an STL: a block, an outward flare (overhang), a shelf with nothing under it (automatic support) and a beam between two pillars (bridge). `HEIGHT_FACTOR` stretches it along Z. |
| `slice.sh` | Slices `typical` (3 specimens at ×1.12) and `long` (10 at ×1.6) with PrusaSlicer 2.9.6 and the CORE One HF0.4 / 0.20mm BALANCED / Prusament PLA system profiles, with automatic supports and ASCII G-code, into `.work/` (gitignored). |
| `resolve_printer_profile.py` | Resolves the printer profile from the vendor bundle. 2.9.6's CLI otherwise slices under `- default FFF -`. |
| `shape_stats.py` | The shape statistics. It writes `shape-stats.json`. |
| `index/` | The throwaway index prototype, a crate in its own workspace. It reads a local file or streams through the OctoPrint adapter (`spike_stream_gcode`, a spike-only twin of `read_gcode_to`'s path). |
| `camera_bench.py` | go2rtc serving `ffmpeg:virtual?video=testsrc&size=1920x1080`. It makes 200 sequential fetches, 200 fetches with 4 concurrent fetchers and 8 cold fetches (15 s idle before each). |
| `obico_reads.py` | Signs in to the self-hosted Obico, starts a print, posts a snapshot and times the status reads. |
| `crates/printobserver-server/tests/journeys/spike_context.rs` | The real server's `context` answer for a print part-way through the long plate. |
| `crates/printobserver-server/tests/journeys/spike_check_delay.rs` | The real scheduled-check path at a 1 s interval, against a camera that answers `500 exit status 69` at once on every attempt. |

PrusaSlicer 2.9.6 publishes no Linux AppImage. Its GitHub release carries only
`.exe`, `.dmg` and a Windows `.zip`. `slice.sh` therefore installs Prusa
Research's own Flathub build of 2.9.6 into a local Docker image,
`po-spike-prusaslicer:2.9.6`, the first time it runs.

## Gate fixture recipe

To generate a deterministic gate fixture with the shape of real CORE One output
(PrusaSlicer 2.9.6, 0.20 mm layers), use these figures from `shape-stats.json`:

- **Bytes per line:** 23.5 to 24.0 on sliced plates, 21.8 to 23.2 on the
  recordings under `tests/real-prints/`.
- **Lines per layer:** about 820 on the 3-part 2.5 h plate and about 2,020 on
  the 10-part 8.8 h plate (19.7 KB and 47.5 KB per layer). The recordings run
  100 to 150 lines per layer (2.3 to 3.4 KB). This density scales with the part
  count on the plate.
- **Bytes per estimated hour:** about 1.5 MB on both plates. A 2.5 h plate is
  3.7 MB and 152 k lines. An 8.8 h plate is 13.5 MB and 574 k lines.
- **Layers:** one `;LAYER_CHANGE` followed by `;Z:<mm>` and `;HEIGHT:` per
  layer, 0.2 mm apart. `;HEIGHT:` also recurs inside a layer, 461 times in
  `typical`. The plates have 186 layers (29.2 mm) and 284 layers
  (41.6 mm).
- **`;TYPE:` labels:** 10 kinds. In order of frequency they are `Perimeter`,
  `External perimeter`, `Internal infill`, `Support material`,
  `Support material interface`, `Solid infill`, `Bridge infill`,
  `Overhang perimeter`, `Top solid infill` and `Custom` (2 per file). About 6
  labels appear per layer per part. Any one layer carries at most 6 distinct
  types.
- **`M73` markers:** normal-mode `M73 P<pct> R<min>` appears 1.3 times per
  layer (typical) and 2.2 times per layer (long), which is 99 and 71 per
  estimated hour. Silent-mode `M73 Q<pct> S<min>` appears 1.09 to 1.15 times as
  often as normal mode. The recordings carry about 0.84 to 1.18 normal
  markers per layer.
- **Estimate line:** `; estimated printing time (normal mode) = 2h 29m 53s`
  near the end of the file, and the same for silent mode.
