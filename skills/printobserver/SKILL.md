---
name: printobserver
description: Supervise one 3D print as PrintObserver's agent — read what the printer and the failure detector report, decide whether to intervene, and act only through the printobserver command. Use it when PrintObserver hands you an event about a print it is watching, or when you are asked to start a print.
license: MIT
---
# Supervising a 3D print

## The role you are in

Supervise one print, an event a turn: is it right, and what bounded change fits?

## The normal workflow

1. **Decide from what is in front of you:** event, facts, frame; full context only if needed.
2. **Judge the frame.** Too dark, blurred or aimed at nothing is uncertain.
3. **Look again only when the frame is genuinely ambiguous.** Open each look; a repeated capture
   time is no newer frame, no defect.
4. **Decide.** By the order below, saying how sure.
5. **To act, ask for the change, then answer.** Give a reason, and a duration if temporary.
   Asking is acting, taken or refused: never look after it.
6. **Your answer's disposition records the check,** with what you saw: stop with any pause;
   watch after a setting or unsure while printing; else continue, false alarms included.

Name no configuration: the environment names it. Give an actor only where a command's usage asks who.

## The decision order

1. A clear defect a setting addresses gets it: flow for under-extruded walls, fan for sagging
   bridges; nothing else has one. Change one only for a defect its stated value explains, or a
   value the plan calls wrong, seen or not: flow one, fan full.
2. A clear defect with no setting, or a part moved, gone or not risen as planned: pause.
3. Uncertain: watch and look again, never pause, unless clearly worsening.
4. Nothing there: carry on.

A person's resume settles what they resumed over; judge only what is new or worse.

- **Panels:** 1 the frame with this layer's planned toolpath (grey: the part under a support-only
  layer); 2 a close-up; 3 the last check. Lines are plan, not plastic. A part off its drawn walls,
  bare sheet inside them, or moved since 3 came loose.
- **The plan** sets shape and place, not quality. Rough, porous or strung material is a defect
  even where planned.
- **Walls** visibly thin, gappy or see-through where solid are under-extruded; ribs are not.
- **Strands** are a defect only inside the part's outline and new or growing, or out of place.
- **Position:** judge growth only against the frame the plan's rise names, never by Z or apparent
  height. Risen less, or unsure, is a clear defect, never watched. A part off the bed, unstuck or
  off its place: pause now, before any look. The facts' X and Y are the bed's.

## When the detector has already paused the print

A look settles nothing, so uncertain is stop. Ask only for a setting that addresses all you see;
the supervisor resumes, never you. Else change nothing; answer stop.

## When you are asked to start a print

Open the current frame. Start only on a clear bed, naming its sheet in the manifest: textured-pei
(bronze, gold) or smooth-pei (grey, dark). Never PLA on textured or PETG on smooth. Lines, logo,
glare or a flat mark without height, shadow or colour are no debris; anything raised is. Any
usable sheet hidden, not its rim, stops a start. Not starting, ask nothing; tell a person why.
Never set an override.

## The boundaries you work inside

Ask inside the context's range. The vocabulary is closed: no path in this system sends a command
to the printer. One `printobserver` command per shell call, alone: no file read, variable, chain,
pipe, redirection, `cd` or usage listing; rewrite a refused one.

## When to escalate instead

Beyond what you may ask: pause, naming what a person must check.

[Commands](reference/command-surface.md), [operations](reference/common-operations.md) (outside
supervisors take their own frames), [API](reference/api-and-clients.md),
[policy](reference/intervention-policy.md), [schemas](reference/schemas.md),
[architecture](reference/architecture.md), [testing](reference/testing.md).
