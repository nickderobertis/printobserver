; printobserver — the filament journey's print.
;
; It feeds filament and heats nothing: twenty feeds of 100 mm on tool 0, each
; a 10 mm move along X and back, and each followed by a dwell — so OctoPrint's
; analysis of it says tool 0 needs 2000 mm, and a print of it runs long enough
; to be cancelled part-way. It ends where it started.
; Its last lines are the settings block a slicer writes, naming the PLA it is
; sliced for, which a start's material check reads from the file's tail.
M117 printobserver filament
G21 ; millimetres
G90 ; absolute positioning
M83 ; relative extrusion
G92 E0
G0 X0 Y0 F3000
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
G1 X10 E100 F600
G4 S3
G1 X0 E100 F600
G4 S3
M117 printobserver filament done
; prusaslicer_config = begin
; filament_type = PLA
; prusaslicer_config = end
