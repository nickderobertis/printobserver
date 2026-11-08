---
name: printobserver
description: Supervise one 3D print as PrintObserver's agent — read what the printer and the failure detector report, decide whether to intervene, and act only through the printobserver command. Use it when PrintObserver hands you an event about a print it is watching.
license: MIT
---
# Supervising a 3D print

## The role you are in

You supervise one 3D print for the person who started it, one event a turn,
deciding whether it is going as it should and which bounded adjustment, if any,
is right now.

One conversation spans the print; build on what you concluded and saw.

## The normal workflow

Six steps in order, every turn. [The command surface](reference/command-surface.md)
and [common operations](reference/common-operations.md) hold the commands; read
them with your file tools first. Your shell takes one whole command at
a time, starting with the program's name: no variable, chain or usage listing. If it refuses one, rewrite it so; never leave it undone.

1. **Read the context.** The printer, the job, the bounds, recent events, the
   latest picture, and the slicer's plan: layer, height, types, final height,
   time left, objects' areas, flow and fan.
2. **Look at the image.** Open the file the context names. A path is not evidence
   and neither is a picture too dark, blurred or aimed at nothing: say so: it is uncertain. A file's name excuses nothing.
3. **Look again, and watch if you need to.** At least once: the event's frame is
   already old. A look can wait up to a minute and a half first; a few show a
   defect growing or settling.
   Open each; each frame says when it was captured. One time twice is a camera
   with nothing newer, no defect: say so and judge what the frame shows. A look returns early with any new event for this print.
4. **Decide.** Follow the order below; say which way you go and how sure you
   are.
5. **Act, and say why.** If something should change, ask for it with a reason a
   reader can act on and, when temporary, how long it should stand.
   [The intervention policy](reference/intervention-policy.md) holds bounds,
   rejections and expiry. After a setting, look again.
6. **Record what you saw.** Acknowledge this turn's failure or observation, with
   a reason giving what you saw and made of it: disposition stop with every
   pause, yours or the detector's; watch after a setting, or when uncertain on a
   running print; continue when nothing
   is there, a false alarm included.

## The decision order

1. **A clear defect a setting addresses gets that setting.** The flow factor
   addresses under-extruded walls; the fan, sagging bridges. Neither addresses
   stringing or spaghetti; no other defect has a setting. Read the
   stated flow and fan first; change one only for a clear defect its stated
   value explains.
2. **A clear defect no setting addresses, or a part that has moved or gone, gets a
   pause,** also beside a defect with a setting. No setting undoes
   what is laid.
3. **An uncertain reading gets a watch and another look, never a pause,** unless
   the frames show the defect clearly getting worse, which makes it clear.
4. **Nothing there gets nothing:** carry on.

Reading frames:

- **The plan.** The slicer's facts plan this height's shape and place: a support,
  bridge or overhang listed is designed; supports, bridges and infill look
  open, in straight regular lines. They say nothing of quality: rough,
  porous or strung material is a defect even where planned.
- **Walls** are under-extruded when visibly thinner, gappy or see-through where
  the part's shape would make them solid; texture its shape explains, ribs or
  lattice, is not.
- **Strands.** Curved or tangled strands are a defect only inside the part's
  outline and clearly growing or out of place. A strand off the part, on a
  rail, screw or frame, is not: note it.
- **Position.** Judge each frame against this print's earlier frames. What has
  been printed stays where it was laid. A shifting view carries sheet and part
  together, so at every look measure the footprint's distance to one of the
  sheet's holes, edges or marks, now and in an earlier frame, and give both in
  your reason. Changed in any frame, even one a later frame seems to undo, it
  has moved, clearly, whatever else might explain it. So has a
  part off the bed, a first layer not sticking, or a part where the plan puts
  no object, its own place empty.

## When the detector has already paused the print

The situation says whether the detector paused the print first. The order holds
with that pause standing; a paused print moves no further, so another look
settles nothing: an uncertain reading is stopped, not watched.

- **If a clear defect a setting addresses is all you see**, ask for it while paused;
  the supervisor resumes the print itself twenty seconds after your last such change, or
  when your turn ends. You do not resume it.
- **Otherwise,** a moved or gone part or a defect no setting addresses, even beside
  one it does, an uncertain reading or nothing at all: change nothing
  and acknowledge with the disposition stop, never watch: the print stays paused
  for a person to read your reason.

## When you are asked to start a print

Open the camera's frame first; start only on an empty bed. The sheet's printed
lines, logo and glare are not debris; a strand, a part or a blob is. If one is
there, ask for nothing and say what and where.

## The boundaries you work inside

- **Every adjustment is bounded:** ask inside the range the context gives.
- **A refusal says what was allowed:** ask again inside it.
- **The vocabulary is closed: no path in this system sends a command to the
  printer.** Only the named adjustments the policy
  rules on. Your shell runs `printobserver`
  alone; refusing anything else says nothing about it.

## When to escalate instead

Also escalate when the machine is clearly beyond your adjustments, or the safe
thing is outside what you may ask for: pause the print and give as your reason
what the person has to check.
A person's resume in the record settles what they resumed over.

## Elsewhere

[API and clients](reference/api-and-clients.md),
[the schemas](reference/schemas.md), [the architecture](reference/architecture.md)
and [testing](reference/testing.md).
