# The command surface

Every command the `printobserver` program has, one entry each. This document
covers what a command is made of, the four global options, the exit statuses,
and the commands — each with its arguments, its output and its failures.

Nothing here is a list somebody maintains. The set of commands is one per
operation the server serves, and each command's options are that operation's own
declared request — for an action or a spool command, the fields the contracts'
own vocabulary declares for that variant. The program writes that declaration out as
`surface.json` beside this document, and the check behind this document reads it
there — it is generated from the program rather than written by hand, and an
installed copy of these documents carries it too.

## What a command is made of

A command line is the program's name, one command, and that command's own
options. An option that takes a document takes it either as the document itself
or, under the same name with `-file` after it, as the path of a file carrying it.

**Where the server is and what authenticates to it are configuration, never
arguments.** They are read from a configuration file's `[client]` table, as
`server` and `credential`, and from the environment variables
`PRINTOBSERVER_SERVER` and `PRINTOBSERVER_CREDENTIAL`, which win over any file.
With no `--config`, the operator's own client configuration is read first — the
file `credential issue` writes, `printobserver/client.toml` under their
configuration home — and the server's own configuration file after it for
whatever the first left unnamed. `--config` names the one file read instead.
When both variables are set and no `--config` names a file, no file is read at
all. A supervision turn is configured exactly so: its environment names the
server and carries a credential minted for that turn alone, its commands take no
`--config`, and that credential acts as the agent, in that turn's session, on
that turn's print, and no other. No command takes either as an option, because
the closure that matters is the closure of the action surface: a path to a file
reaches no printer.

**A value that may be given more than once says so.** `printer-history`'s and
`events`' `--kinds`, `events`' `--exclude-kinds`, and `watch`'s `--kind`,
`--exclude-kind` and `--sample` are those: each one given names one more value.
Every other option is given at most once, and one given twice is refused.

**Every change carries a reason, and an empty one is none.** A command that
changes something takes `--reason`, and one given nothing but whitespace is
refused with `usage` before any request is made.

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
- `unconfigured` (4) — nothing configured this program with a server to talk to, or the server refused the credential it was configured with; the message says where the credential is read from and that `credential issue` issues one.
- `rejected` (5) — the policy refused the action, the filament check refused a start, or the spool ledger refused a spool command, and the rejection is the answer. Beside it the program prints what to do next: for a value out of bounds, ask again inside the range the answer carries; for `insufficient_filament`, `unverified_spool` and `analysis_unavailable`, the spool, its figures and its mark or the file, and the commands that settle each; for `spool_refused`, the ledger's own words.
- `image-elsewhere` (6) — the answer named an image path, and no file is there on this host.
- `refused` (7) — the supervisor answered something this program will not act on, a `403` among them: a request whose `--actor` is not the identity its credential authenticated, which names another print than a turn's own, or which asks a turn's credential for `start-print` or `manifest-set`.

## The commands

### server

Runs the supervisor. This, `sign-in` and the two `credential` commands are the four commands that are not requests to an already-running one. `watch` is the one command that is requests to a running one without being one operation: it asks `events` again and again.

**Arguments.** None of its own; the four global options are the whole of it.

**Output.** The address it is serving on, on standard error, and then nothing until it stops.

**Failures.** `unconfigured` when the configuration file is absent, unreadable or incomplete, naming the field; `usage` when the arguments name nothing this program does.

### sign-in

Signs the supervisor's harness in, as the user it is run as — which is meant to be the user the service runs as. It reads the state directory and the harness from the server's own configuration file and nothing else, keeps the harness's sign-in in that harness's own directory under the state directory, and runs the harness's own interactive sign-in there on the terminal it was run from. Every supervision turn the server runs is pointed at the same directory. It starts no server and reaches no printer.

**Arguments.** None of its own. `--config` names the server's configuration file, and the default is the one the installer writes.

**Output.** One line on standard error naming the harness, its sign-in and the directory it is kept in, and then whatever the harness itself prints and asks. It exits with the harness's own status.

**Failures.** `unconfigured`, before anything is run, when the configuration file cannot be read for its state directory and its harness, when that harness is not one this program can sign in — naming it and the ones it can — when the directory cannot be created, and when the harness's program is not on the caller's path.

### credential issue

