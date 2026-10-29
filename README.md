# printobserver

printobserver is for people who want an AI agent to help supervise a 3D print
without giving that agent unrestricted control of the printer.

[OctoPrint](https://octoprint.org/download/) drives the printer, and
[Obico](https://www.obico.io/docs/user-guides/octoprint-plugin-setup/) watches
the camera and raises failure alerts. printobserver sits between those systems
and the agent. It lets the agent inspect the print and make bounded, reversible,
reasoned interventions through the same commands a person can audit.

Every change requires a `--reason`. An adjustment can include `--duration-s`,
after which printobserver tries to restore the prior value. The intervention
policy refuses values outside the configured safety envelope and grants actions
separately to the operator, agent, and system. You can therefore give an agent
fewer permissions than a person. These limits matter because the service
commands a physical machine that can be damaged.

## How it fits together

```text
camera → Obico → failure webhook → printobserver → supervising agent
printer ← OctoPrint ← allowed action ← policy ← agent decision
```

`printobserver server` is the supervisor. The recommended install runs it as
you: a systemd user unit on Linux, a LaunchAgent on macOS and a Scheduled Task
on Windows. Linux lingering covers boot without login; macOS and Windows need
a logged-in user. Each manager brings it back after a crash. The
[isolated install](#isolated-install) runs it under a system account instead.
It reads printer and job state through
OctoPrint's API using the configured address and API key. It also exposes an
ingress for alerts posted by Obico's webhook notification plugin; when an alert
arrives, it immediately fetches the snapshot named by that alert.

The server exposes an HTTP API that serves only requests carrying its API
credential. Every other `printobserver` subcommand, the Rust, Python, and Node
clients, and the agent use that same API and command surface. The oneharness adapter in `crates/printobserver-oneharness` runs one
supervision turn for each event on the harness identity configured under
`[supervisor]`. The turn's system prompt is
[the PrintObserver skill](./skills/printobserver/SKILL.md), an Agent Skill this
repository publishes and the program does not carry: a host installs it with
`gh skill install nickderobertis/printobserver printobserver`, and
`supervisor.skill_path` names the installed `SKILL.md`. The agent runs from the
skill's own directory, where the reference documents it links to sit, and acts
through the same commands an operator uses.

Every action request is recorded with its actor and reason, together with the
policy decision and, when accepted, its outcome. The history lets a person
audit what the agent requested, why, and what happened. See
[the architecture](./skills/printobserver/reference/architecture.md),
[the intervention policy](./skills/printobserver/reference/intervention-policy.md), and
[the API and clients](./skills/printobserver/reference/api-and-clients.md) for details.

## Supported platforms

printobserver runs on Linux (`linux-x86_64` and `linux-aarch64`), on macOS on
Apple silicon (`macos-aarch64`), and on Windows (`windows-x86_64` and
`windows-aarch64`). Every one of those is a first-class platform: the gate and
the real-OctoPrint integration tier run on each of them on every change, a
release is built for each of them, and each of the three install routes below
is proven on each of them from its registry. The list that decides this is
[`AGENTS.md`'s supported-platform list](./AGENTS.md#supported-platforms); a
platform is supported when it is on that list, and nothing else here can add or
remove one.

Where the instructions below differ by operating system, they say so and give
each one's own answer. Where they do not, the command is the same on all three.

## Set it up on a real printer

### 1. Set up OctoPrint

Install [OctoPrint or OctoPi](https://octoprint.org/download/), connect it to
the printer, and confirm that it can run a job. Generate an API key for
printobserver. OctoPrint documents
[access control and user API keys](https://docs.octoprint.org/en/main/features/accesscontrol.html),
the [Application Keys plugin](https://docs.octoprint.org/en/main/bundledplugins/appkeys.html),
and [REST API authorization](https://docs.octoprint.org/en/main/api/general.html).
Keep the OctoPrint base URL and generated key for step 5.

### 2. Set up self-hosted Obico

This repository supports the webhook notification plugin of a **self-hosted
Obico server**. The tree does not establish that Obico Cloud can send this
webhook to printobserver. Follow Obico's
[self-hosted server guides](https://www.obico.io/docs/server-guides/) and
[server installation guide](https://www.obico.io/docs/server-guides/install/),
then [connect the OctoPrint plugin](https://www.obico.io/docs/server-guides/configure-octoprint-plugin/).
Obico also documents its general
[plugin setup](https://www.obico.io/docs/user-guides/octoprint-plugin-setup/) and
[manual linking flow](https://www.obico.io/docs/user-guides/octoprint-plugin-setup-manual-link/).

In Obico's [notification settings](https://www.obico.io/docs/user-guides/notification-settings/),
set the webhook plugin's custom URL to:

```text
http://PRINTOBSERVER_HOST:8420/obico/webhook?token=YOUR_SHARED_SECRET
```

Use an address and port the Obico server can reach. Choose a long random
`YOUR_SHARED_SECRET` and put the identical value in `ingress.shared_secret` in
step 5. Because the plugin configures only a URL, the `token` query parameter is
how it carries the secret.

The template's listen address accepts connections only from the same machine.
If Obico runs elsewhere, set `listen` in step 5 to an address on the
printobserver machine that Obico can reach and use it in the webhook URL.

### 3. Install printobserver

On Linux and macOS, the recommended install runs as you, with no administrator
step for installation, upgrades, configuration changes or harness sign-in:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh
```

It installs the program in `~/.local/bin`, writes your configuration only when
absent, and starts a systemd user unit on Linux or a LaunchAgent on macOS.
Run it again to upgrade in place, keeping your configuration and state.
`--client-only` installs only the program; `--observation-interval-seconds N`
sets the observation interval for a new configuration.

Linux enables lingering so supervision resumes after reboot without a login.
If the host refuses it, the installer completes, explains that supervision then
requires a login, and prints `loginctl enable-linger "$USER"` to enable it.
On macOS supervision runs only while you are logged in; auto-login covers a
dedicated machine. On every platform supervision turns run as you: the agent
can read your client configuration, so policy is a guard rail rather than an
OS isolation boundary. Choose the [isolated install](#isolated-install) for that.

On Windows, one command from a PowerShell of your own — not an elevated one —
installs the program and runs its server as you. This is the recommended
install, and no step of it, nor any upgrade, sign-in or configuration change
after it, needs an elevated PowerShell:

```powershell
irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1 | iex
```

It puts the program in `%LOCALAPPDATA%\Programs\printobserver` and on your
path, writes your own server configuration and state directory under
`%LOCALAPPDATA%\printobserver` — the configuration only when none is there —
and registers and starts a Scheduled Task named `printobserver` that runs the
server as you. It prints the steps below that are left. Running the same command
again upgrades in place: it stops the task, waits for the server to exit,
replaces the program, keeps your configuration and state, and starts it again.
It also takes `-ObservationIntervalSeconds`, as the service installer does, and
`-ClientOnly` installs the program alone, for a machine whose server is the
[isolated install](#isolated-install).

Two trade-offs come with running the server as you, and they are why the
isolated install exists:

- **It runs only while you are logged on.** After a reboot or a power loss,
  nothing is watched until somebody logs in. On a print PC dedicated to the
  printer, turning on automatic logon covers that.
- **Supervision turns run as you.** The agent could read your own client
  configuration, and with it act beyond what its own policy grants, so the
  per-actor policy is a guard rail rather than a hard boundary.

If a machine already runs the printobserver service, this command refuses and
names the [one elevated step that moves it](#moving-from-the-windows-service-to-the-per-user-install).

On Linux and macOS, choose one of these three routes. Each installs a program
already built for your platform and needs no Rust toolchain on the printer host.
These commands are held by a check to the one statement of the install path the
repository keeps, so they cannot drift from it. The first two install the
program alone on Windows too.

With Python, on Linux, macOS or Windows:

```console
pip install printobserver-cli
```

With Node, on Linux, macOS or Windows:

```console
npm install -g printobserver-cli
```

Or with the bundled installer, which needs neither package manager. On Linux
and macOS:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh
```

It also accepts a release and destination:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh -s -- --version v0.1.0 --to ~/.local/bin
```

On Windows, the command above is this installer's PowerShell form. It takes the
same two options:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1))) -Version v0.1.0 -To C:\Tools\printobserver
```

Whichever route you took, check the program. It prints the version it is,
which is the one thing the install commands cannot tell you:

```console
printobserver --version
```

### 4. Locate your configuration

The bundled installer in step 3 already installed and started the per-user
server. If you chose Python or Node, run the bundled installer now to set up
the server for your account.

Where the installer puts things differs by platform. The rest of this guide
names the three places by role — the configuration file, the state directory,
the program — and this table is what each role means on your machine:

| Platform | Configuration file | State directory | Program |
| --- | --- | --- | --- |
| Linux | `$HOME/.config/printobserver/config.toml` | `$HOME/.local/state/printobserver` | `$HOME/.local/bin/printobserver` |
| Linux, isolated install | `/etc/printobserver/config.toml` | `/var/lib/printobserver` | `/usr/local/lib/printobserver/printobserver` |
| macOS | `$HOME/Library/Application Support/printobserver/config.toml` | `$HOME/Library/Application Support/printobserver/state` | `$HOME/.local/bin/printobserver` |
| macOS, isolated install | `/etc/printobserver/config.toml` | `/var/lib/printobserver` | `/usr/local/lib/printobserver/printobserver` |
| Windows | `%LOCALAPPDATA%\printobserver\config.toml` | `%LOCALAPPDATA%\printobserver\state` | `%LOCALAPPDATA%\Programs\printobserver\printobserver.exe` |
| Windows, isolated install | `C:\ProgramData\printobserver\config.toml` | `C:\ProgramData\printobserver\state` | `C:\Program Files\printobserver\printobserver.exe` |

These are the program's own answers, from `crates/printobserver/src/locations.rs`;
each installer writes exactly them, and a test holds this table to that module.
Linux honors `XDG_CONFIG_HOME` and `XDG_STATE_HOME` instead of the defaults
above when set. Configuration is `0600` and state `0700`, owned by you.
The per-user configuration and state on every platform are yours to edit and
read, including the images and turn records, without elevation.

### 5. Configure the supervisor

Edit the configuration file — as root on Linux and macOS, as yourself on
Windows:

- `state_dir` holds the database, images, sessions, the agent's prompt assets
  and, under `skills`, the agent's installed skill; the template uses your
  platform's state directory from the table above.
- `listen` serves both the HTTP API and Obico ingress. The template uses
  `127.0.0.1:8420`; change it if Obico is on another machine.
- `octoprint.url` is the OctoPrint base URL from step 1.
- `octoprint.api_key` is the API key generated in step 1.
- `octoprint.fan` is `commandable` when printobserver may command the
  part-cooling fan, or `absent` when the printer has none.
- `supervisor.harness` selects the oneharness identity for supervision turns;
  the template selects `claude-code`.
- `supervisor.skill_path` names the installed agent skill's `SKILL.md`; the
  template names `skills/printobserver/SKILL.md` under your state directory,
  which is where step 6 installs it. It is required: printobserver carries no
  skill of its own, and the service refuses to start, naming this field, when
  it names nothing readable. A configuration written before this field was
  required has no such line: add it, and take step 6.
- `ingress.shared_secret` is the private random value used in step 2. Anyone
  who has it can submit an alert that may lead to a printer action.
- `api.credential_verifier` is the one line `printobserver credential issue`
  prints, below: `sha256:` and the SHA-256 of your operator credential. Every
  request to the HTTP API must carry a credential, and the server keeps only
  this verifier of yours, never the credential itself, so nothing a supervision
  turn can read authenticates as you. Left out, the server reads
  `api-credential.verifier` in its state directory, and with neither it
  supervises and refuses every operator request, naming `printobserver
  credential issue`. Your credential is separate from `ingress.shared_secret`,
  and neither is accepted in place of the other.
- `api.credential`, a credential in plaintext, is what earlier versions read.
  It is still admitted, with a warning in the service's log on every start,
  until you replace it with `api.credential_verifier`; see
  [upgrading](#upgrading-from-030-or-earlier).
- `camera.snapshot_url` is optional, and the template carries it commented
  out. It is an `http` or `https` URL answering one still image of the print,
  such as go2rtc's `frame.jpeg` for the printer's camera; with it, the agent's
  `printobserver look` takes a fresh frame, and without it a look carries the
  printer's state and no frame.
- `obico.url` and `obico.access_token` are optional and given together, and the
  template carries them commented out. They are Obico's own address and an
  OAuth2 access token from its administration. When Obico paused a print and
  the agent adjusted it, the service resumes the print and acknowledges the
  alert through this API, which re-arms Obico's detection for the rest of the
  print. The token is never printed or logged.
- `safety.agent_min_interval_s` is the minimum time between agent actions that
  change a moving print; adjustments to a paused print do not wait on it.
  Under `safety.allowed`, set the allowed range for feedrate factor, flowrate
  factor, fan percentage, bed target, and each tool target. Under
  `safety.actions`, grant action names separately to `operator`, `agent`, and
  `system`. The template grants the system `resume`, which is how a print Obico
  paused is resumed once the agent's adjustment is in. Choose limits suitable
  for your printer and material. A print manifest may narrow them but cannot
  widen them.
- The five spool commands are granted like any other action. The template
  grants all five to the operator and none to the agent, which reads what is
  loaded and what a file needs but does not say which spool is on which tool.
  It grants `close_print` to the operator alone too, and a configuration
  granting it to the agent is refused at start: closing a print record is the
  operator's judgement that the print is over.

  ```toml
  [safety.actions]
  operator = ["pause", "resume", "cancel", "start_print", "set_feedrate_factor",
              "set_flowrate_factor", "set_tool_target_c", "set_bed_target_c",
              "set_fan_percent", "acknowledge_failure", "close_print",
              "register_spool", "load_spool", "unload_spool", "adjust_spool",
              "retire_spool"]
  ```

- `[spools]` is optional, and the template carries it commented out with its
  defaults. Before a start reaches the printer, the spool loaded on each tool is
  checked against what OctoPrint's analysis of the file says it needs, plus a
  buffer. `preflight` is `refuse` (the default), `warn` or `off`;
  `buffer_percent` and `buffer_min_g` set the buffer, the larger of the two
  counting; and `analysis_wait_seconds` is how long a start waits for an
  analysis of a file that has none yet. An operator can start a refused print
  anyway with `printobserver start-print ... --override-filament-check true`.

  ```toml
  [spools]
  preflight = "refuse"
  buffer_percent = 10.0
  buffer_min_g = 5.0
  analysis_wait_seconds = 300
  ```

The server validates these values at startup, including reaching OctoPrint and
authenticating its API key, and identifies a field it cannot accept.

#### Issue your operator credential

The service holds no credential you can authenticate with — only its verifier.
Issue yourself one as yourself, unprivileged, on any platform:

```console
printobserver credential issue
```

It draws a credential from 32 random bytes and writes it, with the address the
template has the service listen on, into your own client configuration —
`printobserver/client.toml` under your configuration home: `$XDG_CONFIG_HOME`,
else `~/.config`, on Linux; `~/Library/Application Support` on macOS;
`%APPDATA%` on Windows — readable by you alone. It prints one line and never the
credential:

```toml
api.credential_verifier = "sha256:0d3f…"
```

Put that line in the server's configuration above its first `[table]` header
(or as `credential_verifier = …` inside an `[api]` table) — the command names
the file, which on Windows is your own `%LOCALAPPDATA%\printobserver\config.toml`
— and the service reads it when it next starts. If you change `listen`, run it again with `--replace`
and `PRINTOBSERVER_SERVER` set to the new address, and put the new line in
place of the old one. To use a credential you chose instead, pipe it into
`printobserver credential verifier` for the same line; make it long and random,
because a verifier of a short or guessable credential can be searched offline.

From then on every command you run reads your own configuration first. With no
`--config`, a command reads `printobserver/client.toml` under your configuration
home, then your per-user server configuration when it is there,
and then the service's own configuration file for whatever those left unnamed
(it is private to the service, so from your own account that read is passed
over). `--config <path>` reads that one file instead. `PRINTOBSERVER_SERVER` and
`PRINTOBSERVER_CREDENTIAL` win over any file, and when both are set and no
`--config` is given no file is read at all. A command the server refuses exits
with status 4 and says where the credential is read from.

#### What a supervision turn is handed

Each supervision turn is minted a credential of its own when it starts, handed
to the agent's harness in its environment as `PRINTOBSERVER_SERVER` and
`PRINTOBSERVER_CREDENTIAL` and written to no file, and revoked when the turn
ends — on success, failure or timeout — or the service restarts. It acts as the
agent, in that turn's session, on that turn's print, and nothing else: a request
claiming to be the operator or the system, naming another print, starting a
print or replacing a manifest is refused with `403` before anything is decided,
and the agent's minimum interval holds every change it asks for. The service no
longer writes `client.toml` into its state directory, and deletes one an earlier
version left there.

#### The boundary that remains

When the server and you run as the same OS user, a supervision turn runs as you
too: it can read your own client configuration, and the environment of another
turn running at the same time — through `/proc/<pid>/environ` on Linux, or
`ps -E` on macOS. That is the recommended Windows install, by design: the agent
could act beyond its own grant through your credential, so there the per-actor
policy is a guard rail rather than a hard boundary. Full isolation needs the server to run as an OS user of its own; the
[isolated install](#isolated-install) provides that on every platform.

### 6. Install the agent's skill

The agent's skill is not part of the program: it is the Agent Skill this
repository publishes, and the service reads it from `supervisor.skill_path`.
Install it under your state directory as yourself. It needs GitHub CLI 2.100.0 or later
and no GitHub sign-in: the repository is public.

On Linux:

```console
gh skill install nickderobertis/printobserver printobserver --dir "${XDG_STATE_HOME:-$HOME/.local/state}/printobserver/skills"
```

On macOS:

```console
gh skill install nickderobertis/printobserver printobserver --dir "$HOME/Library/Application Support/printobserver/state/skills"
```

On Windows:

```powershell
gh skill install nickderobertis/printobserver printobserver --dir $env:LOCALAPPDATA\printobserver\state\skills
```

Run it again to take a newer skill; restart the service afterwards, because the
skill is read when the service starts.

### 7. Install and sign in the agent's harness

There is no separate agent endpoint: when an event arrives, the server runs the
harness `supervisor.harness` selects, as your account. The harness keeps its sign-in under the state directory,
in `harness/<harness>` there.
printobserver can sign in `claude-code` and `codex`.

Install the harness into your own account, with its program on your path:

```console
npm install -g @anthropic-ai/claude-code
```

For `codex`, install its program instead:

```console
npm install -g @openai/codex
```

Then sign in as yourself. With no `--config`, this reads your per-user config:

```console
printobserver sign-in
```

This reads `state_dir` and `supervisor.harness` from the configuration file and
nothing else, so it works before or after you fill in the OctoPrint and Obico
values. It creates the harness's directory under the state directory, as
private as that directory is, and runs that harness's own interactive sign-in
in your terminal: `claude auth login`, or `codex login --device-auth`.
Follow its prompts. The command exits with the harness's own status. It starts
no service and contacts neither OctoPrint nor Obico.

Every supervision turn the service runs is pointed at the same directory, so
it uses that sign-in: an Obico failure webhook causes a turn using the installed
skill, and every action the agent requests goes through the policy. Run the
sign-in again if the harness's sign-in expires, or after changing
`supervisor.harness`.

### 8. Restart the server

After editing configuration and installing the skill and harness, restart it.
On Linux:

```console
systemctl --user restart printobserver.service
```

On macOS:

```console
launchctl kickstart -k gui/$(id -u)/io.github.nickderobertis.printobserver
```

On Windows:

```powershell
Stop-ScheduledTask -TaskName printobserver; Start-ScheduledTask -TaskName printobserver
```

Linux's lingering user unit runs without a login. On macOS and Windows it runs
while you are logged in; auto-login covers a dedicated machine. Every manager
brings the server back after a crash. On Windows use
`Disable-ScheduledTask -TaskName printobserver` to stop supervising.

### 9. Verify the installation

```console
printobserver --version
```

Start a print through OctoPrint. Then ask the running supervisor which prints it
holds. Every command reads the address and your credential from the client
configuration `printobserver credential issue` wrote in step 5:

```console
printobserver prints
```

`active` is the ID of the print OctoPrint is running. If no print was open for
that job, this read opens one, so the print has an ID before Obico has reported
anything. Reading again while the job runs opens nothing more, and the first
Obico alert about the job joins that same print. If the printer cannot be read,
the stored prints are still listed and `active` is absent. A print's recorded
`state` stays `printing` while it is open, even when the printer is paused;
`printobserver status` shows what the printer is doing now.

A print ends the first time any read of the printer — this listing, `status`, a
supervision turn, or the supervisor starting up — finds its job finished,
cancelled or failed. A later job of the same file gets a print of its own once a
read finds it has printed more than 120 seconds (the identity tolerance) less than the earlier job was
seen printing, and the earlier print ends with `a later job of the same file
replaced it`; a pause never splits a print. Two cases cannot be told from one
paused job and stay one print: a later job first read after it has printed about
as long as the earlier one was last seen printing, and a later job replacing one
no read ever saw printing. `start-print` opens a print of its own and names it in
its answer's `record.print_id`.

Make the first read with that ID in place of `PRINT_ID`:

```console
printobserver context --print-id PRINT_ID
```

## Isolated install

On Linux and macOS, take the bundled program installer with `--client-only`,
then install the isolated service, which deliberately starts nothing:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.sh | sudo sh
```

Configure `/etc/printobserver/config.toml` as above. Issue your credential as
yourself, paste its verifier there, and install the skill and harness for the
service account:

```console
sudo gh skill install nickderobertis/printobserver printobserver --dir /var/lib/printobserver/skills
sudo npm install -g @anthropic-ai/claude-code
sudo -u printobserver /usr/local/lib/printobserver/printobserver sign-in
```

The systemd unit on Linux and LaunchDaemon on macOS run at boot as the
`printobserver` system user. Start the Linux unit:

```console
sudo systemctl enable --now printobserver.service
```

Or load the macOS LaunchDaemon:

```console
sudo launchctl bootstrap system /Library/LaunchDaemons/io.github.nickderobertis.printobserver.plist
```

For a machine that wants an operating-system boundary between the agent and the
person using it, the server runs as a Windows service instead, under its own
virtual account, `NT SERVICE\printobserver`, with its configuration and state in
`C:\ProgramData\printobserver` readable only by administrators and that
account. Every step of it but the credential is taken in an elevated PowerShell:
each upgrade, each harness sign-in, each configuration change, and each look at
an image or a turn record. It is supported exactly as it always was.

Take step 3 with `-ClientOnly`, which puts the program on your path and installs
no per-user server — or either of the other two routes. Then, from an elevated
PowerShell, put the service in place; it registers the service to start on
demand, as its own virtual account, and to be brought back if its process dies,
and starts nothing:

```powershell
irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.ps1 | iex
```

Fill in its configuration as step 5 describes, from that elevated PowerShell.
Issue your credential from a PowerShell that is not elevated, and put its line
into `C:\ProgramData\printobserver\config.toml`:

```powershell
& 'C:\Program Files\printobserver\printobserver.exe' credential issue
```

Install the skill under the service's state directory, from the elevated
PowerShell:

```powershell
gh skill install nickderobertis/printobserver printobserver --dir C:\ProgramData\printobserver\state\skills
```

Install the harness with `npm install -g` from the elevated PowerShell. The
service's virtual account cannot be signed in to and does not need to be — the
sign-in lands under the state directory, which the installer made the service
account's to read — so sign in from that same elevated PowerShell:

```powershell
& 'C:\Program Files\printobserver\printobserver.exe' sign-in
```

Then set the service to start automatically, and start it, with the one command
that does both:

```powershell
Set-Service -Name printobserver -StartupType Automatic -Status Running
```

The service control manager starts it at every boot, whether or not anybody
logs in, and again if it ends abruptly. Starting is a command of its own because
this service commands a 3D printer: installing software must not start a process
that can move the machine — at install, or at the next reboot.

## Moving from a Linux or macOS system service

Run this one privileged step from your account:
The command forwards each Linux XDG directory only when you have set it, because
sudo otherwise strips those variables. Unset directories keep their defaults.

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/migrate-service-to-per-user.sh | sudo env ${XDG_CONFIG_HOME+"XDG_CONFIG_HOME=$XDG_CONFIG_HOME"} ${XDG_STATE_HOME+"XDG_STATE_HOME=$XDG_STATE_HOME"} sh
```

The source record is the absolute `state_dir` named in the service configuration,
including a nondefault location; the installed default is `/var/lib/printobserver`.
A config with no `state_dir` is refused without inserting a field, as on Windows.
With `--root`, that source and the per-user destinations are staged below the root.

A plain-string `supervisor.skill_path` outside the service layout is accepted and
rewritten too, as on Windows; the installer's next steps reinstall the skill
under the user state directory.

It uses `SUDO_USER`, stops the system service, copies configuration and all state
(database and WAL/SHM, images, sessions, harness sign-ins, skills, assets and
verifier), changes only `state_dir` and `supervisor.skill_path`, and verifies
the copy by SHA-256 before disabling and removing the system registration.
It refuses an existing per-user configuration or state and leaves originals
untouched on a failed copy. The old `/etc/printobserver`, `/var/lib/printobserver`,
`/usr/local/lib/printobserver` and service account remain; the script prints how
to remove them once satisfied. Then run step 3 and `printobserver sign-in` as
yourself, without sudo.

The Unix migration accepts bare and dotted table headers, such as `[supervisor]`,
in the form the generated config uses. Spaces around names/dots and trailing
comments are allowed. It refuses malformed or duplicate headers and quoted,
escaped or array-of-tables headers before stopping the service. Edit the source
config into that generated form or migrate by hand.

## Moving from the Windows service to the per-user install

One elevated step moves a machine that runs the service to the per-user
install, keeping every print record, image and turn. From an elevated
PowerShell:

```powershell
irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/migrate-service-to-per-user.ps1 | iex
```

It stops the service and waits for it to exit; copies its configuration and its
whole state directory — the database, the images, the turn records, the harness
sign-ins and the skill — into your `%LOCALAPPDATA%\printobserver`, owned by you;
rewrites only `state_dir` and `supervisor.skill_path` in the copy; and checks
every copied file against its original's SHA-256 before it deletes the service.
A copy that does not check out is removed again and the service left
registered. It refuses, changing nothing, from a PowerShell that is not elevated,
on a machine with no service, and when you already have a per-user
configuration. It leaves `C:\ProgramData\printobserver` and
`C:\Program Files\printobserver` where they are, and prints how to remove them
once you are satisfied.

If elevating runs it as another administrator, name yourself instead of that
account — your own local application data and your own account:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/migrate-service-to-per-user.ps1))) -LocalAppData C:\Users\operator\AppData\Local -User PRINTPC\operator
```

Then, as yourself and with no elevation, take step 3 — which keeps the copied
configuration and starts the server as you — and sign the harness in again:

```console
printobserver sign-in
```

## Using it

```console
printobserver --help
```

Every command reads the server's address and your credential from your own
client configuration, a file named with `--config`, or `PRINTOBSERVER_SERVER`
and `PRINTOBSERVER_CREDENTIAL`; see step 5.

Every print-specific command takes the print's ID as `--print-id`. Find it
first: `printobserver prints` lists every print, newest first, and names the one
OctoPrint is running as `active`. Then read that print's context before
intervening:

```console
printobserver prints
printobserver context --print-id PRINT_ID
```

Give every change a `--reason`. See
[common operations](./skills/printobserver/reference/common-operations.md) for a worked example
of every command, including a temporary adjustment with `--duration-s`, and
its output.

### Following a print as it runs

`printobserver watch` streams what the supervisor records, filtered by the
supervisor, so nothing beside it has to poll the printer, the detector or the
camera. Every flag is optional:

```console
printobserver watch --json --print-id PRINT_ID --sample periodic_observation=300 --heartbeat-s 60
```

- `--print-id ID` follows that print's events and every event that belongs to no
  print — connection changes, detector health, camera trouble — replayed from
  the print's first event, and ends when the print ends. Without it, every event
  from now on.
- `--kind K` passes only that kind and `--exclude-kind K` never passes it; give
  either again for another kind.
- `--sample K=SECONDS` passes the first event of kind K and then at most one per
  SECONDS by `received_at`, dropping the rest: the example keeps everything but
  routine observations, plus one observation every five minutes.
- `--after EVENT_ID` starts after that event.
- `--heartbeat-s N` prints a heartbeat after N seconds with nothing else to
  print; the default, 0, prints none.
- `--reconnect-s N` keeps retrying from where the stream got to for N seconds
  while the supervisor cannot be reached; 30 by default.

With `--json` every line is one JSON object:

- each passing event, exactly as `history --json` prints it;
- `{"watch_heartbeat": {"at": "<timestamp>", "since": "<timestamp>", "cursor": "<event id>" | null}}`
  with `--heartbeat-s`, where `since` is when the last line was printed — or the
  stream started — which the interval counts from;
- and always, last, `{"watch_ended": {"reason": "print_ended" | "interrupted" |
  "unreachable" | "refused", "detail": "<text>", "cursor": "<event id>" | null}}`.

The ending is how a consumer tells a stream that ended or failed from a quiet
print:

| Reason | When | Exit |
| --- | --- | --- |
| `print_ended` | the watched print ended — every passing event stored before its end is printed first, and the `print_ended` event only when the filters pass it | 0 |
| `interrupted` | SIGINT or SIGTERM | 0 |
| `unreachable` | the supervisor has not answered for longer than `--reconnect-s` | 3 |
| `refused` | the supervisor refused the credential or the request, such as a malformed `--kind`; never retried | 7 |

A malformed `--sample` exits 2, and a missing configuration 4, before the
stream starts and with no ending line. Without `--json`, `watch` prints one line
per event and the same ending in words — on standard error when it is
`unreachable` or `refused`; that rendering is not a contract.

`watch` is built on one read, `printobserver events` (`GET /v1/events`), which a
client can use directly: it answers the events stored after `--after` that pass
`--kinds` and `--exclude-kinds`, oldest first, waiting up to `--wait-s` seconds
(at most 60) for one; the `cursor` to read on from, which skips events the
filters dropped; and, with `--print-id`, `print_ended` — the print's end
whatever the filters say, or `null` while it is open. The
[API reference](./skills/printobserver/reference/api-and-clients.md#events)
states every value.

## Upgrading from 0.3.0 or earlier

Earlier versions generated the API credential into the state directory as
plaintext and wrote it, with the address, into `client.toml` beside it — where
every supervision turn could read it and act as the operator. Upgrading changes
four things.

- **The generated credential is converted on the first start.** When the state
  directory holds `api-credential` and no `api-credential.verifier`, the service
  writes the verifier, private to itself, and deletes `api-credential`. The same
  credential keeps working, so if you already hold it — in
  `PRINTOBSERVER_CREDENTIAL`, or in a `--config` file of your own — nothing
  changes for you.
- **`client.toml` is gone.** The service deletes it at every start and no longer
  writes it. Your commands read your own client configuration instead, which
  `printobserver credential issue` writes, or the two variables, or a file you
  name with `--config`.
- **A plaintext `api.credential` should become `api.credential_verifier`.** The
  service still starts and still admits it, but it logs a warning on every start,
  because a supervision turn runs as the service's user and can read the
  configuration file it is written in. Pipe the credential into
  `printobserver credential verifier`, put the line it prints in the
  configuration, delete `api.credential`, and restart the service. When both keys
  are there, the verifier wins and the plaintext is refused.
- **If your old credential is gone** — you only ever read it out of
  `client.toml` or `api-credential` — issue a new one as yourself with
  `printobserver credential issue --replace`, put the line it prints in the
  server's configuration in place of any earlier one, and restart the service.
  The old credential stops working.

A credential you choose yourself should be long and random: the service keeps
its SHA-256, and a verifier of a short or guessable credential can be searched
offline.

### Spool tracking on an existing installation

Nothing edits a configuration already installed, so one written before spool
tracking grants the operator none of the spool commands, and every one you ask
for is refused `actor_may_not_request`. Add `"register_spool"`,
`"load_spool"`, `"unload_spool"`, `"adjust_spool"` and `"retire_spool"` to the
`operator` row of `[safety.actions]`, and restart the service. Leave the agent's
row as it is.

### Closing a print record left open

A print that stopped while nothing was watching can leave its record open, and
every later preflight then warns about it. Close it with the outcome you judge
it ended with — `completed`, `cancelled`, `failed` or `interrupted`:

```console
printobserver close-print --print-id <id> --outcome interrupted --reason "the printer lost power mid-print"
```

It is refused while the printer shows that record's file printing, paused or
cancelling, and while an adjustment of it is still in force. A printer that is
off refuses nothing. A close charges no spool. A configuration written before
this command existed grants it to nobody, so add `"close_print"` to the
`operator` row of `[safety.actions]` and restart the service.

## Reference documentation

- [The agent skill](./skills/printobserver/SKILL.md), installed with
  `gh skill install nickderobertis/printobserver printobserver`
- [The command surface](./skills/printobserver/reference/command-surface.md)
- [Common operations](./skills/printobserver/reference/common-operations.md)
- [The intervention policy](./skills/printobserver/reference/intervention-policy.md)
- [The API and clients](./skills/printobserver/reference/api-and-clients.md)
- [The schemas](./skills/printobserver/reference/schemas.md)
- [The architecture](./skills/printobserver/reference/architecture.md)
- [Testing](./skills/printobserver/reference/testing.md)

## Development

```console
just bootstrap   # from a clean clone
just check       # the whole gate
just --list      # available recipes
```

<!-- llmlint: ignore[no_redundant_instruction_pointers] the README is the forge's landing page for a person, who is handed no instruction file by any harness -->
See [`AGENTS.md`](./AGENTS.md) for repository development and release rules.

## License

MIT.
