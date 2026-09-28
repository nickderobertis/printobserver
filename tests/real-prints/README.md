# Real prints

Camera frames from real prints on a Prusa CORE One+ with a fault baked into
the G-code, kept as cases for testing what the supervising agent concludes
from a picture. Every case here is a valid failure: the defect happened and
is visible in the frames labelled `defect`.

Each directory holds one print:

- `case.json` — the fault, when it starts, what it looks like, the action a
  supervising agent should take, what the detector made of it, and a label
  for every frame.
- The frames, named in print order with the progress they were taken at.
  Files named `crop-*` are enlarged crops made afterwards, not camera frames.
- `obico-predictions.json` — the detector's score for every frame it saw,
  as Obico recorded them.

- `printobserver-history.json`, where a turn ran — every event printobserver
  recorded for the print, the agent's requests and its assessment included.

Obico alerted on `spaghetti-small-nest` alone, and that is the one print a
turn ran on. On `fan-cut-bridge` and `under-extrusion-lace` it stayed silent,
so those cases record what the agent should do if a detector — or the agent
looking for itself — does raise an alert.
