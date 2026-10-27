; printobserver — the part the recovery journeys free a nozzle from.
;
; Homes, lays two square perimeters, at Z 0.2 and Z 2.0, inside X 90..110 and
; Y 90..110, and then dwells for two minutes before a third perimeter at Z 6.0
; the journeys never let it reach: a print stopped in the dwell laid a part
; 2.0 mm tall over that square. Heats nothing. The short dwells after the long
; one keep the third perimeter's lines out of what is sent ahead of it.
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
G1 Z2.0 F600
G1 X110 Y90 E1 F1200
G1 X110 Y110 E1
G1 X90 Y110 E1
G1 X90 Y90 E1
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
