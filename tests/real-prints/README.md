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

In neither print so far did Obico alert, so no agent turn ran. The cases
record what the agent should do if a detector — or the agent looking for
itself — does raise one.