Issues the operator a credential: 32 bytes of the operating system's secure random source, written with the server's address into the operator's own client configuration — `printobserver/client.toml` under their configuration home (`$XDG_CONFIG_HOME`, else `$HOME/.config`, on Linux; `$HOME/Library/Application Support` on macOS; `%APPDATA%` on Windows), private to them — which every command reads first. Run it as yourself, unprivileged. It reaches no server.

**Arguments.**

- `--replace` — replace an operator configuration already there; without it, one that is there is left alone and the command is refused. The old credential stops working once the server is given the new verifier and restarted.

It writes that file and no other, so it takes no `--config`: a path somebody named could be one other users can read. The address written is `PRINTOBSERVER_SERVER` when that is set, and `http://127.0.0.1:8420` otherwise.

**Output.** One line for the server's configuration, `api.credential_verifier = "sha256:…"`, the SHA-256 of the credential, followed by comment lines saying where to put it — above the configuration's first table — and to restart the service. The credential itself is never printed. With `--json`, a document carrying `credential_verifier` and `client_config`.

**Failures.** `unconfigured` when a configuration is already there and `--replace` was not given, when `PRINTOBSERVER_SERVER` names no address, when there is no configuration home to write into, or when the file cannot be written — a configuration already there is then left as it was; `refused` when the random source refuses; `usage` when given `--config`.

### credential verifier

Prints the verifier of the one credential standard input carries — for a credential an operator chose themselves, or the plaintext an older `api.credential` holds. A credential chosen by hand should be long and random: a verifier of a short or guessable one can be searched offline. It reaches no server and writes nothing.

**Arguments.** None of its own; the credential is read from standard input, never from an argument, and one line terminator after it is set aside.

**Output.** The same `api.credential_verifier = "sha256:…"` line `credential issue` prints, and never the credential. With `--json`, a document carrying `credential_verifier`.

**Failures.** `usage` when standard input carries nothing a credential can be — empty, more than one line, a control character, not text — quoting none of it, and when given `--config`, since it reads no configuration.

### prints

Every print the supervisor holds, most recently opened first, and which of them the printer's current job belongs to. It is where the `--print-id` every other print command takes comes from.

Reached at `GET /v1/prints`.

**Arguments.** None of its own; the four global options are the whole of it.

**Output.** Every print record, and `active` — the ID of the print the printer's current job belongs to — when the printer reports a job it is printing or has paused. When no open print carries that job's file name, reading this opens one for it and names it; reading again while the same job runs opens nothing further. An open print's recorded state is `printing` whether or not the printer is paused now; `status` reads what the printer is doing. When the printer cannot be read, the prints are still listed and `active` is absent.

**Failures.** `unreachable` when nothing answers at the configured address; `refused` when the supervisor answers something this program will not act on.

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

### events

The event log after a cursor, oldest first in the order it was stored, waiting for an event to pass. `watch` follows it as one stream.

Reached at `GET /v1/events`.

**Arguments.**

- `--after` — the event to read after; without it the read starts after the newest event stored, or with `--print-id` at that print's first event (optional).
- `--print-id` — scope the read to that print's events and every event belonging to no print (optional).
- `--kinds` — one kind to pass; give it again for another (optional, repeatable).
- `--exclude-kinds` — one kind never to pass; give it again for another (optional, repeatable).
- `--wait-s` — how many seconds to wait, 0 to 60, for an event to pass, answering the moment one is stored (optional; 0 by default).
- `--limit` — how many events to answer, 1 to 500 (optional; 100 by default).

**Output.** `events`, each exactly as `history` prints it; `cursor`, the last event the read examined, passing or not, to give as `--after` next; and with `--print-id`, `print_ended` — the print's end as stored, its `print_ended` event's `event_id`, `received_at` and `outcome`, whatever the kind filters say — or `null` while the print is open.

**Failures.** `refused` for a malformed kind, a `--wait-s` or `--limit` out of range, an `--after` naming no event and a `--print-id` naming no print, each named; for a supervision turn's credential, a read naming no print or another print. `unreachable` and `refused` as for every command.

### look

A fresh look at the print, as it is now rather than when its event arrived.

