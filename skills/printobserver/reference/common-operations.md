# Common operations

At least one worked example of every operation this system serves, each a
sequence of commands you can run in the order it is given. This document covers
what to do before you start and the worked examples themselves.

Every example below was produced by running exactly the command shown against a
real server, and a committed check runs all of them again and refuses a document
whose shown output is not what the command printed. Nothing here is typed out by
hand.

## Before you start

Two environment variables tell the command where the server is and what
authenticates to it. Where the server is and what authenticates to it are never
arguments, so every example below is the command and nothing else.

- `PRINTOBSERVER_SERVER` — the address the supervisor is listening on.
- `PRINTOBSERVER_CREDENTIAL` — the credential you act with. A supervision turn
  is handed both in its environment — a credential minted for that turn alone —
  and runs every command as shown, with no `--config`. An operator holds the
  credential `printobserver credential issue` wrote into their own client
  configuration.

A configuration file's `[client]` table does the same job, as `server` and
`credential`, and `--config <path>` names one. With neither, the operator's own
`printobserver/client.toml` under their configuration home is read; the
supervisor writes none.

Five words in the examples stand for values that are yours rather than
this document's, and three stand for values that differ on every run:

- `PRINT_ID` — the print you are working on.
- `IMAGE_ID` — an image of it.
- `EVENT_ID` — an event of it.
- `FILE` — a file the printer has, as the printer names it.
- `SPOOL_ID` — a spool the ledger holds; `spools` lists every one.
- `ID` — an identifier the server minted while answering.
- `TIMESTAMP` — an instant, in UTC.
- `STATE_DIR` — the state directory the server was configured with.

Each example shows what the command printed: standard output first and then
standard error, which is the order a terminal shows them in. Where the exit
status matters, `echo $?` is a command of the example like any other.

## The worked examples

### prints

Every print the supervisor holds, most recently opened first, and which of them
the printer's current job belongs to. This is where `PRINT_ID` comes from: start
a print in OctoPrint, run this, and take `active`.

When the printer reports a job it is printing or has paused and no open print is
that job, this read opens one for it — which is how a print started at the
printer has an identifier before Obico has reported anything. Reading it again
while the same job runs opens nothing further, and it asks the printer for
nothing but what it is doing. The first Obico alert about that job joins the
print this opened rather than opening a second.

An open print of the job's file is that job unless the job reports more than 120
seconds (the identity tolerance) less printing (`job.print_time_s`) than the print's `job_print_time_s`,
the longest it was seen printing: then it is a later job, the open print ends with
`a later job of the same file replaced it`, and the job gets a print of its own.
A pause never splits a print — OctoPrint takes a pause back out of the running
time when the job resumes — and a print with no `job_print_time_s`, or a job
reporting no running time, is matched by file name alone. What this keeps as one
print although it may be two: a later job first read after it has printed about
as long as the earlier job was last seen printing, or longer, and a later job
replacing one never seen printing.

A print ends when any read — this one, `status`, `context`, a supervision turn, a
start, or the supervisor's own start — finds its job no longer printing or
paused: its interventions are expired, it ends with `the print reached <state>`,
and its session is closed. A printer that cannot be read ends nothing.

An open print's `state` is the state it was opened in, which is `printing`
whether or not the printer is paused now. What the printer is doing is `status`'s
answer. When the printer cannot be read, the prints are still listed and
`active` is absent.

```console
$ printobserver prints
active: PRINT_ID
prints.0.file_name: FILE
prints.0.id: PRINT_ID
prints.0.job_print_time_s: 900
prints.0.job_started_at: TIMESTAMP
prints.0.narrowings: []
prints.0.opened_at: TIMESTAMP
prints.0.provider_print_id: 4211
prints.0.state: printing
```

### status

What the print and the machine are doing right now. This is the read to start
from when you want one line about where things stand.

