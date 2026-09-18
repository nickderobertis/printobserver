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

`printobserver server` is the supervisor. On an installed machine it runs as
the `printobserver.service` systemd unit. It reads printer and job state through
OctoPrint's API using the configured address and API key. It also exposes an
ingress for alerts posted by Obico's webhook notification plugin; when an alert
arrives, it immediately fetches the snapshot named by that alert.

The server exposes an HTTP API that serves only requests carrying its API
credential. Every other `printobserver` subcommand, the Rust, Python, and Node
clients, and the agent use that same API and command surface. The oneharness adapter in `crates/printobserver-oneharness` runs one
supervision turn for each event on the harness identity configured under
`[supervisor]`. It supplies
[`printobserver-skill.md`](./crates/printobserver-oneharness/assets/printobserver-skill.md)
as the turn's system prompt, and the agent acts through the same commands an
operator uses.

Every action request is recorded with its actor and reason, together with the
policy decision and, when accepted, its outcome. The history lets a person
audit what the agent requested, why, and what happened. See
[the architecture](./docs/reference/architecture.md),
[the intervention policy](./docs/reference/intervention-policy.md), and
[the API and clients](./docs/reference/api-and-clients.md) for details.

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

Choose one alternative. Each installs a prebuilt program and needs no Rust
toolchain on the printer host.

With Python:

```console
pip install printobserver-cli
```

With Node:

```console
npm install -g printobserver-cli
```

Or with the bundled installer:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh
```

It also accepts a release and destination:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh -s -- --version v0.1.0 --to ~/.local/bin
```

Check the program:

```console
printobserver --version
```

### 4. Install the service files

