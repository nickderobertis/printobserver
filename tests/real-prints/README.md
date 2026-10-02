# Real prints

Frames from the Buddy camera of a Prusa CORE One+ (through go2rtc,
1280x720). They were taken on 2026-09-27 and 28 while printobserver,
self-hosted Obico and OctoPrint supervised real prints. They are kept as
cases for testing what the supervising agent concludes from a picture and
from the events around it. Most prints had a fault planted in the G-code.
One failed on its own.

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
    frames. They are not in this tree; see "Where the G-code and the crops
    are" below.
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
  slice before the fault was injected and the script that made the part. The
  scripts are here; the G-code is not, and where a case had both a clean and a
  faulted slice, `fault.diff` is the unified diff from the clean slice to the
  printed one, so the planted fault stays reviewable.
- `case.json`'s `sources` names the printobserver build each turn ran.

`service-config.toml` is the supervisor's configuration for these prints, with
its credentials redacted. It holds the safety bounds and what each actor may
ask for.

Only Buddy camera and Obico frames are kept here. Phone photos of the printer
were left out.

## What these are for

These cases are the basis for skill tests that give the agent a case's
pictures and simulated event data:
- an Obico alert with its image;
- the `situation` and `context` a turn begins with;
- the frames later `look`s answer;
- for `filament-stall-air-print`, the OctoPrint telemetry of a stall that
  raised no event.

The `printobserver` CLI is mocked, and the tests check that the agent's
commands match the case's `assertions`, including every action
it must never take.

## Where the G-code and the crops are

Every `*.gcode` file and every `crop-*` image these cases were recorded with
remains on branch `fix/windows-supervision-turns` (PR #129), at commit
`45e7fed`, at its original path under `tests/real-prints/`. They were left out
of `main` because the G-code is about 120k lines and the crops are derived.
A `case.json` names one of them as `<branch>@<commit>:<path>`, for example
`fix/windows-supervision-turns@45e7fed:tests/real-prints/fan-cut-bridge/fan-cut-test.gcode`,
and `git show 45e7fed:<path>` reads it from a clone that has fetched that branch.

## The `assertions` block

Every `case.json` carries a top-level `assertions` object beside its prose
fields. It is the one source of what the case expects: the skill tests derive
every assertion from it at run time, and the prose fields
(`expected_agent_action` among them) are a reader's record rather than a
second copy. `case.schema.json` declares its shape.

`assertions.scenarios` lists one or more situations to put the agent in. Each
scenario has:

- `id`, kebab-case and unique within the case, and a one-sentence `summary`;
- `trigger`: `recorded_alert` (an Obico alert this case's history recorded,
  named by `event_id`), `synthetic_alert` (an alert a test raises because
  Obico never did) or `start_request` (a request to start a print);
- `event_image`, the picture handed with the event, or for `start_request`
  the camera's current frame;
- `printer_state`, in the printer contract's `PrinterState` spelling, and
  `detector_warned` and `detector_paused_the_print`, `null` when the trigger
  is not a detection;
- `looks`: what successive `printobserver look` calls answer, in order, the
  last repeating; each may list `arrived_event_ids`, the history events it
  delivers;
- `accept_any_of`, the acceptable outcomes, and `never`, the steps the agent
  must not take. A step is an `operation` (a `printobserver` command,
  hyphenated) and optional `args`, named as the operation's parameters in
  `schemas/printobserver-server/operations.json`.

How a run is judged:

- An outcome is met when its steps occur in order among the `printobserver`
  commands the agent ran. Other commands may come between them. An empty
  outcome asks for nothing.
- A step with `args` matches a command naming each listed parameter with that
  value. Numbers are compared numerically.
- A scenario passes when at least one outcome is met and no command the agent
  ran matches any `never` step.

## What holds them

`just check-repo` runs six deterministic checks over this directory, from
`tools/repo-checks`'s `checks_real_prints`: every `case.json` satisfies
`case.schema.json` (`real-prints-schema`); every file a `case.json` names
exists and every file of a case is named by it (`real-prints-files`); every
`agent-*` image's sha256 is the one its event records in
`printobserver-history.json` (`real-prints-images`); every scenario's events
are in its case's history and its images are files of the case
(`real-prints-scenarios`); every step is an operation of
`schemas/printobserver-server/operations.json` with that operation's own
parameters (`real-prints-operations`); and `service-config.toml` parses with
every credential redacted (`real-prints-service-config`).