```console
$ printobserver status --print-id PRINT_ID
interventions: []
job.completion.out_of_range: false
job.completion.value: 0.42
job.estimated_print_time_s: 3600
job.file_name: FILE
job.file_origin: local
job.print_time_left_s: 1200
job.print_time_s: 900
job.size_bytes: 4211
job.state: printing
print.file_name: FILE
print.id: PRINT_ID
print.job_print_time_s: 900
print.job_started_at: TIMESTAMP
print.narrowings: []
print.opened_at: TIMESTAMP
print.provider_print_id: 4211
print.state: printing
printer.bed.actual_c.out_of_range: false
printer.bed.actual_c.value: 59.5
printer.bed.target_c.out_of_range: false
printer.bed.target_c.value: 60.0
printer.connection: printing
printer.observed_at: TIMESTAMP
printer.tools.0.actual_c.out_of_range: false
printer.tools.0.actual_c.value: 209.5
printer.tools.0.target_c.out_of_range: false
printer.tools.0.target_c.value: 210.0
```

### context

Everything one supervision turn is given, in one read: the printer, the job, the
bounds actually in force, the interventions still standing, the recent events,
and the path the latest image was materialized to.

```console
$ printobserver context --print-id PRINT_ID
context.bounds.allowed.bed_target.max: 110.0
context.bounds.allowed.bed_target.min: 0.0
context.bounds.allowed.fan.max: 100.0
context.bounds.allowed.fan.min: 0.0
context.bounds.allowed.feedrate.max: 1.5
context.bounds.allowed.feedrate.min: 0.5
context.bounds.allowed.flowrate.max: 1.1
context.bounds.allowed.flowrate.min: 0.9
context.bounds.allowed.tool_target:0.max: 260.0
context.bounds.allowed.tool_target:0.min: 0.0
context.interventions: []
context.job.completion.out_of_range: false
context.job.completion.value: 0.42
context.job.estimated_print_time_s: 3600
context.job.file_name: FILE
context.job.file_origin: local
context.job.print_time_left_s: 1200
context.job.print_time_s: 900
context.job.size_bytes: 4211
context.job.state: printing
context.latest_image.id: IMAGE_ID
context.latest_image.sha256: 106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
context.print.file_name: FILE
context.print.id: PRINT_ID
context.print.job_print_time_s: 900
context.print.job_started_at: TIMESTAMP
context.print.narrowings: []
context.print.opened_at: TIMESTAMP
context.print.provider_print_id: 4211
context.print.state: printing
context.printer.bed.actual_c.out_of_range: false
context.printer.bed.actual_c.value: 59.5
context.printer.bed.target_c.out_of_range: false
context.printer.bed.target_c.value: 60.0
context.printer.connection: printing
context.printer.observed_at: TIMESTAMP
context.printer.tools.0.actual_c.out_of_range: false
context.printer.tools.0.actual_c.value: 209.5
context.printer.tools.0.target_c.out_of_range: false
context.printer.tools.0.target_c.value: 210.0
context.recent_events.0.id: ID
context.recent_events.0.kind: startup_reconciliation
context.recent_events.0.payload.outcome: print_adopted
context.recent_events.0.payload.print_id: PRINT_ID
context.recent_events.0.print_id: PRINT_ID
context.recent_events.0.received_at: TIMESTAMP
context.recent_events.0.source: system
context.recent_events.1.id: EVENT_ID
context.recent_events.1.image.id: IMAGE_ID
context.recent_events.1.image.sha256: 106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
context.recent_events.1.kind: obico_failure_alert
context.recent_events.1.payload.ended_at: TIMESTAMP
context.recent_events.1.payload.file_name: FILE
context.recent_events.1.payload.is_warning: true
context.recent_events.1.payload.obico_print_id: 4211
context.recent_events.1.payload.print_paused: false
context.recent_events.1.payload.started_at: TIMESTAMP
context.recent_events.1.print_id: PRINT_ID
context.recent_events.1.received_at: TIMESTAMP
context.recent_events.1.source: obico
image_path: STATE_DIR/images/10/106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
```

