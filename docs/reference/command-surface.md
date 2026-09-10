# The command surface

Every command the `printobserver` program has, one entry each. This document
covers what a command is made of, the four global options, the exit statuses,
and the commands — each with its arguments, its output and its failures.

Nothing here is a list somebody maintains. The set of commands is one per
operation the server serves, and each command's options are that operation's own
declared request — for an action, the fields the contracts' action vocabulary
declares for that variant. `docs/reference/surface.json` is that declaration,
written out by the program itself, and the check behind this document reads it.

## What a command is made of

A command line is the program's name, one command, and that command's own
options. An option that takes a document takes it either as the document itself
or, under the same name with `-file` after it, as the path of a file carrying it.

**Where the server is and what authenticates to it are configuration, never
arguments.** They are read from a configuration file — which may be the server's
own — and from the environment variables `PRINTOBSERVER_SERVER` and
`PRINTOBSERVER_CREDENTIAL`, which win over the file. No command takes either as
an option, because the closure that matters is the closure of the action surface:
a path to a file reaches no printer.

**Images are a path, never bytes.** Every answer that carries an image carries an
absolute path on the *server's* own filesystem. This program transports no image
content at all.

## The four global options

Every command takes these four and there is no fifth.

- `--json` — print the answer as the document the server sent rather than as
  labelled lines.
- `--config <path>` — read configuration from this file instead of the default.
- `--help` — print the whole command surface.
- `--version` — print this program's version.

## The exit statuses

A caller decides what to do next from the status before it reads a word, so each
class has a status of its own.

- `success` (0) — the command did what it was asked.
- `usage` (2) — the arguments name nothing this program does.
- `unreachable` (3) — nothing answered at the configured address.
- `unconfigured` (4) — nothing configured this program with a server to talk to.
- `rejected` (5) — the policy refused the action, and the rejection is the answer.
- `image-elsewhere` (6) — the answer named an image path, and no file is there on this host.
- `refused` (7) — the supervisor answered something this program will not act on.

## The commands

### server

Runs the supervisor. This is the only command that is not a request to an already-running one.

**Arguments.** None of its own; the four global options are the whole of it.

**Output.** The address it is serving on, on standard error, and then nothing until it stops.

**Failures.** `unconfigured` when the configuration file is absent, unreadable or incomplete, naming the field; `usage` when the arguments name nothing this program does.

### status

What the print and the machine are doing right now.

Reached at `GET /v1/prints/{print_id}/status`.

**Arguments.**

- `--print-id` — the print this command is about (required).

**Output.** The print record, a printer snapshot and a job snapshot when one could be taken, the supervision session watching it when one is, and every bounded intervention still in force.

**Failures.** `unreachable` when nothing answers at the configured address; `refused` when the supervisor answers something this program will not act on.

### context

Everything one supervision turn is given about a print, in one read.

Reached at `GET /v1/prints/{print_id}/context`.

**Arguments.**

- `--print-id` — the print this command is about (required).

**Output.** The print's context — the printer, the job, the effective bounds, the active interventions and the recent events — and the absolute path on the server's own host where the latest image was materialized.

**Failures.** `image-elsewhere` when that path names no file on the host this command ran on: the rest of the answer is printed and the path is replaced by a line saying why. `unreachable` and `refused` as for every command.

### image

One image record, and where its bytes are.

Reached at `GET /v1/images/{image_id}`.

**Arguments.**

- `--image-id` — the image this command is about (required).

**Output.** The image record and the absolute path on the server's own host where its bytes are. No route of this system answers image bytes.

**Failures.** `image-elsewhere` when the path names no file here; `unreachable` and `refused` as for every command.

### history

The print's events, newest first.

Reached at `GET /v1/prints/{print_id}/history`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--limit` — how many events to answer, newest first (optional).

**Output.** Every event of the print, newest first, each with its kind and its payload.

**Failures.** `unreachable` and `refused` as for every command.

### manifest-get

The manifest a print is running under.

Reached at `GET /v1/prints/{print_id}/manifest`.

**Arguments.**

- `--print-id` — the print this command is about (required).

**Output.** The manifest when the print has one, and every range that manifest asked wider than the safety envelope allows, narrowed to the envelope's.

**Failures.** `unreachable` and `refused` as for every command.

### manifest-set

Replace the manifest a print runs under. It carries a reason because a manifest narrows what any actor may ask for, so replacing one changes the bounds rather than annotating them.

Reached at `PUT /v1/prints/{print_id}/manifest`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--reason` — why this change is being made, in the actor's own words (required).
- `--manifest` — the manifest itself, as JSON (required).
- `--manifest-file` — the path of a file carrying that JSON instead (optional).

**Output.** The manifest as it now stands, and its narrowings.

**Failures.** The request is refused before anything is written when it carries no reason. `unreachable` and `refused` as for every command.

### pause

Pause the print. Valid only while the printer is printing.

Reached at `POST /v1/prints/{print_id}/actions/pause`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### resume

Resume the print. Valid only while the printer is paused.

Reached at `POST /v1/prints/{print_id}/actions/resume`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### cancel

Cancel the print. Valid while the printer is printing or paused.

Reached at `POST /v1/prints/{print_id}/actions/cancel`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### start-print

Start a print of a named file, bounded by a manifest. Valid only while the printer is operational.

Reached at `POST /v1/prints/{print_id}/actions/start_print`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--file-name` — the file to print, as the printer names it (required).
- `--manifest` — the manifest itself, as JSON (required).
- `--manifest-file` — the path of a file carrying that JSON instead (optional).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### set-feedrate-factor

Set the feedrate multiplier. Bounded, and temporary when a duration is given.

Reached at `POST /v1/prints/{print_id}/actions/set_feedrate_factor`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--duration-s` — how long the change stands for, in whole seconds (optional).
- `--factor` — the multiplier asked for (required).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### set-flowrate-factor

Set the flowrate multiplier. Bounded, and temporary when a duration is given.

Reached at `POST /v1/prints/{print_id}/actions/set_flowrate_factor`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--duration-s` — how long the change stands for, in whole seconds (optional).
- `--factor` — the multiplier asked for (required).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### set-tool-target-c

Set one tool's target temperature. Bounded, and temporary when a duration is given.

Reached at `POST /v1/prints/{print_id}/actions/set_tool_target_c`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--duration-s` — how long the change stands for, in whole seconds (optional).
- `--reason` — why this change is being made, in the actor's own words (required).
- `--target-c` — the temperature asked for, in degrees Celsius (required).
- `--tool` — which tool the adjustment is about (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### set-bed-target-c

Set the bed's target temperature. Bounded, and temporary when a duration is given.

Reached at `POST /v1/prints/{print_id}/actions/set_bed_target_c`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--duration-s` — how long the change stands for, in whole seconds (optional).
- `--reason` — why this change is being made, in the actor's own words (required).
- `--target-c` — the temperature asked for, in degrees Celsius (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### set-fan-percent

Set the fan percentage. Bounded, and temporary when a duration is given.

Reached at `POST /v1/prints/{print_id}/actions/set_fan_percent`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--duration-s` — how long the change stands for, in whole seconds (optional).
- `--percent` — the percentage asked for (required).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

### acknowledge-failure

Write down what was made of a failure event, and what should happen next. It asks nothing of the machine and is valid from every state.

Reached at `POST /v1/prints/{print_id}/actions/acknowledge_failure`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--disposition` — what should happen next (required).
- `--event-id` — the failure event being acknowledged (required).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses: the answer is still the record, and it carries the rejection's reason, the value asked for and the range allowed, which is what a second request is composed from. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.
