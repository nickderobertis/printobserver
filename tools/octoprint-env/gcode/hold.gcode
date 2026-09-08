; printobserver — the hold print.
;
; Trivial and slow on purpose. It moves nothing and heats nothing: every line
; is a dwell, so it runs for the same wall-clock time on the virtual printer
; the integration tier drives and on a real machine, and leaves that machine
; exactly where it found it.
;
; 40 dwells of 10 seconds is 400 seconds of print — well past the minimum
; `octoprint_env.py` states in HOLD_SECONDS, so the tier still finds a running
; print after it has waited that minimum out.
M117 printobserver hold
G21 ; millimetres
G90 ; absolute positioning
M82 ; absolute extrusion
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
G4 S10
M117 printobserver hold done
