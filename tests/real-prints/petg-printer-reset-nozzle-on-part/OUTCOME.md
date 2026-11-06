# Outcome: failed at 82.5% (printer reset), side bins finished by a separate tops print

- Started 2026-10-10 ~06:29 local (Obico print 11, printobserver record 01a125ca…).
- 10:39: Obico warning (no pause) at ~29% for a rough patch in the grid infill at the back of the
  left bin. printobserver's agent took three looks over ~3 min, saw it not grow, and acknowledged
  it with disposition `watch`. The operator's monitor never showed this: it ignores a record that
  already exists when the watch starts, and printobserver opens its record at the print's start.
- 14:22-15:28: Obico web container down; restarting only `obico-web-1` brought it back. Over
  14:28-14:47 every `printobserver prints` call also failed to start (exit 0xC0000142).
- 16:10:45: the printer reset at 82.5% (Z 46.4 complete, Z 46.6 partial; that layer starts at G-code
  line 916835). OctoPrint lost the port; reconnected 16:44.
- ~17:07: after the reconnect the printer itself heated the nozzle to 230 C, retracted and reported
  "Done printing file", with the nozzle ~0.5 mm above the bin. The PETG welded nozzle to part, and a
  10 mm lift pried the sheet up. Recovered by reversing the move, holding 180 C and lifting slowly;
  the person cut the blob free.
- While freeing the parts two bins were knocked loose. A resume attempt homed Z with the third bin
  still on the bed: the gantry hit it ("HOMING ERROR Z"). The person cleared the bed; the loadcell
  test, homing and alignment all passed afterwards.
- Lessons (now in the operator's printer notes): never home or probe Z with tall parts on the bed (Z homes at
  X240 Y10 and the gantry passes low over the parts); never heat a nozzle resting near a part;
  check the printer's screen before sending anything after a reset.
- The bins' top 9 mm (Z 46.4-55.4) were reprinted as the 2026-10-10 run of plate1b-side-bin-tops to
  glue on. The printobserver record is still open as "printing" (printobserver#169).
