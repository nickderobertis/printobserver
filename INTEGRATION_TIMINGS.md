# Integration timing evidence

The baseline for the integration preparation and pacing work starts from
`f7b8897`. This record is outside the integration workload; the baseline changes
no test, fixture, recipe, or workflow. Before and after measurements must use
this draft’s own runs with identical project selections and collected test
counts. Measurements are pending; no improvement is claimed yet.

The spike reference workload comprises 24 projects, 12 Rust crates (1,049
Windows tests and 1,081 Linux tests), 1,368–1,371 repo-checks tests, 424
release-artifacts tests, 337 end-to-end journeys, and seven integration projects.
The client walk traces 776 invocations and the integration walk traces 276.
Windows and Linux report four processors; macOS reports three. These reference
figures are distinct from this draft’s baseline and implementation runs.
