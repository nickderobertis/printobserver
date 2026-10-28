<!-- llmlint: ignore[instruction_layer_localized] Reviews of this subtree are routed: `.github/CODEOWNERS` assigns every path, this one included, with `* @nickderobertis`. A run over a diff is handed only the files that changed and that file is not one of them, so a judge of this diff can read this crate's file and not the routing it asks for. -->
# printobserver

What this crate's own suite needs from the host that runs it, where that is not
what every other crate needs.

## The service manager's own journey is not in this suite

`tests/service.rs` proves what the committed installer writes and that the
definition's own start command starts a server that answers. Whether the service
manager itself starts the service, reports it as one it starts at boot, and
brings it back after it is killed is
`tests/repo-e2e/tests/test_service_manager_journey.py`'s: one walk, in the
`test-e2e` tier, with one adapter per service manager — systemd as the first
process of a throwaway Docker container, launchd on the host through a
password-free `sudo` on macOS and against a `launchctl` stand-in on Linux, and
the Windows service through `sc.exe`. Change the lifecycle there; this suite
carries no copy of it and needs no Docker.

## Where a client reads its server and credential from

`config::load` reads the operator's own `client.toml` under their configuration
home (`locations::operator_client_config`), then the per-user server
configuration when there is one (`locations::per_user_config`, Windows alone
for now), then the service's default file — a client with no `[client]` table
takes the address the server listens on. With `--config`, that one file alone.
`sign-in` with no `--config` reads `locations::default_config`: the per-user
file when it is there, the service's otherwise. `tests/per_user.rs` drives both
through the binary. `PRINTOBSERVER_SERVER` and
`PRINTOBSERVER_CREDENTIAL` win over any file, and with both set and no
`--config` no file is opened at all, which is how a supervision turn runs:
`tests/service.rs` traces the open calls to hold that order.

## `watch` wakes only on the supervisor's own writes

A waiting `events` read is woken by the store's signal, raised by writes the
running supervisor makes. A row written through a second connection — how
`tests/journeys/streaming.rs` records observations with chosen `received_at`
values — raises none, so those journeys follow each such write with a `look`
the supervisor records itself (`support/standalone.rs`'s `nudge`); without it a
read finds the rows only at its deadline.
