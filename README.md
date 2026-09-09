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
