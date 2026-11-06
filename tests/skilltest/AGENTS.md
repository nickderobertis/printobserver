# skilltest

The rules the real-print suite is held to, and why. The root `AGENTS.md` says when
it runs.

- **Doubled are only what no test host has:** OctoPrint, the camera and Obico. A
  frame or an event the replay gets wrong reads as a fault in the agent, so a
  failure is read against the replay's realism as well as the skill.
- **The next event stands the moment the harness exits.** The server's scheduled
  check fires on its own clock whenever nothing holds the print, so
  `printobserver-server-turn-launcher` reports the exit synchronously, before the
  server can release it. It is a Rust program so the gate's wiring tests run on
  Windows, and `test-responder` keeps it off what a consumer installs.
- **Nothing a turn can read moves past the next event's moment.** A look stops at
  the last frame before it and answers that again with the job of that same moment,
  in new bytes, as a still scene's frames from a real camera are. A repeated frame
  never comes with advancing progress.
- **What a turn did is read from where Claude Code keeps it,** its one result
  document and the session transcript under its configuration directory. A live
  test fails a turn reporting no usage, models or commands, so a move of either
  fails the next run rather than passing with an empty record.
- **A case's position and time are data,** in `case.json`; `checks_real_prints` and
  `tests/skilltest-wiring` hold them. `just skilltest-record` folds a run into
  `results/latest.json`.
