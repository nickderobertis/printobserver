# Recorded OctoPrint 1.11.8 bodies

Every body here was answered by the OctoPrint 1.11.8 `just octoprint-up`
provisions, with its virtual printer, by `curl` against the paths the file
names, and is committed exactly as it was answered — with one exception, which
is derived and says so.

- `job-running.json` — `GET /api/job` while `extrusion.gcode` printed.
- `job-two-tools-selected.json` — `GET /api/job` with `two-tools.gcode`
  selected and not started.
- `job-cancelled.json` — `GET /api/job` after `extrusion.gcode` was cancelled:
  `progress` cleared to nulls.
- `job-completed.json` — `GET /api/job` after `two-tools.gcode` completed:
  `completion` 100 and `filepos` the file's size.
- `job-null-lengths.json` — **derived** from `job-two-tools-selected.json` by
  nulling `tool0.length` and `tool1.volume`, the two nulls OctoPrint's
  `job.filament` is documented to carry and a virtual printer does not produce.
- `file-before-analysis.json` — `GET /api/files/local/two-tools.gcode` straight
  after its upload, before the analysis queue reached it.
- `file-analysed-never-printed.json` — the same file once analysed.
- `file-printed.json` — `GET /api/files/local/extrusion.gcode` after one
  cancelled print.
- `files-recursive.json` — `GET /api/files?recursive=true` with a folder
  holding a file.
- `file-missing.json` — the body of the `404` `GET /api/files/local/<name>`
  answers for a file the instance does not hold.
