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

`printobserver server` is the supervisor. On an installed machine it runs as a
service under that machine's own service manager — a systemd unit on Linux, a
launchd daemon on macOS, a Windows service on Windows — started at boot and
started again after a crash. It reads printer and job state through
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

Choose one of these three routes, on whichever operating system you are on.
Each installs a program already built for your platform and needs no Rust
toolchain on the printer host. The routes are stated once, in
[`AGENTS.md`'s install path](./AGENTS.md#the-end-user-install-path), and a
check holds the commands here to the ones stated there.

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

On Windows, the same installer in its PowerShell form, from any PowerShell:

```powershell
irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1 | iex
```

It takes the same two options:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.ps1))) -Version v0.1.0 -To C:\Tools\printobserver
```

Whichever route you took, check the program. It prints the version it is,
which is the one thing the install commands cannot tell you:

```console
printobserver --version
```

### 4. Install the service files

This puts four things in place — the program, a private state directory, a
configuration template, and the service definition your platform's service
manager reads — and deliberately starts nothing.

On Linux and macOS, the installer writes a systemd unit or a launchd property
list, whichever the host runs services under:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.sh | sudo sh
```

On Windows, from an elevated PowerShell, the same installer in its Windows form
registers a service with the service control manager — to start on demand, as
its own virtual account, and to be brought back if its process dies:

```powershell
irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.ps1 | iex
```

Where the installer puts things differs by platform. The rest of this guide
names the three places by role — the configuration file, the state directory,
the program — and this table is what each role means on your machine:

| Platform | Configuration file | State directory | Program |
| --- | --- | --- | --- |
| Linux | `/etc/printobserver/config.toml` | `/var/lib/printobserver` | `/usr/local/lib/printobserver/printobserver` |
| macOS | `/etc/printobserver/config.toml` | `/var/lib/printobserver` | `/usr/local/lib/printobserver/printobserver` |
| Windows | `C:\ProgramData\printobserver\config.toml` | `C:\ProgramData\printobserver\state` | `C:\Program Files\printobserver\printobserver.exe` |

These are the program's own answers, from `crates/printobserver/src/locations.rs`;
each installer writes exactly them, and a test holds this table to that module.
The configuration file and the state directory are private: on Linux and macOS
they are readable by root and by the service's own user, and on Windows by
administrators and by the service's account.

### 5. Configure the supervisor

Edit the configuration file — as root on Linux and macOS, from an elevated
PowerShell on Windows:

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

The server validates these values at startup, including reaching OctoPrint and
authenticating its API key, and identifies a field it cannot accept.

#### Issue your operator credential

The service holds no credential you can authenticate with — only its verifier.
Issue yourself one as yourself, unprivileged. On Linux and macOS:

```console
printobserver credential issue
```

On Windows, from a PowerShell that is not elevated:

```powershell
& 'C:\Program Files\printobserver\printobserver.exe' credential issue
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
(or as `credential_verifier = …` inside an `[api]` table), and the service reads
it when it next starts. If you change `listen`, run it again with `--replace`
and `PRINTOBSERVER_SERVER` set to the new address, and put the new line in
place of the old one. To use a credential you chose instead, pipe it into
`printobserver credential verifier` for the same line; make it long and random,
because a verifier of a short or guessable credential can be searched offline.

From then on every command you run reads your own configuration first. With no
`--config`, a command reads `printobserver/client.toml` under your configuration
home, then the service's own configuration file for whatever that left unnamed
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

When the service and you run as the same OS user, a supervision turn runs as you
too: it can read your own client configuration, and the environment of another
turn running at the same time — through `/proc/<pid>/environ` on Linux, or
`ps -E` on macOS. Full isolation needs the service to run as an OS user of its
own. The installer gives it one when it runs as root, as the steps above have
it: on Linux and macOS it creates the `printobserver` system user (or runs the
service as the user `--user` names), and on Windows the service runs as its own
virtual account, `NT SERVICE\printobserver`. Issue your credential as your own
user, not as the service's.

### 6. Install the agent's skill

The agent's skill is not part of the program: it is the Agent Skill this
repository publishes, and the service reads it from `supervisor.skill_path`.
Install it under the state directory, where the service can read it, as the
host's administrator — as root on Linux and macOS, from an elevated PowerShell
on Windows. It needs GitHub CLI 2.100.0 or later, the first release with
`gh skill`, and no GitHub sign-in: the repository is public. The installer's
comment above `skill_path` names the same command.

On Linux and macOS:

```console
sudo gh skill install nickderobertis/printobserver printobserver --dir /var/lib/printobserver/skills
```

On Windows:

```powershell
gh skill install nickderobertis/printobserver printobserver --dir C:\ProgramData\printobserver\state\skills
```

Run it again to take a newer skill; restart the service afterwards, because the
skill is read when the service starts.

### 7. Install and sign in the agent's harness

There is no separate agent endpoint: when an event arrives, the server runs the
harness `supervisor.harness` selects, as the service's own account. That
account has no home directory of its own to keep a sign-in in, so the harness
keeps it under the state directory instead, in `harness/<harness>` there.
printobserver can sign in `claude-code` and `codex`.

Install the harness program system-wide, so that the service account's path
finds it. On Linux and macOS:

```console
sudo npm install -g @anthropic-ai/claude-code
```

For `codex`, install its program instead:

```console
sudo npm install -g @openai/codex
```

Then sign it in once, as the service user:

```console
sudo -u printobserver /usr/local/lib/printobserver/printobserver sign-in
```

On Windows, install the harness with the same `npm install -g` command, without
`sudo`, from an elevated PowerShell. The service's virtual account cannot be
signed in to and does not need to be — the sign-in lands under the state
directory, which the installer made the service account's to read — so sign in
from that same elevated PowerShell:

```powershell
& 'C:\Program Files\printobserver\printobserver.exe' sign-in
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

### 8. Enable and start the service

One command, and which one is your platform's service manager's. On Linux:

```console
sudo systemctl enable --now printobserver.service
```

On macOS:

```console
sudo launchctl bootstrap system /Library/LaunchDaemons/io.github.nickderobertis.printobserver.plist
```

On Windows, from an elevated PowerShell, the one command that sets the service
to start automatically and starts it:

```powershell
Set-Service -Name printobserver -StartupType Automatic -Status Running
```

On every platform the service manager starts it again at every boot, and
again if it ends abruptly.

Starting is separate because this service commands a 3D printer. Installing
software must not start a process that can move the machine — at install, or at
the next reboot.

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