Reached at `GET /v1/prints/{print_id}/look`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--wait-s` — how many seconds to wait first, at most 90, returning the moment an event arrives for the print's running turn (optional; a longer wait is refused rather than shortened).

**Output.** The look as it was written into the print's history, the events that arrived while it waited, the frame the camera gave and the absolute path on the server's own host it was stored at, the printer and the job as they are now, and whether the print is held by the detector's pause. With no camera configured, or one that gave no frame, the frame and its path are absent.

**Failures.** `image-elsewhere` when the frame's path names no file on the host this command ran on: the rest of the answer is printed and the path is replaced by a line saying why. `refused` for a wait over 90. `unreachable` and `refused` as for every command.

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

### spools

Every spool the ledger holds, the retired ones only when asked for, each beside one line saying what it holds — and the charges a tool is holding after a filament change nobody has confirmed.

Reached at `GET /v1/spools`.

**Arguments.**

- `--include-retired` — `true` to list the retired spools too (optional; left out, they are not listed).

**Output.** `at_a_glance` first: one line per spool, in the order `spools` lists them — `<name> | <material> | <remaining> g of <initial> g left | on tool <n>`, or `| not loaded`, then `| verified`, or `| unverified: <the oldest mark's reason>` followed by `(+<k> more)` when the spool holds more marks, with `| retired` at the end of a retired spool's line and both figures to one decimal. Then `pending`, every charge held for a tool after an unconfirmed filament change — what the next spool loaded onto that tool is charged — and `spools`, every spool's whole record, most recently registered first.

**Failures.** `unreachable` and `refused` as for every command.

### spool

One spool, retired or not, and the newest twenty of the printer's own events that name it.

Reached at `GET /v1/spools/{spool_id}`.

**Arguments.**

- `--spool-id` — the spool this command is about (required).

**Output.** The spool's whole record — what it held full and holds now, its diameter, density and tare, the tool it is on, when it was registered and retired, and every mark that puts its figure in doubt — and `recent`, the newest twenty events naming it, newest first.

**Failures.** `refused` for a spool nothing holds, which the server answers `404`. `unreachable` and `refused` as for every command.

### printer-history

The printer's own history, newest first, a page at a time: every print started and ended, every charge of filament, every spool command and every refusal of one, every filament check a start made, every filament change signalled, and every print the printer counted that nothing watched. A print's own `history` is that print's events; this is the printer's.

Reached at `GET /v1/history`.

**Arguments.**

- `--kinds` — one kind to read, and it may be given more than once, each another kind (optional; left out, every kind). The kinds are `print_started`, `print_ended`, `filament_consumed`, `spool_registered`, `spool_loaded`, `spool_unloaded`, `spool_adjusted`, `spool_retired`, `spool_command_rejected`, `spool_unverified`, `filament_change_signalled`, `unaccounted_print`, `preflight_checked`, `charge_pended`, `print_untracked` and `file_tally_rebaselined`; any other is refused.
- `--since` — the earliest instant to read from, inclusive, in RFC 3339 (optional).
- `--until` — the latest instant to read to, inclusive (optional).
- `--spool-id` — only the events naming this spool (optional).
- `--before` — only the events older than this one: the `next` a page answers (optional).
- `--limit` — how many events to answer (optional; 100 when left out, and more than 1,000 is refused).

**Output.** `events`, newest first, and `next`, the cursor to give as `--before` for the following, older page — absent on the last page.

**Failures.** `refused` for a kind outside this list, a limit above 1,000, or an instant or identifier spelled any other way. `unreachable` and `refused` as for every command.

### preflight

Whether the spools loaded hold enough for one file, read now. It records nothing, starts nothing and waits for no analysis.

Reached at `GET /v1/preflight`.

**Arguments.**

- `--file-name` — the file, as the printer names it (required).

**Output.** For each tool `OctoPrint`'s analysis of the file says it feeds: the spool loaded there and its name, what the print needs from it in grams, the buffer asked beyond that, what the spool holds, the spool's marks, and a verdict — `passed`, `insufficient`, `unverified_spool` or `unchecked`, the last for a tool no tracked spool is on — and the worst of them for the whole print. A file with no analysis yet answers `no_analysis` for every loaded spool, or `unverified_spool` for one in doubt.

**Failures.** `refused` for a file name the printer could not have. `unreachable` and `refused` as for every command.

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

Start a print of a named file, bounded by a manifest. Valid only while the printer is operational. When no print is open after the start reads the printer, it opens one for the file, and `record.print_id` in the output names it: act on that print afterwards.

Reached at `POST /v1/prints/{print_id}/actions/start_print`.

**Arguments.**

