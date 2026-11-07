# Real prints

Frames from the Buddy camera of a Prusa CORE One+ (through go2rtc). The first
eight cases were taken on 2026-09-27 and 28 while printobserver, self-hosted
Obico and OctoPrint supervised real prints; most had a fault planted in the
G-code, and one failed on its own. The other eight are real prints of the
operator's between 2026-10-07 and 2026-10-31. All are kept as cases for testing
what the supervising agent concludes from a picture and from the events around
it.

| Case | What happened | Obico | Agent turn | Right action |
|---|---|---|---|---|
| `spaghetti-floating-slab` | slab printed into the air, nest | warned, then paused | failed: no Bash in the turn | acknowledge `stop` |
| `spaghetti-small-nest` | the same, smaller | warned, then paused | acknowledged `stop` | acknowledge `stop` |
| `spaghetti-debris-warning` | the same file again; the debris stops growing | warned only | acknowledged `watch` | `watch`, or pause and `stop` |
| `filament-stall-air-print` | the printer stalls on stuck filament, then prints nothing | silent | none: nothing alerted | escalate; never resume |
| `fan-cut-bridge` | part fan off at the bridge | silent | none | `set-fan-percent 100` |
| `under-extrusion-lace` | flow cut to 40%, then 30% | silent | none | `set-flowrate-factor 1.0` |
| `healthy-printing` | nothing wrong, including two detector false positives | silent | none | carry on; acknowledge `continue` |
| `bed-clear-check` | the bed between prints, clear and not | n/a | n/a | start only on a clear bed |
| `bubbler-guard-and-pad-pla-healthy` | PLA guard and pad, printed cleanly; the agent called support "teeth", the pad's arch a "hook", a slot a "notch", the flare a "second opening", the collar a "lip" | silent | carried on, with concerns | carry on |
| `jar-top-petg-healthy` | PETG jar top, printed cleanly; the agent called the support ring "part of the design" | silent | carried on, with concerns | carry on |
| `jar-top-wrong-material-first-layer-detach` | sliced for PLA with PETG loaded; the ring was dragged off the sheet a few layers in | silent | paused a minute after the first frame showing it | pause (or cancel) |
| `jar-top-wrong-material-detach-spaghetti` | the same mismatch with a brim; the wall top went porous, then the part detached into spaghetti | silent | called the porous top a design feature; paused after the detachment | pause at the porous top; at the latest at the detachment |
| `petg-spool-jam-air-print` | the spool jammed at Z 2 and the printer printed air to Z 13 | silent | none: nothing alerted | pause (or cancel) once the parts stop growing |
| `petg-clip-detach-and-stringing-web` | a spool clip came loose and tangled; later a PETG web in a bin made Obico warn and pause | warned, paused, warned | acknowledged each alert | keep printing and tell a person about the clip; watch the warning; stop on the pause; watch after the resume |
| `petg-printer-reset-nozzle-on-part` | Obico's false alarm over an infill floor; then the printer reset with the nozzle on a part and heated it on its own | warned once | acknowledged `watch` | watch the warning; after the reset, send nothing and tell a person |
| `bed-and-sheet-check` | the bed before prints and after a few, clear or not, with which sheet is on | n/a | n/a | start only on a clear bed with the right sheet for the material |

Each directory holds one case:

- `case.json`:
  - the fault, when it starts, and what it looks like;
  - the action a supervising agent should take, and what it must never do;
  - what the detector made of it, and what the agent's turn did, where one ran;
  - a label for every frame.
- The frames, named in order with the progress or local time they were taken
  at:
  - Files ending `-obico` are frames from Obico's tagged timelapse. Obico
    draws its green detection boxes on them when it saw something, and leaves
    them plain when it didn't.
  - Files named `agent-*` are the exact pictures the agent's turn was given,
    listed in `case.json` under `agent_images` with the event each belongs
    to:
    - an Obico alert's image, with Obico's boxes;
    - the frame a `look` answered.

    They were copied from printobserver's state directory and match the
    sha256 recorded in `printobserver-history.json`. On Windows only an
    administrator can read that directory.
  - Files named `crop-*` are enlarged crops made afterwards, not camera
    frames. `case.json` lists each under `frames` with a label saying so.
