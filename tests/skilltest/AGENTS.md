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
  skill says is no reason to stop.
- **What a turn did is read from where Claude Code keeps it,** its one result
  document and the session transcript under its configuration directory. A live
  test fails a turn reporting no usage, models or commands, so a move of either
  fails the next run rather than passing with an empty record.
- **A case's position and time are data,** in `case.json`; `checks_real_prints` and
  `tests/skilltest-wiring` hold them. `just skilltest-record` folds a run into
  `results/latest.json`.
