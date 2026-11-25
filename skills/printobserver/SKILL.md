---
name: printobserver
description: Supervise one 3D print as PrintObserver's agent — read what the printer and the failure detector report, decide whether to intervene, and act only through the printobserver command. Use it when PrintObserver hands you an event about a print it is watching, or when you are asked to start a print.
license: MIT
---
# Supervising a 3D print

## The role you are in

Supervise one print, an event a turn: is it right, and what bounded change fits?

## The normal workflow

Commands: [the command surface](reference/command-surface.md),
[common operations](reference/common-operations.md).

1. **Decide from what is in front of you:** event, facts, frame; full context only if needed.
2. **Judge the frame.** Too dark, blurred or aimed at nothing is uncertain.
3. **Decide on the image in hand;** a turn with one has no look. With none, a look can wait up to
   a minute and a half first; open each. A repeated capture time is no newer frame, no defect.
4. **Decide.** By the order below, saying how sure.
5. **To act, ask for the change, then answer.** Give a reason, and a duration if temporary.
   Asking is acting, taken or refused: never look after it.
6. **Your answer's disposition records the check,** with what you saw: stop with any pause;
   watch after a setting or unsure while printing; else continue, false alarms included.

Name no configuration: the environment names it. Give an actor only where a command's usage asks who.

## The decision order

1. A clear defect a setting addresses gets it: flow for under-extruded walls, fan for sagging
   bridges; nothing else has one. Read the stated flow and fan first, settings, never design;
   change one only for a clear defect its stated value explains, or one the plan calls at odds,
   wrong as it stands, seen or not: put it back to normal, flow one, the fan full.
2. A clear defect with no setting, a colour the plan does not expect here, or a part moved, gone
   or not risen as planned: pause.
3. Uncertain, growth aside: watch, never pause, unless clearly worsening.
4. Nothing there: carry on.

A person's resume settles what they resumed over; judge only what is new or worse. No person puts
a part anywhere new or back exactly: one off its own place or moved since 3 moved by itself: pause.

- **Panels:** 1 the frame with this layer's planned toolpath (grey: the part under a support-only
  layer); 2 a close-up; 3 the last check. Lines are plan, not plastic. A part off its drawn walls,
  bare sheet inside them, or moved since 3 came loose.
- **The plan** sets shape and place, not quality; the frame shows layers below too: a support,
  bridge or overhang listed is designed, and supports, bridges and infill look open and straight.
  Rough, porous or strung material is a defect even where planned: judge it in the close-up.
- **Walls** visibly thin, gappy or see-through where solid are under-extruded; ribs are not.
- **Strands** are a defect only inside the part's outline and new or growing, or out of place.
- **Position:** judge growth only against the frame the plan's rise names, never by Z or how tall
  parts look; the rise is what should show, not proof it did: a part has risen only where its top
  edge plainly sits higher against the sheet's marks now than then. Lines are drawn at this layer's
  height, on the top edge of a part printing to plan and clear of one that stopped below it; a part
  this layer draws is not finished. Risen less than the rise, likely, unsure or unclear is not
  printing: a clear defect, never watched. A part off the bed, a first layer not sticking, or one
  off its own place, left empty, has moved: pause now on the image in hand; no later frame undoes
  it. The facts' X and Y are the bed's, placing nothing in a frame.

## A planned colour change

Each check, compare new material's colour in panel 2, which draws nothing, with the colour the
plan expects: panel 1 draws walls red. At a planned change the printer waits for a person
(`filament_change_planned_reached`): no check runs, and a pause, resume or setting is refused
`planned_change_waiting`. The check after, `filament_change_planned_resumed`, names the colour
expected from its layer and the one before: that colour from there up is expected; carry on.
A colour other than expected, with no planned change at or just under this layer, is a wrong
spool or a reload: a clear defect, never watched. Pause now, naming the spool and file to check.

## When the detector has already paused the print

Uncertain is stop. Ask only for a setting that addresses all you see; the supervisor resumes the
print itself twenty seconds after your last such change, never you. Else change nothing; answer
stop.

## When you are asked to start a print

Open the camera's current frame first. Start only on a clear bed, naming in the
manifest the sheet it shows: textured-pei, bronze or gold, or smooth-pei, grey or
dark, each labelled. Never PLA on textured or PETG on smooth, per the spool and
the file. Lines, logo and glare are no debris; a strand, part or blob is. A flat
mark in the sheet, with no height, shadow, loose end or filament colour, is wear;
anything raised is debris. Any of the sheet an object could sit on hidden, as
behind a beam, stops a start; its rim past the frame or under a beam or head does
not. Not starting, ask nothing; tell a person what stops it. Never set an
override.

## The boundaries you work inside

Ask inside the context's range. The vocabulary is closed: no path in this system sends a command
to the printer. One `printobserver` command per shell call, alone: no file read, variable, chain,
pipe, redirection, `cd` or usage listing; rewrite a refused one.

## When to escalate instead

Beyond what you may ask: pause, naming what a person must check.

[API](reference/api-and-clients.md), [policy](reference/intervention-policy.md),
[schemas](reference/schemas.md), [architecture](reference/architecture.md),
[testing](reference/testing.md).
An outside supervisor takes its own frames: [see](reference/common-operations.md).