### image

One image record and where its bytes are. The path is on the *server's* own
filesystem; nothing here transports image content.

```console
$ printobserver image --image-id IMAGE_ID
path: STATE_DIR/images/10/106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
record.byte_len: 38
record.content_type: image/jpeg
record.event_id: EVENT_ID
record.fetched_at: TIMESTAMP
record.id: IMAGE_ID
record.print_id: PRINT_ID
record.relative_path: images/10/106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
record.sha256: 106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
record.source_url: http://a-detector.invalid/snapshot.jpg
```

### history

The print's events, newest first. `--limit` is how many to answer.

```console
$ printobserver history --print-id PRINT_ID --limit 1
events.0.id: ID
events.0.kind: startup_reconciliation
events.0.payload.outcome: print_adopted
events.0.payload.print_id: PRINT_ID
events.0.print_id: PRINT_ID
events.0.received_at: TIMESTAMP
events.0.source: system
```

### manifest_get

The manifest the print is running under, and every range it asked wider than the
safety envelope allows.

```console
$ printobserver manifest-get --print-id PRINT_ID
narrowings: []
```

### manifest_set

Replace that manifest. It carries a reason, because a manifest narrows what any
actor may ask for: replacing one changes the bounds rather than annotating them.

```console
$ printobserver manifest-set --print-id PRINT_ID --reason "the operator re-sliced this job at 0.6mm" --manifest '{"file_name":"FILE","material":"PLA","nozzle_diameter_mm":0.6,"slicer_profile":"draft","allowed":{"feedrate":{"min":0.8,"max":1.2}},"metadata":{}}'
manifest.allowed.feedrate.max: 1.2
manifest.allowed.feedrate.min: 0.8
manifest.file_name: FILE
manifest.material: PLA
manifest.metadata: {}
manifest.nozzle_diameter_mm: 0.6
manifest.slicer_profile: draft
narrowings: []
```

### set_feedrate_factor

Slow the print down or speed it up. Give `--duration-s` and the supervisor puts
the old value back when the time is up.

```console
$ printobserver set-feedrate-factor --print-id PRINT_ID --actor operator --factor 0.9 --duration-s 600 --reason "slowing down while the first layer settles"
intervention.action_id: ID
intervention.adjustable: feedrate
intervention.applied_at: TIMESTAMP
intervention.applied_value: 0.9
intervention.expires_at: TIMESTAMP
intervention.id: ID
intervention.outcome: still_active
intervention.print_id: PRINT_ID
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: set_feedrate_factor
record.request.action.actor: operator
record.request.action.duration_s: 600
record.request.action.factor: 0.9
record.request.action.reason: slowing down while the first layer settles
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

Ask for a value the bounds do not allow and the answer is a rejection carrying
what you asked for and what was allowed, which is what a second, acceptable
request is composed from:

```console
$ printobserver set-feedrate-factor --print-id PRINT_ID --actor operator --factor 2.5 --reason "asking for more than the envelope allows"
record.decision.rejected.out_of_bounds.adjustable: feedrate
record.decision.rejected.out_of_bounds.allowed.max: 1.2
record.decision.rejected.out_of_bounds.allowed.min: 0.8
record.decision.rejected.out_of_bounds.requested: 2.5
record.id: ID
record.print_id: PRINT_ID
record.request.action.action: set_feedrate_factor
record.request.action.actor: operator
record.request.action.factor: 2.5
record.request.action.reason: asking for more than the envelope allows
record.request.actor: operator
record.request.requested_at: TIMESTAMP
the supervisor's policy refused this action: out_of_bounds. It was asked for 2.5, and what is allowed is {"max":1.2,"min":0.8}. Ask again inside what the answer says is allowed
$ echo $?
5
```

### set_flowrate_factor

Change how much filament is extruded per unit of travel.

```console
$ printobserver set-flowrate-factor --print-id PRINT_ID --actor operator --factor 1.05 --duration-s 900 --reason "compensating for a slightly under-extruding spool"
intervention.action_id: ID
intervention.adjustable: flowrate
intervention.applied_at: TIMESTAMP
intervention.applied_value: 1.05
intervention.expires_at: TIMESTAMP
intervention.id: ID
intervention.outcome: still_active
intervention.print_id: PRINT_ID
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: set_flowrate_factor
record.request.action.actor: operator
record.request.action.duration_s: 900
record.request.action.factor: 1.05
record.request.action.reason: compensating for a slightly under-extruding spool
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### set_tool_target_c

