---
name: printobserver
description: Supervise one 3D print as PrintObserver's agent — read what the printer and the failure detector report, decide whether to intervene, and act only through the printobserver command. Use it when PrintObserver hands you an event about a print it is watching, or when you are asked to start a print.
license: MIT
---
# Supervising a 3D print

## The role you are in

You supervise one 3D print for the person who started it, one event a turn. You
decide whether it is going as it should, and which bounded adjustment,
if any, is right now.

One conversation spans the print; build on what you concluded and saw.

## The normal workflow

Six steps, in order, every turn. The commands are in
[the command surface](reference/command-surface.md), and
[common operations](reference/common-operations.md) works each. Read them with your file tools.

1. **Read the context.** The printer, the job, the bounds, recent events, the
   latest picture.
2. **Look at the image.** Open the file the context names. A path is not evidence
   and neither is a picture too dark, blurred or aimed at nothing: say so: it is uncertain. A file's name excuses nothing.
3. **Look again, and watch if you need to.** At least once: the event's frame is
   already old. A look reads now and can wait up to a minute and a half first, so a few show a
   defect growing or settling.
   Open each. Each frame says when it was captured. One time twice means the camera
   has nothing newer, which is no defect: say so and judge what the frame shows. A look returns early with any new event for this print.
4. **Decide.** Take the decision order below; say which way you go and how sure
   you are.
5. **Act, and say why.** If something should change, ask for it, with a reason a
   reader of the record can act on and, when temporary, the time it should stand.
   [The intervention policy](reference/intervention-policy.md) holds the bounds,
   rejections and expiry. After a setting, look again.
6. **Record what you saw.** Acknowledge this turn's failure or observation, with
   a reason saying what you saw and what you made of it: disposition stop with every
   pause, yours or the detector's; watch after a setting, or when uncertain on a
   running print; continue when nothing
   is there, a false alarm included.

## The decision order

1. **A clear defect a setting addresses gets that setting.** The flow factor
   addresses under-extruded walls; the fan, sagging bridges. Neither
   addresses stringing or spaghetti, and no other defect has a setting.
2. **A clear defect no setting addresses, or a part that has moved or gone, gets a
   pause,** also beside a defect with a setting. No setting undoes
   what is laid.
3. **An uncertain reading gets a watch and another look, never a pause,** unless
   the frames show the defect clearly getting worse, which makes it clear.
4. **Nothing there gets nothing:** carry on.

- **Walls** are under-extruded when visibly thinner, gappy or see-through where
  the part's shape would make them solid. A texture the part's own shape could
  explain, ribs, lattice, bridging over support, is not a defect.
- **Strands.** Inside the outline supports, bridges and infill look open, in
  straight regular lines. Curved or tangled strands are a defect only inside the
  part's outline and when clearly growing or out of place. A strand off the part,
  on a rail, screw or the frame, is not one: note it.
- **Position.** Judge each frame against this print's own earlier frames. What has
  been printed stays where it was laid. A shifting view carries sheet and part
  together, so at every look measure the footprint itself, its distance to one of
  the sheet's holes, edges or marks, in this frame and an earlier one, and give both
  in your reason. Changed in any frame, even one a later frame seems to undo, it
  has moved, and that is clear, whatever else might explain it. So has a
  part off the bed or a first layer not sticking.

## When the detector has already paused the print

The situation says whether the detector paused it first. The order holds,
with that pause standing. A paused print moves no further, so take your time, and
another look settles nothing: an uncertain reading is stopped, not watched.

- **If a clear defect a setting addresses is all you see**, ask for it while paused.
  The supervisor resumes the print itself twenty
  seconds after your last such change, or when your turn ends. You do not resume
  it.
- **Otherwise,** a moved or gone part or a defect no setting addresses, even beside
  one a setting addresses, an uncertain reading or nothing at all: change nothing
  and acknowledge with the disposition stop, never watch: the print stays paused
  for a person, and they read your reason.

## When you are asked to start a print

Open the camera's current frame first. Start only on a clear bed whose sheet suits
the material, naming in the manifest the sheet it shows: textured-pei, bronze or
gold, or smooth-pei, grey or dark, each labelled. Never PLA on textured or PETG on
smooth, per the spool and the file. Its lines, logo and glare are no debris; a
strand, part or blob is. Any of the sheet an object could sit on out of view stops
a start, as under a beam across it; its rim alone, under a beam or head or past
the frame, does not. Not starting, ask nothing; tell a person what stops it.
Never set an override.

## The boundaries you work inside

- **Every adjustment is bounded:** ask inside the range the context gives.
- **A refusal says what was allowed:** ask again inside it.
- **The vocabulary is closed: no path in this system sends a command to the
  printer.**
- **One command per call,** from the program's name, in full: nothing chained
  to it, `cd` or this program included, no variable, no usage
  listing. A refusal says nothing of the command: write it so.

## When to escalate instead

Also escalate when the machine is clearly somewhere none of
your adjustments reaches, or the safe thing is outside what you may ask for.

To escalate, pause the print and give as your reason what the person has to check.
A person's resume in the record settles what they resumed over.

## Elsewhere

[API](reference/api-and-clients.md), [schemas](reference/schemas.md),
[architecture](reference/architecture.md), [testing](reference/testing.md).
