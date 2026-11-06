# Outcome: finished 2026-10-09 16:37 local, one object cancelled and one Obico pause

- Reprint of the 2026-10-08 run of plate2-pan-bin-scraper-clips after the nozzle swap and an
  anti-despool device on the cardboard Elegoo Rapid PETG spool. Started ~07:22 local.
- Spool clip at X218.9-240.8 Y8.4-24.9 (M486 object 9) came loose at about Z 5.8 (09:47-09:52) and
  tangled in the front-right corner. With the person's go-ahead, `M486 P9` cancelled just that
  object at 09:59:47; the rest kept printing and the tangle never grew. That clip needs reprinting.
- From about Z 70 the PETG strung a web across the opening of the tall magnetic bin body (travel
  moves crossing it). Nozzle stayed clean, walls intact. Obico's score climbed: alert at 14:20:34
  (0.45), then it auto-paused at 14:29:54 (0.70) at 76.2% / Z 78.2; nozzle heater off, bed held 85 C.
  A false alarm in the sense that nothing was failing, but the web was real.
- The person cleared the strings; resumed 15:07 with the operator's resume command, acknowledging the Obico alert as NOT_FAILED
  (about 37 min paused; M602 reheated to 250 C). A second alert at 15:09:04 during the restart did
  not pause. Lighter stringing returned and stayed below the alert level to the end.
- Obico print 10. printobserver record 01a120d3-e59e-7b40-94f1-4eefc67bb6d1 was still "printing"
  right after the end (see the stale-record issue).