Set one tool's target temperature. `--tool` is which one.

```console
$ printobserver set-tool-target-c --print-id PRINT_ID --actor operator --tool 0 --target-c 208 --duration-s 1200 --reason "raising the nozzle to improve layer adhesion"
intervention.action_id: ID
intervention.adjustable: tool_target:0
intervention.applied_at: TIMESTAMP
intervention.applied_value: 208.0
intervention.expires_at: TIMESTAMP
intervention.id: ID
intervention.outcome: still_active
intervention.print_id: PRINT_ID
intervention.prior_value: 210.0
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: set_tool_target_c
record.request.action.actor: operator
record.request.action.duration_s: 1200
record.request.action.reason: raising the nozzle to improve layer adhesion
record.request.action.target_c: 208.0
record.request.action.tool: 0
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### set_bed_target_c

Set the bed's target temperature.

```console
$ printobserver set-bed-target-c --print-id PRINT_ID --actor operator --target-c 62 --duration-s 1200 --reason "warming the bed to hold the first layer down"
intervention.action_id: ID
intervention.adjustable: bed_target
intervention.applied_at: TIMESTAMP
intervention.applied_value: 62.0
intervention.expires_at: TIMESTAMP
intervention.id: ID
intervention.outcome: still_active
intervention.print_id: PRINT_ID
intervention.prior_value: 60.0
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: set_bed_target_c
record.request.action.actor: operator
record.request.action.duration_s: 1200
record.request.action.reason: warming the bed to hold the first layer down
record.request.action.target_c: 62.0
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### set_fan_percent

Set the part-cooling fan.

```console
$ printobserver set-fan-percent --print-id PRINT_ID --actor operator --percent 80 --duration-s 300 --reason "more cooling over a small overhang"
intervention.action_id: ID
intervention.adjustable: fan
intervention.applied_at: TIMESTAMP
intervention.applied_value: 80.0
intervention.expires_at: TIMESTAMP
intervention.id: ID
intervention.outcome: still_active
intervention.print_id: PRINT_ID
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: set_fan_percent
record.request.action.actor: operator
record.request.action.duration_s: 300
record.request.action.percent: 80.0
record.request.action.reason: more cooling over a small overhang
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### pause

Pause the print. Valid only while the printer is printing.

```console
$ printobserver pause --print-id PRINT_ID --actor operator --reason "the operator wants to look at the part"
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: pause
record.request.action.actor: operator
record.request.action.reason: the operator wants to look at the part
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### resume

Resume it. Valid only while the printer is paused.

```console
$ printobserver resume --print-id PRINT_ID --actor operator --reason "the part looks fine, carrying on"
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: resume
record.request.action.actor: operator
record.request.action.reason: the part looks fine, carrying on
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### cancel

Cancel it. Valid while the printer is printing or paused.

```console
$ printobserver cancel --print-id PRINT_ID --actor operator --reason "the part has come off the bed and is not recoverable"
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: cancel
record.request.action.actor: operator
record.request.action.reason: the part has come off the bed and is not recoverable
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### start_print

Start a print of a named file, bounded by a manifest. Valid only while the
printer is operational. A start opens a print of its own when no print is open,
so `record.print_id` names the print the job runs under and its manifest is
attached to: act on that print afterwards. The examples below this one do.

