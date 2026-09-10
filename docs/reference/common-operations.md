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
- `PRINTOBSERVER_CREDENTIAL` — the credential it was configured with.

A configuration file does the same job and `--config <path>` names one; see
[the command surface](command-surface.md).

Four words in the examples stand for values that are yours rather than
this document's, and three stand for values that differ on every run:

- `PRINT_ID` — the print you are working on.
- `IMAGE_ID` — an image of it.
- `EVENT_ID` — an event of it.
- `FILE` — a file the printer has, as the printer names it.
- `ID` — an identifier the server minted while answering.
- `TIMESTAMP` — an instant, in UTC.
- `STATE_DIR` — the state directory the server was configured with.

Each example shows what the command printed: standard output first and then
standard error, which is the order a terminal shows them in. Where the exit
status matters, `echo $?` is a command of the example like any other.

## The worked examples

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
print.narrowings: []
print.obico_print_id: 4211
print.opened_at: TIMESTAMP
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
context.print.narrowings: []
context.print.obico_print_id: 4211
context.print.opened_at: TIMESTAMP
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
printer is operational.

```console
$ printobserver start-print --print-id PRINT_ID --actor operator --file-name FILE --manifest '{"file_name":"FILE","material":"PLA","nozzle_diameter_mm":0.4,"slicer_profile":"draft","allowed":{},"metadata":{}}' --reason "re-running the job after clearing the bed"
record.decision: accepted
record.executed_at: TIMESTAMP
record.id: ID
record.outcome: succeeded
record.print_id: PRINT_ID
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
