# Outcome: failed (air print), cancelled at 21.1% by file

- Started 2026-10-08 ~16:07 local. The cardboard Elegoo Rapid PETG spool jammed (filament dropped
  behind the rear flange and wrapped the hub). Extrusion stopped at about Z 2 mm; the printer kept
  going to Z 13 (layer 65) printing nothing.
- The stalled PETG caused heat creep: the nozzle was fully clogged afterwards (no flow at 270 C with
  PLA, cold pull impossible). The nozzle was replaced on 2026-10-09.
- Neither Obico (alerted_at null, print 9) nor printobserver flagged it; the person spotted it from
  the side. The top-down camera hid the missing height; I misread it as normal. A useful negative case.
- Paused 17:27, cancelled 17:29 with heaters off. Reprinted as the 2026-10-09 run of plate2-pan-bin-scraper-clips-r2.
