# skilltest

The rules the real-print suite is held to, and why.

- **Doubled are only what no test host has:** OctoPrint, the camera and Obico. A
  frame or an event the replay gets wrong reads as a fault in the agent, so a
  failure is read against the replay's realism as well as the skill.
- **The next event stands the moment the harness exits.** The server's scheduled
  check fires on its own clock whenever nothing holds the print, so
  `printobserver-server-turn-launcher` reports the exit synchronously, before the
  server can release it. It is a Rust program so the gate's wiring tests run on
  Windows, and `test-responder` keeps it off what a consumer installs.
- **Nothing a turn can read moves past the next event's moment.** A look stops at
  the last frame captured before it, whatever the next frame's label, and answers
  that again, in new bytes and stating its capture time, as a still scene's frames
  from a real camera are, while the job goes on at the recording's own rate for the
  seconds the look really waited, short of the next event's position. A frozen
  counter is what a real stall looks like, so a healthy print never shows one; a
  repeated capture time over a moving job is a camera with nothing newer, which the
  skill says is no defect.
- **What a turn did is read from where Claude Code keeps it,** its one result
  document and the session transcript under its configuration directory. A live
  test fails a turn reporting no usage, models or commands, so a move of either
  fails the next run rather than passing with an empty record.
- **A check's calls are the supervisor port's record of them,** each ledger turn's
  `calls`, which `OneHarness` reads off Claude Code's stream-json output. A check
  is acting when the server recorded it asking for a pause, cancel, resume or any
  `set-*`, else quiet; its requests run until the next harness run starts, because
  a turn that acknowledged nothing by command is acknowledged from its answer once
  its harness exits. A check that runs `look` after acting fails.
- **The replays run the installer's compaction settings,** `idle_compact_tokens`
  and `auto_compact_tokens`, which the record states. A compaction's harness is
  reported to the replay on its own paths and serves no entry, so it moves the
  replay nowhere; the scheduled check due while it runs waits for it to end, by
  manager ruling, so the live run proves the wait and the Rust journeys the
  cancel. Each check's wait, from its event's arrival to its turn's start, and
  every compaction are read from the port's ledger.
- **The probe is one turn of the shipped configuration alone,** through
  `printobserver-server-probe`, with no image; its first call's input is the base
  context every check's input figure is held free of history against. The image
  probe is the same turn with one case's check image attached, composed by
  `printobserver-server-compose`; what an image adds is its first call less the
  text probe's.
- **A case naming a `camera_calibration` is replayed with it** on the camera
  double's stream; the record carries each check's image, read off its prompt's
  `image` line, and the alignment verdict the server kept for the print.
- **Every print's checks get the image the latest code builds.** A live test fails
  any case's check shown the whole frame because no calibration is configured, its
  frames are not the calibration's size, the print's alignment refused it, or its
  landmarks are not found or show the camera moved again; no case is exempt. A
  covered part's inconclusive alignment is not among them.
- **The camera is rechecked as the job passes its first layer.** The live watch
  of the release a case was recorded under decided alignment on the frame it
  took as the first layer went down; the server now judges it on its checks'
  own frames, so the replay stands at the first recorded observation frame past
  that layer, passing over one the case marks `not_at_print_height` — between
  two events while the ended turn still holds the print, or, when it precedes
  the first event, in a start of the server with no check due — and runs the
  operator's `recheck-camera` there. A frame that does not clearly find the
  layer leaves the alignment pending for the checks to judge.
- **A case keeping no frame of its first layer records its alignment instead**
  (`no_first_layer_frame`): its first frame stands far above the layer the check
  scores, where the part's body pulls the score off a right projection. The replay
  writes the passed verdict it records, at the offset its marks lie within, and
  refuses the record on a case that names no calibration or keeps such a frame.
- **An answer of `stop` that asked for no pause fails its decision point.** The
  server refuses it once in the same turn and writes down `agent_stop_unacted`
  when it comes again.
- **A case's position and time are data,** in `case.json`; `checks_real_prints` and
  `tests/skilltest-wiring` hold them. `just skilltest-record` folds a run into
  `results/latest.json`.
- **A start request is an operator's agent's message, not a supervision turn.**
  `starts.py` sends each as one message of `oneharness run --session
  operator-<day>`, the requests of a calendar day continuing one conversation, on
  the shipped harness with the skill installed as a person installs an Agent
  Skill. Its tools are a supervision turn's plus Claude Code's skill tool for that
  one skill, which is the only way that harness offers an installed one.
- **A start request's file and spool are the request's own.** The replay writes a
  small slice naming its material in its settings, and loads a full spool of that
  material. A frame recording no material is one taken after a print, so the
  request asks for the material its sheet suits under the installer's default
  `[sheets.allowed]` table, and the bed is the only thing that stops it.
- **Every conversation's compound `printobserver` commands are in the record**,
  under `compound_commands`, and a live test fails on any: production permissions
  refuse a line chaining another command no rule allows, which costs the action it
  carried. A pipe only into read-only filters, such as `printobserver context | grep
  x`, is not one: the Claude Code release the suite runs runs it under the rule
  allowing the command before it, and `shell_lines.READ_ONLY_FILTERS` names them.
- **A watch line is an outside supervisor's message, not a supervision turn.**
  `outside.py` hands each `outside_supervision` entry to one `oneharness run
  --session outside-<case>` conversation through the operator's agent of
  `starts.py`, while printobserver's turn before it still holds the print, so no
  scheduled check fires before it is answered. Its frame is the case's nearest
  after its position, stated as captured at that moment, as a camera asked then
  would. It passes on a look at the printer with no `--print-id` whose stored frame
  was opened, no look naming the print, and nothing asked that acts.