```console
$ printobserver start-print --print-id PRINT_ID --actor operator --file-name FILE --manifest '{"file_name":"FILE","material":"PLA","nozzle_diameter_mm":0.4,"slicer_profile":"draft","allowed":{},"metadata":{}}' --reason "re-running the job after clearing the bed"
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: ID
record.request.action.action: start_print
record.request.action.actor: operator
record.request.action.file_name: FILE
record.request.action.manifest.allowed: {}
record.request.action.manifest.file_name: FILE
record.request.action.manifest.material: PLA
record.request.action.manifest.metadata: {}
record.request.action.manifest.nozzle_diameter_mm: 0.4
record.request.action.manifest.slicer_profile: draft
record.request.action.reason: re-running the job after clearing the bed
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### acknowledge_failure

Write down what was made of a failure event and what should happen next. It asks
nothing of the machine and is valid from every state, which is what makes it the
way an observation is put into the record.

```console
$ printobserver acknowledge-failure --print-id PRINT_ID --actor operator --event-id EVENT_ID --disposition watch --reason "spaghetti in the alert image, but the part is still attached; watching it"
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
record.request.action.action: acknowledge_failure
record.request.action.actor: operator
record.request.action.disposition: watch
record.request.action.event_id: EVENT_ID
record.request.action.reason: spaghetti in the alert image, but the part is still attached; watching it
record.request.actor: operator
record.request.requested_at: TIMESTAMP
```

### look

See the print as it is now rather than as it was when its event arrived. A look
takes a fresh frame from the camera the supervisor is configured with, reads the
printer and the job again, and writes itself into the print's history with the
frame as its image, so every frame stays in the record. Open the frame at the
`image_path` it answers exactly as you open the context's.

`--wait-s` waits that many seconds first, at most 90, so a few looks in a row
watch a defect grow or settle. A look inside a turn returns the moment a new
event arrives for its print and answers it under `arrived`: that is how a later
alert reaches the agent already watching the print rather than a second one.
`detector_paused` says whether the detector's pause is what is holding the print.

```console
$ printobserver look --print-id PRINT_ID --wait-s 1
detector_paused: false
event.id: ID
event.image.id: ID
event.image.sha256: d0531542555342942653ef333e89b8f8dfd3600bedde9fddbb51e03af10f28ff
event.kind: camera_look
event.payload.delivered: []
event.payload.waited_s: 1
event.print_id: PRINT_ID
event.received_at: TIMESTAMP
event.source: system
frame.id: ID
frame.sha256: d0531542555342942653ef333e89b8f8dfd3600bedde9fddbb51e03af10f28ff
image_path: STATE_DIR/images/d0/d0531542555342942653ef333e89b8f8dfd3600bedde9fddbb51e03af10f28ff
job.completion.out_of_range: false
job.completion.value: 0.42
job.estimated_print_time_s: 3600
job.file_name: FILE
job.file_origin: local
job.print_time_left_s: 1200
job.print_time_s: 900
job.size_bytes: 4211
job.state: printing
printer.bed.actual_c.out_of_range: false
printer.bed.actual_c.value: 59.5
printer.bed.target_c.out_of_range: false
printer.bed.target_c.value: 60.0
printer.connection: printing
printer.observed_at: TIMESTAMP
printer.tools.0.actual_c.out_of_range: false
printer.tools.0.actual_c.value: 209.5
printer.tools.0.target_c.out_of_range: false
printer.tools.0.target_c.value: 210.0
```

### register_spool

Put a spool on the shelf into the ledger. `--initial-g` is the filament on it
full; give `--remaining-g` for a roll already started, and `--tare-g`, the empty
spool's own weight, so it can be corrected from a scale reading later. A
material with a known density needs no `--density-g-cm3`.

```console
$ printobserver register-spool --name "PLA white" --material PLA --color white --initial-g 1000 --tare-g 250 --actor operator --reason "a new roll on the shelf"
decision: accepted
events.0.id: ID
events.0.kind: spool_registered
events.0.payload.actor: operator
events.0.payload.reason: a new roll on the shelf
events.0.payload.spool.color: white
events.0.payload.spool.density_g_cm3: 1.24
events.0.payload.spool.diameter_mm: 1.75
events.0.payload.spool.id: ID
events.0.payload.spool.initial_g: 1000.0
events.0.payload.spool.material: PLA
events.0.payload.spool.name: PLA white
events.0.payload.spool.registered_at: TIMESTAMP
events.0.payload.spool.remaining_g: 1000.0
events.0.payload.spool.tare_g: 250.0
events.0.payload.spool.trust.state: verified
events.0.payload.spool.updated_at: TIMESTAMP
events.0.received_at: TIMESTAMP
events.0.source: operator
spool.color: white
spool.density_g_cm3: 1.24
spool.diameter_mm: 1.75
spool.id: ID
spool.initial_g: 1000.0
spool.material: PLA
spool.name: PLA white
spool.registered_at: TIMESTAMP
spool.remaining_g: 1000.0
spool.tare_g: 250.0
spool.trust.state: verified
spool.updated_at: TIMESTAMP
```

### load_spool

Say which spool is on a tool. Loading onto an occupied tool swaps the spool
there off it in the same write; during a print, what the tool fed up to now is
charged to the spool that was feeding it.

```console
$ printobserver load-spool --spool-id SPOOL_ID --tool 0 --actor operator --reason "the black roll is going on the printer"
decision: accepted
events.0.id: ID
events.0.kind: spool_loaded
events.0.payload.actor: operator
events.0.payload.reason: the black roll is going on the printer
events.0.payload.spool_id: SPOOL_ID
events.0.payload.tool: 0
events.0.print_id: PRINT_ID
events.0.received_at: TIMESTAMP
events.0.source: operator
spool.density_g_cm3: 1.24
spool.diameter_mm: 1.75
spool.id: SPOOL_ID
spool.initial_g: 1000.0
spool.loaded_on_tool: 0
spool.material: PLA
spool.name: PLA black
spool.registered_at: TIMESTAMP
spool.remaining_g: 1000.0
spool.tare_g: 250.0
spool.trust.state: verified
spool.updated_at: TIMESTAMP
```

### spools

Every spool, and what each holds at a glance: the `at_a_glance` lines come first,
one per spool, before the charges a tool is holding (`pending`) and the whole
records. `--include-retired true` lists the retired spools too.

```console
$ printobserver spools
at_a_glance.0: PLA black | PLA | 1000.0 g of 1000.0 g left | on tool 0 | verified
at_a_glance.1: PLA white | PLA | 1000.0 g of 1000.0 g left | not loaded | verified
pending: []
spools.0.density_g_cm3: 1.24
spools.0.diameter_mm: 1.75
spools.0.id: ID
spools.0.initial_g: 1000.0
spools.0.loaded_on_tool: 0
spools.0.material: PLA
spools.0.name: PLA black
spools.0.registered_at: TIMESTAMP
spools.0.remaining_g: 1000.0
spools.0.tare_g: 250.0
spools.0.trust.state: verified
spools.0.updated_at: TIMESTAMP
spools.1.color: white
spools.1.density_g_cm3: 1.24
spools.1.diameter_mm: 1.75
spools.1.id: ID
spools.1.initial_g: 1000.0
spools.1.material: PLA
spools.1.name: PLA white
spools.1.registered_at: TIMESTAMP
spools.1.remaining_g: 1000.0
spools.1.tare_g: 250.0
spools.1.trust.state: verified
spools.1.updated_at: TIMESTAMP
```

### spool

One spool's whole record — its marks, if anything put its figure in doubt — and
the newest of the printer's events that name it.

```console
$ printobserver spool --spool-id SPOOL_ID
recent.0.id: ID
recent.0.kind: spool_loaded
recent.0.payload.actor: operator
recent.0.payload.reason: the black roll is going on the printer
recent.0.payload.spool_id: SPOOL_ID
recent.0.payload.tool: 0
recent.0.print_id: PRINT_ID
recent.0.received_at: TIMESTAMP
recent.0.source: operator
recent.1.id: ID
recent.1.kind: spool_registered
recent.1.payload.actor: operator
recent.1.payload.reason: a roll on the shelf, registered before it is used
recent.1.payload.spool.density_g_cm3: 1.24
recent.1.payload.spool.diameter_mm: 1.75
recent.1.payload.spool.id: SPOOL_ID
recent.1.payload.spool.initial_g: 1000.0
recent.1.payload.spool.material: PLA
recent.1.payload.spool.name: PLA black
recent.1.payload.spool.registered_at: TIMESTAMP
recent.1.payload.spool.remaining_g: 1000.0
recent.1.payload.spool.tare_g: 250.0
recent.1.payload.spool.trust.state: verified
recent.1.payload.spool.updated_at: TIMESTAMP
recent.1.received_at: TIMESTAMP
recent.1.source: operator
spool.density_g_cm3: 1.24
spool.diameter_mm: 1.75
spool.id: SPOOL_ID
spool.initial_g: 1000.0
spool.loaded_on_tool: 0
spool.material: PLA
spool.name: PLA black
spool.registered_at: TIMESTAMP
spool.remaining_g: 1000.0
spool.tare_g: 250.0
spool.trust.state: verified
spool.updated_at: TIMESTAMP
```

### adjust_spool

Correct a spool's figure after weighing it. `--gross-g` is the scale reading of
the whole spool, which its tare turns into what is left; `--remaining-g` gives
what is left directly. Give one of the two. A correction clears every mark the
spool held.

```console
$ printobserver adjust-spool --spool-id SPOOL_ID --gross-g 900 --actor operator --reason "weighed it on the kitchen scale"
decision: accepted
events.0.id: ID
events.0.kind: spool_adjusted
events.0.payload.actor: operator
events.0.payload.gross_g: 900.0
events.0.payload.reason: weighed it on the kitchen scale
events.0.payload.remaining_after_g: 650.0
events.0.payload.remaining_before_g: 1000.0
events.0.payload.spool_id: SPOOL_ID
events.0.print_id: PRINT_ID
events.0.received_at: TIMESTAMP
events.0.source: operator
spool.density_g_cm3: 1.24
spool.diameter_mm: 1.75
spool.id: SPOOL_ID
spool.initial_g: 1000.0
spool.loaded_on_tool: 0
spool.material: PLA
spool.name: PLA black
spool.registered_at: TIMESTAMP
spool.remaining_g: 650.0
spool.tare_g: 250.0
spool.trust.state: verified
spool.updated_at: TIMESTAMP
```

### preflight

Whether the spools loaded hold enough for a file, checked the way a start checks
it, without starting anything or writing anything down. Here OctoPrint has no
analysis of the file, so the spool on tool 0 is `no_analysis`; with one, each tool
carries what the print needs from it, the buffer asked beyond that and what the
spool holds. A start refused by this check prints those figures and what
settles them, and `--override-filament-check true` on `start-print` starts it
anyway.

```console
$ printobserver preflight --file-name FILE
preflight.checked_at: TIMESTAMP
preflight.file_name: FILE
preflight.tools.0.remaining_g: 650.0
preflight.tools.0.spool_id: ID
preflight.tools.0.spool_name: PLA black
preflight.tools.0.tool: 0
preflight.tools.0.verdict: no_analysis
preflight.verdict: no_analysis
```

### unload_spool

Take a spool off its tool.

```console
$ printobserver unload-spool --spool-id SPOOL_ID --actor operator --reason "swapping colours for the next job"
decision: accepted
events.0.id: ID
events.0.kind: spool_unloaded
events.0.payload.actor: operator
events.0.payload.reason: swapping colours for the next job
events.0.payload.spool_id: SPOOL_ID
events.0.payload.tool: 0
events.0.print_id: PRINT_ID
events.0.received_at: TIMESTAMP
events.0.source: operator
spool.density_g_cm3: 1.24
spool.diameter_mm: 1.75
spool.id: SPOOL_ID
spool.initial_g: 1000.0
spool.material: PLA
spool.name: PLA black
spool.registered_at: TIMESTAMP
spool.remaining_g: 650.0
spool.tare_g: 250.0
spool.trust.state: verified
spool.updated_at: TIMESTAMP
```

### printer_history

The printer's own history, newest first: here narrowed to two kinds — `--kinds`
may be given any number of times — and to one spool, one event to a page.
`next` is what to give as `--before` for the page after; it is absent on the
last. `--limit` is 100 when left out, and more than 1,000 is refused.

```console
$ printobserver printer-history --spool-id SPOOL_ID --kinds spool_adjusted --kinds spool_unloaded --limit 1
events.0.id: ID
events.0.kind: spool_unloaded
events.0.payload.actor: operator
events.0.payload.reason: swapping colours for the next job
events.0.payload.spool_id: SPOOL_ID
events.0.payload.tool: 0
events.0.print_id: PRINT_ID
events.0.received_at: TIMESTAMP
events.0.source: operator
next: ID
```

### retire_spool

Retire a spool that is used up. It takes no further command: a load of it
afterwards is the ledger's refusal, which says why, and exits with the rejected
status.

```console
$ printobserver retire-spool --spool-id SPOOL_ID --actor operator --reason "the roll is used up"
decision: accepted
events.0.id: ID
events.0.kind: spool_retired
events.0.payload.actor: operator
events.0.payload.reason: the roll is used up
events.0.payload.remaining_g: 650.0
events.0.payload.spool_id: SPOOL_ID
events.0.print_id: PRINT_ID
events.0.received_at: TIMESTAMP
events.0.source: operator
spool.density_g_cm3: 1.24
spool.diameter_mm: 1.75
spool.id: SPOOL_ID
spool.initial_g: 1000.0
spool.material: PLA
spool.name: PLA black
spool.registered_at: TIMESTAMP
spool.remaining_g: 650.0
spool.retired_at: TIMESTAMP
spool.tare_g: 250.0
spool.trust.state: verified
spool.updated_at: TIMESTAMP
$ printobserver load-spool --spool-id SPOOL_ID --tool 0 --actor operator --reason "putting it back on"
decision.rejected.spool_refused.detail: spool PLA black is retired and takes no further command; register a new spool instead
events.0.id: ID
events.0.kind: spool_command_rejected
events.0.payload.command.actor: operator
events.0.payload.command.command: load_spool
events.0.payload.command.reason: putting it back on
events.0.payload.command.spool_id: SPOOL_ID
events.0.payload.command.tool: 0
events.0.payload.decision.rejected.spool_refused.detail: spool PLA black is retired and takes no further command; register a new spool instead
events.0.print_id: PRINT_ID
events.0.received_at: TIMESTAMP
events.0.source: operator
spool.density_g_cm3: 1.24
spool.diameter_mm: 1.75
spool.id: SPOOL_ID
spool.initial_g: 1000.0
spool.material: PLA
spool.name: PLA black
spool.registered_at: TIMESTAMP
spool.remaining_g: 650.0
spool.retired_at: TIMESTAMP
spool.tare_g: 250.0
spool.trust.state: verified
spool.updated_at: TIMESTAMP
the supervisor's spool ledger refused this command: spool_refused. spool PLA black is retired and takes no further command; register a new spool instead
$ echo $?
5
```
