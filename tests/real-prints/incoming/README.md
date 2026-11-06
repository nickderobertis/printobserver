# Real prints from this PC

Cases for printobserver's skill tests, packaged from what printobserver and the operator's print tool recorded on this PC between 2026-10-07 and 2026-10-31. They are in the format of printobserver's `tests/real-prints` (its `README.md` and `case.schema.json`): one directory per case, a `case.json` with a label for every frame, and an `assertions` block the tests run.

| Case | Frames | What happened | Right action |
|---|---|---|---|
| `bubbler-guard-and-pad-pla-healthy` | 86 | PLA guard and pad, printed cleanly; the agent called support "teeth", the pad's arch a "hook", a slot a "notch", the flare a "second opening", the collar a "lip" | carry on |
| `jar-top-petg-healthy` | 74 | PETG jar top, printed cleanly; the agent called the support ring "part of the design" | carry on |
| `jar-top-wrong-material-first-layer-detach` | 12 | sliced for PLA with PETG loaded; the ring was dragged off the sheet a few layers in | pause (or cancel) |
| `jar-top-wrong-material-detach-spaghetti` | 36 | the same mismatch with a brim; the wall top went porous, then the part detached into spaghetti | pause at the porous top; at the latest at the detachment |
| `petg-spool-jam-air-print` | 49 | the spool jammed at Z 2 and the printer printed air to Z 13; no alert, no turn | pause (or cancel) once the parts stop growing |
| `petg-clip-detach-and-stringing-web` | 77 | a spool clip came loose and tangled; later a PETG web in a bin made Obico warn and pause | keep printing and tell a person about the clip; watch the warning; stop on the pause; watch after the resume |
| `petg-printer-reset-nozzle-on-part` | 61 | Obico's false alarm over an infill floor; then the printer reset with the nozzle on a part and heated it on its own | watch the warning; after the reset, send nothing and tell a person |
| `bed-and-sheet-check` | 51 | the bed before prints and after a few, clear or not, with which sheet is on | start only on a clear bed with the right sheet for the material |

How they were made:
- `agent-*` files are the exact pictures printobserver's agent was given, copied through `printobserver image --image-id <id> --json`. Each one's sha256 matches the one its event records in `printobserver-history.json`.
- `frame-*` files are the operator's own 20-second camera frames, sampled as each case's `frame_sampling` says. The October printobserver build looked only when Obico alerted, so these frames are the only record between alerts.
- `after-*` files are frames taken after the 2026-10-10 reset, from the operator's printobserver#183 evidence.
- Each frame is labelled with what is visible, whether it is healthy, and which part of the print is in view. The labels were made from the frame itself, the G-code's `;Z:` and `;TYPE:` at the print's file position, and the model.
- Every concern the agent raised is in `agent_concerns`, with whether it was a real fault, a design feature, support, overhang, bridge, brim, or a camera or lighting effect.
- The G-code, `provenance.json`, `obico-predictions.json`, `printobserver-history.json` and any `OUTCOME.md` are copied from the run folder. No client config, credential or token is included.
- For this export, the private names and paths in those texts and in each `case.json` (run folders, temporary and absolute paths, user names, the operator's own tool, its commands and its notes) were replaced with neutral words, or a field that held only a path was dropped. Four Obico alerts' `raw` payloads, base64 in two histories, named the Obico server by its LAN address and its snapshot by a URL carrying an access token: each was re-encoded with that host made `obico-server.invalid` and the token taken off, every other byte of the payload as recorded. `verify-export.py` lists every change by kind. Every frame, agent image, G-code, model and `obico-predictions.json` is byte for byte as recorded.
- Every `case.json` passes `case.schema.json`, and every `printobserver-history.json` passes the server's `HistoryAnswer.json`. Every file in a case is named in its `case.json`, every `agent-*` image's sha256 matches its event, and every scenario's events, images and operation parameters exist.

Frames and sampling, case by case:
- `bubbler-guard-and-pad-pla-healthy`, `jar-top-petg-healthy`, `jar-top-wrong-material-first-layer-detach` and `jar-top-wrong-material-detach-spaghetti`: 86, 74, 12 and 36 `agent-*` pictures, every picture the case's history records (`obs` for a periodic observation, `look` for a look the agent took).
- `petg-spool-jam-air-print`: 49 `frame-*` frames of the print's 601, one every 2 minutes for the first 42 minutes and one every 6 minutes after, plus the last.
- `petg-clip-detach-and-stringing-web`: 71 `frame-*` frames of 1658, one every 30 minutes, every 60 s from 09:40 to 10:02, every 2 minutes from 13:50 to 14:31 and every 60 s from 15:04 to 15:12, plus the last; and the 6 `agent-*` pictures its history records.
- `petg-printer-reset-nozzle-on-part`: 53 `frame-*` frames of 1784, one every 30 minutes, every 60 s from 10:35 to 10:45 and every 40 s from 15:55 to the last at 16:10:23; the 4 `agent-*` pictures its history records; and 4 `after-*` frames.
- `bed-and-sheet-check`: 51 bed frames named `<bed>-<sheet>-<date>-<time>.jpg`, each taken just before a print or after one.

What is missing:
- The October plates' 3MF models (Printables designs reworked locally) are no longer on this PC. Their cases keep the G-code, and `provenance.json` records each model's name and sha256.
- No `agent-turn.json`. The turns' transcripts were not reduced and scrubbed for this branch, so only the assessments and requests in `printobserver-history.json` are here.
- The plate prints of 2026-10-07 to 2026-10-10 that finished without a fault are not packaged. Each of their bed frames is in `bed-and-sheet-check`.

Size: the directory is 194.9 MB raw and 152.9 MB of unique blobs. Git stores identical files once: the three jar-top cases share one 3MF and one STL, the two plate cases share one G-code, and a few bed frames are also frames of a case. Every G-code stays uncompressed under its own name.
