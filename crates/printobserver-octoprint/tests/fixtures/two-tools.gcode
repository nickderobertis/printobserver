; printobserver — a file fed by two tools, for OctoPrint's analysis of each.
G21
G90
M83 ; relative extrusion
T0
G1 X10 Y10 E5 F1200
G1 X20 Y10 E5
T1
G1 X20 Y20 E3
G1 X10 Y20 E4
M84
