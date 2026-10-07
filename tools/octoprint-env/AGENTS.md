# octoprint-env

The rules `octoprint_env.py` is held to, and why. The root `AGENTS.md`'s "The
scripted OctoPrint environment" says what this environment is for, which tiers
bracket it and where the virtual printer is unavailable — that last as a block
`just check-repo` reads from the root. This file holds what only this script
carries.

- **Two modes, one flag apart.** `--mode virtual` enables OctoPrint's own
  virtual printer and connects to it, which is what the tier drives.
  `--mode serial --device /dev/ttyACM0 --baudrate 115200` connects to a real
  USB device, which is what the printer uses. The two compose the same
  configuration and differ in the connection alone; the script's
  `CONNECTION_KEYS` names exactly which keys that is, and a journey asserts the
  two configurations differ in those and in nothing else.
- **A device is named the way the host names one.** The script's
  `SERIAL_PLATFORMS` is the one table of what each platform calls a serial
  device, where it lists the ones it has, and what lets a user open one. A name
  the host cannot have at all is refused naming the shape it does use, and one
  it can have is opened by the host's own means and refused if it cannot be —
  both before anything is provisioned or started, with the next action in that
  host's words. The Unix shape is deliberately no tighter than the platform's
  own, because a `udev` rule may link a printer under any name.
- **Provisioned, not assumed.** `install` is idempotent and unattended: a
  pinned OctoPrint in a virtual environment of its own (this repository's Python
  is newer than anything OctoPrint supports), the first-run wizard already
  answered so nothing waits on a browser, API authentication left **enabled**,
  and the provisioned key written to a path the script names on its own output.
  Turning authentication off would prove a configuration nobody runs.
- **Started, not raced.** `up` answers only once the instance answers its own
  API *and* reports a connected printer, and `--port auto` takes a free port so
  two runs on one host do not collide. In `--mode virtual` it also uploads
  `tools/octoprint-env/gcode/hold.gcode`, selects it and starts it, and states
  in `HOLD_SECONDS` the minimum that print keeps running for — which is what
  gives the tier something to act on. It does **not** start a print in
  `--mode serial` unless asked: a real printer moves.
- **Diagnosed, not timed out.** Every way starting can fail is one of the
  script's `FAILURE_CLASSES`, reported by name with a next action; anything
  outside that closed set is reported with the underlying error's own text. One
  diagnosed failure path and a bare timeout everywhere else is the shape that
  reads as diagnostics without being any.