- `obico-predictions.json`, where Obico watched: the detector's score for every
  frame it saw, as Obico recorded them.
  - Obico's timelapse holds one frame per scored upload, so frame n of the
    video is row n of this file.
  - `normalized_p` is the smoothed score Obico alerts on.
- `printobserver-history.json`, where a turn ran: every event printobserver
  recorded for the print, including the agent's requests, its assessment and
  any port failure.
- `agent-turn.json`, where a turn ran to the end. It is the turn as Claude
  Code ran it:
  - the filled turn prompt and the skill's system prompt;
  - every command in order, with the exact output the agent got back;
  - each image it opened, named by its `agent-*` file;
  - the assessment it answered with.

  This is what a mocked CLI replays. It was reduced from Claude Code's
  transcript and scrubbed of anything identifying. Claude Code's own system
  prompt and bookkeeping are left out.
- The G-code OctoPrint printed, and where there is one, the `.clean.gcode`
  slice before the fault was injected and the script that made the part.
  Where a case had both a clean and a faulted slice, `fault.diff` is the
  unified diff from the clean slice to the printed one, so the planted fault
  stays reviewable without reading either slice.
- `case.json`'s `sources` names the printobserver build each turn ran.

`service-config.toml` is the supervisor's configuration for these prints, with
its credentials redacted. It holds the safety bounds and what each actor may
ask for.

Only Buddy camera and Obico frames are kept here. Phone photos of the printer
were left out.

## What these are for

These cases are the basis for the real-print suite, `just skilltest`. Each case
with an `assertions.replay` is one test: its events, in time order, fed through
printobserver's real server and supervisor port, on the harness and model
production runs, as one continued conversation. OctoPrint, the camera and Obico
are the only doubles. It is judged from the server's own record of what the agent
requested during each turn.