- `--print-id` — the print this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--file-name` — the file to print, as the printer names it (required).
- `--manifest` — the manifest itself, as JSON (required).
- `--manifest-file` — the path of a file carrying that JSON instead (optional).
- `--override-filament-check` — written `--override-filament-check true` to start despite a spool too short or in doubt, or a file `OctoPrint` has not analysed, with this start's reason as the override's (optional). Only an operator's override counts; it is recorded on the action and on the filament check the start made.
- `--reason` — why this change is being made, in the actor's own words (required).

Before the machine is asked, the start checks the spools loaded against what `OctoPrint`'s analysis of the file says each tool needs, waiting for that analysis when the file has none yet and a tool holds a tracked spool. In the default mode a start is refused while a loaded spool holds less than the print needs plus its buffer, while its figure is in doubt, or while the file still has no analysis; the server's `[spools]` table can set it to warn instead, or to check nothing.

**Output.** The action record, carrying the request, who asked, the reason and the policy's decision; the bounded intervention it opened, when it opened one; and what the printer said, when the request reached it and it refused.

**Failures.** `rejected` when the policy refuses, and when the filament check does: `insufficient_filament`, `unverified_spool` or `analysis_unavailable`, the record carrying the spool, its name and its figures, or the file and how long the start waited. The printer is not started, and the program says beside the record what settles it — load a spool holding enough, weigh the spool and run `adjust-spool`, start again once `OctoPrint` has analysed the file, or start again with `--override-filament-check true`. `refused` when the policy accepted it and the machine did not. `unreachable` when nothing answers at the configured address.

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

### register-spool

Register one spool on the shelf.

Reached at `POST /v1/spools/actions/register_spool`.

**Arguments.**

- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--name` — your own label for it (required).
- `--material` — the material, such as `PLA` (required).
- `--color` — the filament's colour (optional).
- `--diameter-mm` — the filament's diameter (optional; 1.75 when left out).
- `--density-g-cm3` — the material's density (optional; the known density of the material when left out, and a material with none known is refused).
- `--initial-g` — the net filament on the spool when full, in grams (required).
- `--remaining-g` — the filament on it now (optional; the full figure when left out).
- `--tare-g` — the empty spool's own weight, which a scale reading needs (optional).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The spool command's answer: its `decision`, the spool as the command left it, and every event it wrote, in order.

**Failures.** `rejected` when the actor may not ask for it — `actor_may_not_request`, which the safety envelope's grants decide, and a supervision turn's agent is granted no spool command — or when the ledger will not carry it out — `spool_refused`, whose `detail` says why and what to do instead, and which the program prints beside the record. Both are recorded. `usage` when the reason is empty, before any request is made. `refused` for an `--actor` the credential is not. `unreachable` when nothing answers at the configured address.

### load-spool

Load a spool onto a tool. Onto an occupied tool it is a swap, and the spool there is unloaded in the same write. During a print, what the tool fed up to now is charged to the spool that was feeding it. After a filament change nobody confirmed, loading another spool takes every charge held for that tool; naming the spool already there confirms it was never swapped.

Reached at `POST /v1/spools/{spool_id}/actions/load_spool`.

**Arguments.**

