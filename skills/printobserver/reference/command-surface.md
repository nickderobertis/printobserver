# The command surface

Every command the `printobserver` program has, one entry each. This document
covers what a command is made of, the four global options, the exit statuses,
and the commands — each with its arguments, its output and its failures.

Nothing here is a list somebody maintains. The set of commands is one per
operation the server serves, and each command's options are that operation's own
declared request — for an action, the fields the contracts' action vocabulary
declares for that variant. The program writes that declaration out as
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
- `rejected` (5) — the policy refused the action, and the rejection is the answer.
- `image-elsewhere` (6) — the answer named an image path, and no file is there on this host.
- `refused` (7) — the supervisor answered something this program will not act on, a `403` among them: a request whose `--actor` is not the identity its credential authenticated, which names another print than a turn's own, or which asks a turn's credential for `start-print` or `manifest-set`.

## The commands

### server

Runs the supervisor. This, `sign-in` and the two `credential` commands are the four commands that are not requests to an already-running one.

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

`--config <path>` names the file to write instead. The address written is `PRINTOBSERVER_SERVER` when that is set, and `http://127.0.0.1:8420` otherwise.

**Output.** One line for the server's configuration, `api.credential_verifier = "sha256:…"`, the SHA-256 of the credential, followed by comment lines saying where to put it — above the configuration's first table — and to restart the service. The credential itself is never printed. With `--json`, a document carrying `credential_verifier` and `client_config`.

**Failures.** `unconfigured` when a configuration is already there and `--replace` was not given, when `PRINTOBSERVER_SERVER` names no address, when there is no configuration home to write into, or when the file cannot be written; `refused` when the random source refuses.

### credential verifier

Prints the verifier of the one credential standard input carries — for a credential an operator chose themselves, or the plaintext an older `api.credential` holds. A credential chosen by hand should be long and random: a verifier of a short or guessable one can be searched offline. It reaches no server and writes nothing.

**Arguments.** None of its own; the credential is read from standard input, never from an argument, and one line terminator after it is set aside.

**Output.** The same `api.credential_verifier = "sha256:…"` line `credential issue` prints, and never the credential. With `--json`, a document carrying `credential_verifier`.

**Failures.** `usage` when standard input carries nothing a credential can be — empty, more than one line, a control character — quoting none of it.

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
