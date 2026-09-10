# printobserver

A thin, provably-correct supervision layer between a 3D printer and an agent
that watches it. One installable artifact — the `printobserver` command —
carries everything: `printobserver server` runs the supervisor, and the other
subcommands are the operator's surface onto a running one.

## Install

Three **alternative** routes put the `printobserver` program on your path. Take
one of them, not all three — whichever suits the machine in front of you. Every
one of them installs a program already built for your platform, so none needs a
Rust toolchain on the target host.

From the Python package registry:

```console
pip install printobserver-cli
```

From the JavaScript package registry:

```console
npm install -g printobserver-cli
```

Or with the bundled install script, which needs neither package manager:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh
```

It defaults to the newest release and a default directory, and also accepts a
pinned release and an install directory:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh -s -- --version v0.1.0 --to ~/.local/bin
```

Then, in order, two commands. The first puts the binary, the state directory,
the configuration and the systemd unit in place:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.sh | sudo sh
```

The second enables and starts the service:

```console
sudo systemctl enable --now printobserver.service
```

Between the two, edit `/etc/printobserver/config.toml`: the installer writes a
template, and the OctoPrint address and key and the shared secret the Obico
ingress requires are yours to fill in. Every value is validated when the service
starts, and one that cannot work is refused naming the field it is about.

Enabling and starting is a command of its own rather than something the
installer does, **because this service commands a 3D printer**: installing a
package must not, as a side effect, start a process that can move a machine.

`AGENTS.md`'s "The end-user install path" is the authoritative source of all
five commands above; this section is derived from it and a check holds the two
together.

## Using it

Everything the supervising agent does and everything an operator does goes
through the one command, against a running server:

```console
printobserver --help
```

That surface is **closed and derived**: one command per action the contracts
declare plus the reads the server serves, each taking exactly the values that
operation's own request schema names. Nothing in the command-line program lists
them, so nothing there can grow them.

Four options are common to every command: `--json` for machine-readable output,
`--config <path>`, `--help` and `--version`. Where the server is and what
authenticates to it are **configuration** rather than arguments — read from that
file, which may be the server's own `/etc/printobserver/config.toml`, and from
`PRINTOBSERVER_SERVER` and `PRINTOBSERVER_CREDENTIAL`.

Every mutating command takes a `--reason`, which is what makes the history worth
reading; every adjustment may take a `--duration-s`, which makes it a bounded
intervention that puts the prior value back when it expires. An image is an
absolute path on the server's own host: this program transports no image bytes,
and a path that names no file where it is running is a failure of its own rather
than a file name that names nothing.

## Reference documentation

The supervising agent's own skill is
[`crates/printobserver-oneharness/assets/printobserver-skill.md`](./crates/printobserver-oneharness/assets/printobserver-skill.md)
— deliberately short, because it is the system prompt of every supervision turn.
Everything it does not say is in these, and an operator reads the same ones:

- [The command surface](./docs/reference/command-surface.md) — every command,
  its arguments, its output and how it fails.
- [Common operations](./docs/reference/common-operations.md) — a worked example
  of each, every one of them run against a real server by a committed check.
- [The intervention policy](./docs/reference/intervention-policy.md) — where the
  bounds come from, what a rejection carries, and what expiry does.
- [The API and the clients](./docs/reference/api-and-clients.md) — the same
  surface over HTTP.
- [The schemas](./docs/reference/schemas.md) — generated from the types.
- [The architecture](./docs/reference/architecture.md) — what each crate owns,
  and the two structural rules the design rests on.
- [Testing](./docs/reference/testing.md) — each tier, what it proves, and which
  ones do not run on every change.

## Development

```console
just bootstrap   # from a clean clone
just check       # the whole gate
just --list      # the command surface
```

See [`AGENTS.md`](./AGENTS.md) for how this repository is built, governed and
released.

## License

MIT.
