; printobserver — a short print the recovery journeys run to its end.
;
; Homes, lays one square perimeter at Z 1.0 inside X 90..110 and Y 90..110,
; lifts the nozzle to Z 20, and dwells there long enough for a read of the
; machine to see how far into the file it is. Heats nothing.
G21
G90
M83
G28
G1 Z1.0 F600
G1 X90 Y90 F3000
G1 X110 Y90 E1 F1200
G1 X110 Y110 E1
G1 X90 Y110 E1
G1 X90 Y90 E1
G1 Z20 F600
G4 S15
M84
