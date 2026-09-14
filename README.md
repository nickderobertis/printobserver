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

The server exposes an HTTP API. Every other `printobserver` subcommand, the
Rust, Python, and Node clients, and the agent use that same API and command
surface. The oneharness adapter in `crates/printobserver-oneharness` runs one
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

<!-- llmlint: ignore[contracts_have_one_source_or_a_drift_gate] This is the literal URL a person types into Obico's webhook plugin, which accepts only a URL, so the setup step cannot defer it elsewhere; it is an instruction to a person rather than a mirrored copy any program reads, and INGRESS_PATH in crates/printobserver-server/src/operations.rs and the token carriers in crates/printobserver-server/src/ingress.rs remain the one source. -->
In Obico's [notification settings](https://www.obico.io/docs/user-guides/notification-settings/),
set the webhook plugin's custom URL to:

```text
http://PRINTOBSERVER_HOST:8420/obico/webhook?token=YOUR_SHARED_SECRET
```

Use an address and port the Obico server can reach. Choose a long random
`YOUR_SHARED_SECRET` and put the identical value in `ingress.shared_secret` in
step 5. Because the plugin configures only a URL, the `token` query parameter is
how it carries the secret. A caller that can set headers may instead use
`x-printobserver-token`.

The default listen address is `127.0.0.1:8420`, which is reachable only on the
same machine. If Obico runs elsewhere, set `listen` to an address on the
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

### 5. Configure the supervisor

<!-- llmlint: ignore[contracts_have_one_source_or_a_drift_gate] A person filling in the configuration has to be told what each value is and where to get it; this explanation is not a second schema anything parses, the template in scripts/install-service.sh and the validation in crates/printobserver-server/src/config.rs remain the one source, and the server names any field it cannot accept at startup. -->
Edit `/etc/printobserver/config.toml`:

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
- `safety.agent_min_interval_s` is the minimum time between agent actions.
  Under `safety.allowed`, set the allowed range for feedrate factor, flowrate
  factor, fan percentage, bed target, and each tool target. Under
  `safety.actions`, grant action names separately to `operator`, `agent`, and
  `system`. Choose limits suitable for your printer and material. A print
  manifest may narrow them but cannot widen them.

The server validates these values at startup, including reaching OctoPrint and
authenticating its API key, and identifies a field it cannot accept.

### 6. Enable and start the service

```console
sudo systemctl enable --now printobserver.service
```

Starting is separate because this service commands a 3D printer. Installing
software must not start a process that can move the machine.

### 7. Verify the installation

```console
printobserver --version
```

<!-- llmlint: ignore[contracts_have_one_source_or_a_drift_gate] A first read against the running server is a walkthrough step and needs one concrete command for a person to type; the drift-gated source for it is the context example in docs/reference/common-operations.md, which checks_docs holds to surface.json, and the README restates only this single invocation rather than keeping a second command list. -->
Start a print through OctoPrint. Once you have its printobserver ID, make a
first read against the running supervisor:

```console
printobserver context --print-id PRINT_ID
```

The command surface requires the internal print ID for print-specific reads but
exposes no operation that lists print IDs. The tree does not provide an
end-user procedure for discovering that ID, so this part of the first-read
workflow remains unspecified by the implementation.

### 8. Attach the agent

There is no separate agent endpoint: when an event arrives, the server invokes
the harness selected by `supervisor.harness`. The installer runs printobserver
as a system user with no home directory, and its unit sets `ProtectHome=true`.
This repository does not provide a production procedure for installing the
selected harness or signing it in for that service user. Resolve that
harness-specific service setup before relying on supervision turns. Once the
harness is available, an Obico failure webhook causes a turn using the bundled
skill, and every requested action goes through the policy.

## Using it

```console
printobserver --help
```

The server address comes from the configuration file or
`PRINTOBSERVER_SERVER`. Read a print's context before intervening, and give
every change a `--reason`. See
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