- `--spool-id` — the spool this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--reason` — why this change is being made, in the actor's own words (required).
- `--tool` — the tool, in the printer's own numbering (required).

**Output.** The spool command's answer: its `decision`, the spool as the command left it, and every event it wrote, in order.

**Failures.** `rejected` when the actor may not ask for it — `actor_may_not_request`, which the safety envelope's grants decide, and a supervision turn's agent is granted no spool command — or when the ledger will not carry it out — `spool_refused`, whose `detail` says why and what to do instead, and which the program prints beside the record. Both are recorded. `usage` when the reason is empty, before any request is made. `refused` for an `--actor` the credential is not. `unreachable` when nothing answers at the configured address.

### unload-spool

Unload a spool from its tool. During a print, what the tool fed up to now is charged to it first.

Reached at `POST /v1/spools/{spool_id}/actions/unload_spool`.

**Arguments.**

- `--spool-id` — the spool this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The spool command's answer: its `decision`, the spool as the command left it, and every event it wrote, in order.

**Failures.** `rejected` when the actor may not ask for it — `actor_may_not_request`, which the safety envelope's grants decide, and a supervision turn's agent is granted no spool command — or when the ledger will not carry it out — `spool_refused`, whose `detail` says why and what to do instead, and which the program prints beside the record. Both are recorded. `usage` when the reason is empty, before any request is made. `refused` for an `--actor` the credential is not. `unreachable` when nothing answers at the configured address.

### adjust-spool

Correct a spool's figure: from what is left, or from a scale reading of the whole spool, which its tare turns into what is left. Give exactly one of the two. A correction clears every mark on the spool.

Reached at `POST /v1/spools/{spool_id}/actions/adjust_spool`.

**Arguments.**

- `--spool-id` — the spool this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--gross-g` — a scale reading of the spool, in grams (optional; needs the spool's tare).
- `--reason` — why this change is being made, in the actor's own words (required).
- `--remaining-g` — the filament left, in grams (optional).

**Output.** The spool command's answer: its `decision`, the spool as the command left it, and every event it wrote, in order.

**Failures.** `rejected` when the actor may not ask for it — `actor_may_not_request`, which the safety envelope's grants decide, and a supervision turn's agent is granted no spool command — or when the ledger will not carry it out — `spool_refused`, whose `detail` says why and what to do instead, and which the program prints beside the record. Both are recorded. A command giving both figures or neither, and a scale reading of a spool registered with no tare, are `spool_refused`. `usage` when the reason is empty, before any request is made. `refused` for an `--actor` the credential is not. `unreachable` when nothing answers at the configured address.

### retire-spool

Retire a spool. A retired spool takes no further command, and is listed only when the retired ones are asked for.

Reached at `POST /v1/spools/{spool_id}/actions/retire_spool`.

**Arguments.**

- `--spool-id` — the spool this command is about (required).
- `--actor` — who is asking (required).
- `--actor-file` — the path of a file carrying that value instead (optional).
- `--reason` — why this change is being made, in the actor's own words (required).

**Output.** The spool command's answer: its `decision`, the spool as the command left it, and every event it wrote, in order.

**Failures.** `rejected` when the actor may not ask for it — `actor_may_not_request`, which the safety envelope's grants decide, and a supervision turn's agent is granted no spool command — or when the ledger will not carry it out — `spool_refused`, whose `detail` says why and what to do instead, and which the program prints beside the record. Both are recorded. `usage` when the reason is empty, before any request is made. `refused` for an `--actor` the credential is not. `unreachable` when nothing answers at the configured address.

### watch

Follows what the supervisor records as one stream, filtered by the supervisor rather than by the caller, until the watched print ends, a signal stops it, the supervisor cannot be reached, or the supervisor refuses it. It asks `events` again from its own cursor for as long as it runs, so a consumer runs no polling loop of its own.

Composes `GET /v1/events`.

**Arguments.**

- `--print-id` — follow that print's events and every event belonging to no print, replayed from the print's first event, and end when the print ends (optional; without it, every event from now on, and the stream ends only by a signal or a failure).
- `--after` — start after this event rather than where `--print-id` or now would (optional).
- `--kind` — one kind to pass; give it again for another (optional, repeatable).
- `--exclude-kind` — one kind never to pass; give it again for another (optional, repeatable).
- `--sample` — `KIND=SECONDS`: pass the first event of that kind and then at most one per that many seconds by `received_at`, dropping the rest — `--sample periodic_observation=300` keeps one routine observation every five minutes (optional, repeatable).
- `--heartbeat-s` — with nothing else to print for that many seconds, print a heartbeat (optional; 0, the default, prints none).
- `--reconnect-s` — how long to keep retrying from the cursor while the supervisor cannot be reached (optional; 30 by default).

**Output.** With `--json`, every line is one JSON object: each passing event exactly as `history --json` prints it; `{"watch_heartbeat": {"at": "<timestamp>", "cursor": "<event id>" | null}}` each `--heartbeat-s` seconds with nothing else to print; and always, as the last line, `{"watch_ended": {"reason": "print_ended" | "interrupted" | "unreachable" | "refused", "detail": "<text>", "cursor": "<event id>" | null}}`. Watching a print, every passing event stored before its end is printed before the ending, and no event the filters exclude is printed — the `print_ended` event included, which is printed only when the filters pass it. A watch started or reconnected after the print ended replays what is left and then ends. Without `--json`, one line per event and the same ending in words, on standard error for `unreachable` and `refused`; that rendering is not a contract.

**Failures.** Each is its own ending line and exit: `print_ended` and `interrupted` (SIGINT or SIGTERM) exit `success`; `unreachable` once the supervisor has not answered for longer than `--reconnect-s`; `refused`, at once and never retried, when the supervisor refuses the credential or the request — a malformed `--kind` among them. A malformed `--sample`, or a number of seconds below zero — the only bound either takes — is refused with `usage`, and a configuration this program cannot read with `unconfigured`, before any stream starts and without an ending line.
