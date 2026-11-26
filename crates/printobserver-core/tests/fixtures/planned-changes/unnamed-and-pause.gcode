; Written by hand in PrusaSlicer 2.9.6's G-code syntax, not sliced by it: an M600 whose ;COLOR_CHANGE is not the line before it, and an M601 pause laid mid-layer.
M83
;LAYER_CHANGE
;Z:0.2
;HEIGHT:0.2
G1 X0 Y0 F12000
;TYPE:External perimeter
G1 X10 Y0 E1
;LAYER_CHANGE
;Z:0.4
;HEIGHT:0.2
;COLOR_CHANGE,T0,#00FF00
G1 Z.4 F720
M600
G1 X10 Y10 E1
;LAYER_CHANGE
;Z:0.6
;HEIGHT:0.2
G1 X0 Y10 E1
M601
G1 X0 Y0 E1
;LAYER_CHANGE
;Z:0.8
;HEIGHT:0.2
G1 X10 Y0 E1