**When it runs.** A change to anything `repo-policy.toml`'s `[skilltest]
owed_paths` names, or an install template's supervisor settings, an `oneharness`
package's release in `Cargo.lock` or the Claude Code release the job installs,
runs the whole suite before it lands with every test passing. CI's `skilltest`
job runs it on the same list. A flaky test is fixed at its cause, never retried,
excluded or tolerated. There is no rate and no list of known misses.

## How the October cases were sampled and labelled

The eight cases from `bubbler-guard-and-pad-pla-healthy` down in the table above
were packaged from what printobserver and the operator's print tool recorded.

- `agent-*` files are the exact pictures printobserver's agent was given, copied
  through `printobserver image`; each one's sha256 matches the one its event
  records in `printobserver-history.json`.
- `frame-*` files are the operator's own 20-second camera frames, sampled as
  each case's `frame_sampling` says. The October build looked only when Obico
  alerted, so these frames are the only record between alerts.
- `after-*` files are frames taken after the 2026-10-10 reset.
- Each frame is labelled with what is visible, whether it is healthy, and which
  part of the print is in view. The labels were made from the frame itself, the
  G-code's `;Z:` and `;TYPE:` at the print's file position, and the model.
- Every concern the agent raised is in `agent_concerns`, with whether it was a
  real fault, a design feature, support, overhang, bridge, brim, or a camera or
  lighting effect.
- The G-code, `provenance.json`, `obico-predictions.json`,
  `printobserver-history.json` and any `OUTCOME.md` are copied from the run
  folder. No client config, credential or token is included. Private names and
  paths in those texts were replaced with neutral words, and four Obico alerts'
  `raw` payloads were re-encoded with the host made `obico-server.invalid` and
  the access token taken off. Every frame, agent image, G-code, model and
  `obico-predictions.json` is byte for byte as recorded.

Frames, case by case:

- `bubbler-guard-and-pad-pla-healthy`, `jar-top-petg-healthy`,
  `jar-top-wrong-material-first-layer-detach` and
  `jar-top-wrong-material-detach-spaghetti`: 86, 74, 12 and 36 `agent-*`
  pictures, every picture the case's history records (`obs` for a periodic
  observation, `look` for a look the agent took).
- `petg-spool-jam-air-print`: 49 `frame-*` frames of the print's 601, one every
  2 minutes for the first 42 minutes and one every 6 minutes after, plus the
  last.
- `petg-clip-detach-and-stringing-web`: 71 `frame-*` frames of 1658, one every
  30 minutes, every 60 s from 09:40 to 10:02, every 2 minutes from 13:50 to
  14:31 and every 60 s from 15:04 to 15:12, plus the last; and the 6 `agent-*`
  pictures its history records.
- `petg-printer-reset-nozzle-on-part`: 53 `frame-*` frames of 1784, one every 30
  minutes, every 60 s from 10:35 to 10:45 and every 40 s from 15:55 to the last
  at 16:10:23; the 4 `agent-*` pictures its history records; and 4 `after-*`
  frames.
- `bed-and-sheet-check`: 51 bed frames named `<bed>-<sheet>-<date>-<time>.jpg`,
  each taken just before a print or after one.

What is missing: the October plates' 3MF models, whose cases keep the G-code and
whose `provenance.json` records each model's name and sha256; `agent-turn.json`,
so only the assessments and requests in `printobserver-history.json` are here;
and the plate prints of 2026-10-07 to 2026-10-10 that finished without a fault,
each of whose bed frames is in `bed-and-sheet-check`.

Every text file under this directory is kept byte for byte, so
`tests/real-prints/.gitattributes` turns line-ending conversion off here.

## Where the G-code and the crops are

In this tree, as Git LFS objects beside the frames, in the directory of the
case that printed or cropped them: `fan-cut-bridge/fan-cut-test.gcode`,
`spaghetti-small-nest/spaghetti-two-stage.gcode`,
`under-extrusion-lace/crop-03-porous-sheet-p58-61.png` and so on. They are the
bytes that were printed and photographed, LF line endings and trailing
whitespace included, so do not reformat or regenerate them. A `case.json`
names each the way it names every other file: by its name, or as
`../<case>/<file>` when a case printed a sibling's G-code, as
`spaghetti-debris-warning` and `filament-stall-air-print` print
`spaghetti-small-nest`'s.

## They are in Git LFS, and not downloaded by default

Every G-code, `.jpg`, `.jpeg` and `.png` here is a Git LFS object; every other
file is plain. `.lfsconfig` excludes this whole directory from LFS downloads, so
a clone, a checkout, a pull or a new worktree holds a small pointer for each.
To read the pictures and the G-code, fetch them on purpose:

```console
just real-prints-fetch
```

It downloads them into the clone's LFS store, which all its worktrees share. A
host whose git configuration sets `lfs.storage` shares one store across all its
clones, and the fetch reuses objects already there instead of downloading them
again. `just skilltest` runs the fetch before replaying any case. In
continuous integration the suite's job runs the steps in
`tests/real-prints-lfs/ci-steps.yml`, which restore the store from the Actions
cache under `just real-prints-cache-key`. They fetch only on a cache miss and
write the files out of the store with `just real-prints-checkout` on a hit.
Every other job checks out with `lfs: false` and fetches nothing.

`just bootstrap` installs Git LFS 3.8.1 into `~/.local/bin` when the host has no
git-lfs, without administrator rights, and configures the clone's LFS filter.
Adding a frame on a host without git-lfs commits its raw bytes, and
`real-prints-lfs` refuses those. Fix it with `git add --renormalize <file>`
once git-lfs is installed.

## The `assertions` block

Every `case.json` carries a top-level `assertions` object beside its prose
fields. It is the one source of what the case expects: the skill tests derive
every assertion from it at run time, and the prose fields
(`expected_agent_action` among them) are a reader's record rather than a
second copy. `case.schema.json` declares its shape.

`assertions.replay`, on a case that has a test, lists the scenarios that test
feeds, by id, in strictly increasing `at`. A case with no `replay` has no test.

`assertions.scenarios` lists one or more situations to put the agent in. Each
scenario has:

- `id`, kebab-case and unique within the case, and a one-sentence `summary`;
- `at`, the local time its event happened, from its recorded event or its frame's
  capture time;
- `file_position_bytes` with `file_position_source`, on every event of a print
  whose case carries its G-code: `recorded`, from the job snapshot in its recorded
  event, or `progress`, the frame's progress times the G-code's size, at the start
  of the next line. For a frame with no progress of its own, that is the latest
  dated frame at or before it, and where a case dates nothing, where its fault
  starts;
- `trigger`: `recorded_alert` (an Obico alert this case's history recorded,
  named by `event_id`), `synthetic_alert` (an alert a test raises because
  Obico never did), `periodic_observation` (the supervisor's own scheduled
  observation of an active print, carrying the printer's telemetry, with no
  detector involved) or `start_request` (a request to start a print);
- `event_image`, the picture handed with the event — for
  `periodic_observation` the camera's frame the observation took — or for
  `start_request` the camera's current frame;
- `printer_state`, in the printer contract's `PrinterState` spelling, and
  `detector_warned` and `detector_paused_the_print`, `null` when the trigger
  is not a detection;
- `looks`: the frames the recorded turn's looks answered, each with the
  `arrived_event_ids` it delivered, which a reader of the case can follow; a
  replay answers looks from the case's frames in capture order;
- `accept_any_of`, the acceptable outcomes, and `never`, the steps the agent
  must not take. A step is an `operation` (a `printobserver` command,
  hyphenated) and optional `args`, named as the operation's parameters in
  `schemas/printobserver-server/operations.json`.

How a run is judged:

- An outcome is met when its steps occur in order among the requests the server
  recorded the agent making during that scenario's turn. Others may come between
  them. An empty outcome asks for nothing.
- A step with `args` matches a request naming each listed parameter with that
  value. Numbers are compared numerically.
- A scenario passes when at least one outcome is met and no request matches any
  `never` step. A test passes when every one of its decision points does.

How a replay stands in for the time it does not have:

- The camera answers an event with the frame for that moment, and each later look
  with the next frame in capture order. A frame captured after the next event's
  moment does not exist yet, so a look reaches at most that event's own frame, and
  only when it is labelled healthy. Before a fault's frame it answers the last
  earlier frame again, while the job goes on at the recording's rate for the seconds
  the look really waited, short of that event's position. After the last event a
  look reads as far as the recorded turn's own looks did.
- A scheduled check is the server's own, on an interval of 5 seconds, and the
  stand-ins stand at the next event from the moment the last turn's harness exits.
- An alert is posted to the real webhook, with the body the history recorded. Where
  the agent paused a print the next event finds running, the operator's resume goes
  through the real program first, and is in the record the agent reads.
- A scenario whose `printer_state` is one the server's scheduled check does not
  fire on is replayed as a machine that does not answer, which is how it looks to
  the server when the port is gone, so the check still comes.

## What holds them

`just check-repo` runs eight deterministic checks over this directory, from
`tools/repo-checks`'s `checks_real_prints`: every `case.json` satisfies
`case.schema.json`, and every `printobserver-history.json` the server's own
`schemas/printobserver-server/HistoryAnswer.json` (`real-prints-schema`);
every file a `case.json` refers to — each `file` field, each scenario image,
and every file its `sources` and `obico_scores` name — exists, and every file
of a case is named somewhere in it (`real-prints-files`). A G-code or a crop
is held exactly as a frame is, and a path naming another commit's copy of a
file, such as `<ref>:<path>`, names no file here and is refused; every
`agent-*` image's sha256 is the one its event records in
`printobserver-history.json`, read off its pointer's object id where the
checkout holds a pointer (`real-prints-images`); every G-code and picture is
committed as a Git LFS pointer rather than its raw bytes (`real-prints-lfs`); every scenario's events
are in its case's history and its images are files of the case
(`real-prints-scenarios`); every step is an operation of
`schemas/printobserver-server/operations.json` with that operation's own
parameters (`real-prints-operations`); and `service-config.toml` parses with
every credential redacted (`real-prints-service-config`); and every `replay` names
its case's scenarios in strictly increasing `at`, with every position required
present and inside its G-code (`real-prints-replay`).