This installs the program, private state directory, configuration template,
and systemd unit. It deliberately starts nothing.

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.sh | sudo sh
```

On Windows, from an elevated PowerShell, the same installer in its Windows form:
the program under `C:\Program Files\printobserver`, the configuration and the
private state directory under `C:\ProgramData\printobserver`, and a Windows
service registered with the service control manager — to start on demand, as
its own virtual account, and to be brought back if its process dies. It, too,
starts nothing.

```powershell
irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.ps1 | iex
```

### 5. Configure the supervisor

Edit `/etc/printobserver/config.toml` (`C:\ProgramData\printobserver\config.toml` on
Windows):

- `state_dir` holds the database, images, sessions, and installed agent assets;
  the template uses `/var/lib/printobserver`.
- `listen` serves both the HTTP API and Obico ingress. The template uses
  `127.0.0.1:8420`; change it if Obico is on another machine.
- `octoprint.url` is the OctoPrint base URL from step 1.
- `octoprint.api_key` is the API key generated in step 1.
- `octoprint.fan` is `commandable` when printobserver may command the
  part-cooling fan, or `absent` when the printer has none.
- `supervisor.harness` selects the oneharness identity for supervision turns;
  the template selects `claude-code`.
- `ingress.shared_secret` is the private random value used in step 2. Anyone
  who has it can submit an alert that may lead to a printer action.
- `api.credential` is optional, and the template leaves it out. Every request to
  the HTTP API must carry the API credential, and the server refuses any request
  that does not. Left out, the service generates a random credential the first
  time it starts, into `/var/lib/printobserver/api-credential`, and reuses it on
  every later start. To choose it yourself, add an `[api]` table with
  `credential` set to a long random value; the generated file is then not used.
  This credential is separate from `ingress.shared_secret`, and neither is
  accepted in place of the other.
- `safety.agent_min_interval_s` is the minimum time between agent actions.
  Under `safety.allowed`, set the allowed range for feedrate factor, flowrate
  factor, fan percentage, bed target, and each tool target. Under
  `safety.actions`, grant action names separately to `operator`, `agent`, and
  `system`. Choose limits suitable for your printer and material. A print
  manifest may narrow them but cannot widen them.

The server validates these values at startup, including reaching OctoPrint and
authenticating its API key, and identifies a field it cannot accept.

Each time it starts, the service writes `/var/lib/printobserver/client.toml`: a
`[client]` table with `server`, the address it is listening on, and
`credential`, the API credential in force. Its supervision turns read that file.
You can too, as root: pass `--config /var/lib/printobserver/client.toml` to any
command. `client.toml` and `api-credential` are readable only by the service's
user.

From your own user account you cannot read `/etc/printobserver/config.toml` or
anything under `/var/lib/printobserver`. Read the credential once as root, then
supply it with the address. Either set both environment variables:

```console
export PRINTOBSERVER_SERVER=http://127.0.0.1:8420
export PRINTOBSERVER_CREDENTIAL="$(sudo cat /var/lib/printobserver/api-credential)"
```

or put both in a `[client]` table in a file only you can read, and pass that
file with `--config`:

```toml
[client]
server = "http://127.0.0.1:8420"
credential = "the credential you read as root"
```

If you set `api.credential`, use that value instead of the generated file. When
both variables are set and you pass no `--config`, `printobserver` does not read
the service's configuration file at all. A command the server refuses exits with
status 4 and says where the credential is read from.

### 6. Install and sign in the agent's harness

There is no separate agent endpoint: when an event arrives, the server runs the
harness `supervisor.harness` selects, as the service's own user. That user has
no home under `/home` and the unit hides every home directory, so the harness
keeps its sign-in under the state directory instead, in
`/var/lib/printobserver/harness/<harness>`. printobserver can sign in
`claude-code` and `codex`.

Install the harness program where the service user's path finds it, which a
system-wide install does:

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

On Windows the service's virtual account cannot be signed in to and does not
need to be — the sign-in lands under the state directory, which the installer
made the service account's to read — so install the harness with
`npm install -g` and sign in from the same elevated PowerShell:

```powershell
& 'C:\Program Files\printobserver\printobserver.exe' sign-in
```

This reads `state_dir` and `supervisor.harness` from
`/etc/printobserver/config.toml` and nothing else, so it works before or after
you fill in the OctoPrint and Obico values. It creates the harness's directory,
readable by the service user alone, and runs that harness's own interactive
sign-in in your terminal: `claude auth login`, or `codex login --device-auth`.
Follow its prompts. The command exits with the harness's own status. It starts
no service and contacts neither OctoPrint nor Obico.

Every supervision turn the service runs is pointed at the same directory, so
it uses that sign-in: an Obico failure webhook causes a turn using the bundled
skill, and every action the agent requests goes through the policy. Run the
sign-in again if the harness's sign-in expires, or after changing
`supervisor.harness`.

### 7. Enable and start the service

```console
sudo systemctl enable --now printobserver.service
```

On Windows, the one command that sets the service to start automatically and
starts it:

```powershell
Set-Service -Name printobserver -StartupType Automatic -Status Running
```

Starting is separate because this service commands a 3D printer. Installing
software must not start a process that can move the machine — at install, or at
the next reboot.

### 8. Verify the installation

```console
printobserver --version
```

Start a print through OctoPrint. Then ask the running supervisor which prints it
holds. Every command reads the address and the API credential from the two
environment variables or the `--config` file described in step 5:

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

Make the first read with that ID in place of `PRINT_ID`:

```console
printobserver context --print-id PRINT_ID
```

## Using it

```console
printobserver --help
```

Every command reads the server's address and the API credential from a
configuration file named with `--config`, or from `PRINTOBSERVER_SERVER` and
`PRINTOBSERVER_CREDENTIAL`; see step 5.

Every print-specific command takes the print's ID as `--print-id`. Find it
first: `printobserver prints` lists every print, newest first, and names the one
OctoPrint is running as `active`. Then read that print's context before
intervening:

```console
printobserver prints
printobserver context --print-id PRINT_ID
```

Give every change a `--reason`. See
[common operations](./docs/reference/common-operations.md) for a worked example
of every command, including a temporary adjustment with `--duration-s`, and
its output.

## Reference documentation

- [The agent skill](./crates/printobserver-oneharness/assets/printobserver-skill.md)
- [The command surface](./docs/reference/command-surface.md)
- [Common operations](./docs/reference/common-operations.md)
- [The intervention policy](./docs/reference/intervention-policy.md)
- [The API and clients](./docs/reference/api-and-clients.md)
- [The schemas](./docs/reference/schemas.md)
- [The architecture](./docs/reference/architecture.md)
- [Testing](./docs/reference/testing.md)

## Development

```console
just bootstrap   # from a clean clone
just check       # the whole gate
just --list      # available recipes
```

See [`AGENTS.md`](./AGENTS.md) for repository development and release rules.

## License

MIT.
