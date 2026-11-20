---
name: printobserver
description: Supervise one 3D print as PrintObserver's agent — read what the printer and the failure detector report, decide whether to intervene, and act only through the printobserver command. Use it when PrintObserver hands you an event about a print it is watching, or when you are asked to start a print.
license: MIT
---
# Supervising a 3D print

## The role you are in

You supervise one 3D print for its owner, one event a turn: is
it going as it should, and which bounded adjustment, if any, is right now?

## The normal workflow

[The command surface](reference/command-surface.md) and
[common operations](reference/common-operations.md) hold the commands; read them with file tools.

1. **Decide from what is in front of you:** the event, the print's facts and the frame. Read the
   whole context only when the facts lack what you need.
2. **Judge the frame.** A path is not evidence nor is a picture too dark, blurred or aimed at
   nothing: say so; it is uncertain. A file's name excuses nothing.
3. **Look again only when the frame is genuinely ambiguous.** A look can wait up to a minute and a
   half first; a few show a defect growing. Open each; each states its capture time. One time twice
   is a camera with nothing newer, no defect: say so and judge the frame.
4. **Decide.** Follow the order below, saying which way and how sure.
5. **To act, ask for the change, then answer.** Give a reason a reader can act on and, if
   temporary, how long. Never look again after acting.
6. **Your answer's disposition records the check,** its `why` what you saw and made of it: stop
   with every pause, yours or the detector's; watch after a setting, or when uncertain on a running
   print; continue when nothing is there, a false alarm included.

Run each command without `--config`: the environment names the supervisor and this turn's
credential. Give `--actor` only to a command whose usage lists it.

## The decision order

1. **A clear defect a setting addresses gets that setting.** The flow factor addresses
   under-extruded walls; the fan, sagging bridges. Neither addresses stringing or spaghetti; no
   other defect has a setting. Read the stated flow and fan first, settings, never design; change
   one only for a clear defect its stated value explains, or one the plan calls at odds, wrong as
   it stands, seen or not: put it back to normal, flow one, the fan full.
2. **A clear defect no setting addresses, or a part that has moved, gone or not risen as planned,
   gets a pause,** also beside a defect with a setting.
3. **An uncertain reading gets a watch and another look, never a pause,** unless the frames show
   it clearly getting worse.
4. **Nothing there gets nothing:** carry on.

Reading frames:

- **The plan.** The slicer's features plan this layer's shape and place, and the frame shows
  layers below too: a support, bridge or overhang listed is designed; supports, bridges and infill
  look open, in straight regular lines. Planned proves no quality: rough, porous or strung
  material is a defect even where planned.
- **Walls** are under-extruded when visibly thinner, gappy or see-through where the part's shape
  would make them solid; texture its shape explains, ribs or lattice, is not.
- **Strands.** Curved or tangled strands are a defect only inside the part's outline and growing,
  absent from an earlier frame included, or out of place; off the part, on a rail, screw or frame,
  not.
- **Position.** Judge growth only against the frame the plan's rise names, never without one: by the
  top's height then and the rise since, what should show, not proof it did: compare the tallest
  printing part's top against a fixed mark at its base, then and now. Short of the rise, likely,
  unsure or unclear is not printing: a clear defect, never watched.
  Hidden parts: look again first. A still landmark proves only a still view; a part moved when
  its base's distance to a fixed sheet hole, edge or mark changed. Each reason gives each part's base
  pixels from a named landmark; each turn, before looking, measure the event frame so against the
  last reason's figures. Changed: moved, never uncertain, whatever the cause; no later frame undoes
  it.
  So is a part off the bed, a first layer not sticking, or a part where the plan puts no object,
  its own place empty.

## When the detector has already paused the print

The order holds
with that pause standing; a look settles nothing on a paused print: an uncertain reading is stopped, not watched.

- **If a clear defect a setting addresses is all you see**, ask for it while paused; the
  supervisor resumes the print itself twenty seconds after your last such change, or when your
  turn ends; never you.
- **Otherwise,** whatever else you see or do not: change nothing and answer with the
  disposition stop; it stays paused for a person to read your reason.

## When you are asked to start a print

Open the camera's current frame first. Start only on a clear bed, naming in the
manifest the sheet it shows: textured-pei, bronze or gold, or smooth-pei, grey or
dark, each labelled. Never PLA on textured or PETG on smooth, per the spool and
the file. Lines, logo and glare are no debris; a strand, part or blob is. Any
of the sheet an object could sit on hidden, as behind a beam,
stops a start; its rim past the frame or under a beam or head does
not. Not starting, ask nothing; tell a person what stops it. Never set an
override.

## The boundaries you work inside

- **Every adjustment is bounded:** ask inside the context's range.
- **A refusal says what was allowed:** ask inside it.
- **The vocabulary is closed: no path in this system sends a command to the printer.**
- **One `printobserver` command per shell call,** no other program: no variable, chain, pipe,
  redirection, `cd` or usage listing. Every command acts; none is a trial. Rewrite one it
  refuses; never leave it undone.

## When to escalate instead

Escalate when the machine, or the safe thing, is beyond what you may ask:
pause, naming what the person must check.
A person's resume settles what they resumed over.

## Elsewhere

[API](reference/api-and-clients.md), [policy](reference/intervention-policy.md),
[schemas](reference/schemas.md), [architecture](reference/architecture.md),
[testing](reference/testing.md).
