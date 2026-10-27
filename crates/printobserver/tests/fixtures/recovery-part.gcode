; printobserver — the part the recovery journeys free a nozzle from.
;
; Homes, lays a square perimeter at Z 0.2 inside X 90..110 and Y 90..110, and
; a second over it that is 2.0 mm tall only by the reader's own rules, then
; dwells for two minutes before a third perimeter at Z 6.0 the journeys never
; let it reach: a print stopped in the dwell laid a part 2.0 mm tall over that
; square. Heats nothing. The short dwells after the long one keep the third
; perimeter's lines out of what is sent ahead of it.
;
; The second perimeter is written in every form the reader follows, so that
; reading 2.0 mm over exactly that square needs each of them read right: an
; extruder reset (G92), moves relative to the last (G91), a home of one axis
; (G28 X), an arc whose circle stays inside the square (G3), a prime that
; feeds while it rises to 2.4 with no travel in X or Y and so lays nothing
; along a path, and a segment descending while it extrudes — the only line
; laying material at 2.0, the rest of the perimeter being at 1.8.
G21
G90
M83
G28
G1 Z0.2 F600
G1 X90 Y90 F3000
G1 X110 Y90 E1 F1200
G1 X110 Y110 E1
G1 X90 Y110 E1
G1 X90 Y90 E1
G92 E0
G91
G1 Z1.6 F600
G1 X20 E1 F1200
G1 Y10 E1
G90
G3 X100 Y110 I-10 J0 E1
G28 X
G1 X100 Y110 F3000
G1 X90 Y110 E1 F1200
G1 Z2.4 E0.5 F600
G1 Z2.0 F600
G1 X90 Y90 Z1.8 E1 F1200
G4 S120
G4 P10
G4 P10
G4 P10
G4 P10
G4 P10
G1 Z6.0 F600
G1 X120 Y90 E1 F1200
G1 X120 Y120 E1
M84
