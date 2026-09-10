# The API and the clients

The HTTP surface `printobserver server` serves, and the clients this repository
publishes for it. This document covers the one versioned prefix, how a request is
authenticated, the operations — one entry each — and the clients, one entry per
method each of them exports.

## The one versioned prefix

Every public operation is served beneath `/v1`, takes `application/json` where it
takes a body at all, and answers `application/json`.

One endpoint sits outside it: `/obico/webhook`, where Obico's own webhook
notification plugin posts. That is the producer's ingress rather than an
operation a client calls — it is authenticated by a shared secret rather than by
an actor, it carries Obico's body rather than this system's, and it answers
before its handling completes so that the producer's short posting timeout is
never the thing that loses an alert.

## How a request is authenticated

Every request to a versioned operation carries the credential the server was
configured with. A client reads that credential and the server's address from a
configuration file or from `PRINTOBSERVER_SERVER` and `PRINTOBSERVER_CREDENTIAL`;
neither is ever a request parameter. Who is *asking* is a different thing from
who is *authenticated*: every mutating operation carries an actor in its body,
and the safety envelope grants actions per actor class.

A mutating operation answers `200` when it was carried out and `409` when the
policy refused it. A rejection is not a transport failure — the body is the same
action record, carrying the rejection's own reason, the value asked for and the
range allowed.

## The operations

### status

What the print and the machine are doing right now.

`GET /v1/prints/{print_id}/status` — answers success.

Takes `print_id` (path).

### context

Everything one supervision turn is given about a print, in one read, with the latest image already materialized to a path.

`GET /v1/prints/{print_id}/context` — answers success.

Takes `print_id` (path).

Its answer carries `image_path`, an absolute path on the server's
own filesystem. No route of this server answers image bytes.

### image

One image record and the absolute path its bytes are at.

`GET /v1/images/{image_id}` — answers success.

Takes `image_id` (path).

Its answer carries `path`, an absolute path on the server's
own filesystem. No route of this server answers image bytes.

### history

The print's events, newest first.

`GET /v1/prints/{print_id}/history` — answers success.

Takes `print_id` (path), `limit` (query, optional).

### manifest_get

The manifest a print is running under, and every range it asked wider than the envelope allows.

`GET /v1/prints/{print_id}/manifest` — answers success.

Takes `print_id` (path).

### manifest_set

Replace that manifest. It carries a reason, because a manifest narrows what any actor may ask for.

`PUT /v1/prints/{print_id}/manifest` — answers success.

Takes `print_id` (path), `reason` (body), `manifest` (body).

### pause

Pause the print.

`POST /v1/prints/{print_id}/actions/pause` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `reason` (body).

### resume

Resume the print.

`POST /v1/prints/{print_id}/actions/resume` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `reason` (body).

### cancel

Cancel the print.

`POST /v1/prints/{print_id}/actions/cancel` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `reason` (body).

### start_print

Start a print of a named file, bounded by a manifest.

`POST /v1/prints/{print_id}/actions/start_print` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `file_name` (body), `manifest` (body), `reason` (body).

### set_feedrate_factor

Set the feedrate multiplier.

`POST /v1/prints/{print_id}/actions/set_feedrate_factor` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `duration_s` (body, optional), `factor` (body), `reason` (body).

### set_flowrate_factor

Set the flowrate multiplier.

`POST /v1/prints/{print_id}/actions/set_flowrate_factor` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `duration_s` (body, optional), `factor` (body), `reason` (body).

### set_tool_target_c

Set one tool's target temperature.

`POST /v1/prints/{print_id}/actions/set_tool_target_c` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `duration_s` (body, optional), `reason` (body), `target_c` (body), `tool` (body).

### set_bed_target_c

Set the bed's target temperature.

`POST /v1/prints/{print_id}/actions/set_bed_target_c` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `duration_s` (body, optional), `reason` (body), `target_c` (body).

### set_fan_percent

Set the fan percentage.

`POST /v1/prints/{print_id}/actions/set_fan_percent` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `duration_s` (body, optional), `percent` (body), `reason` (body).

### acknowledge_failure

Record what was made of a failure event and what should happen next.

`POST /v1/prints/{print_id}/actions/acknowledge_failure` — answers success, rejected.

Takes `print_id` (path), `actor` (body), `disposition` (body), `event_id` (body), `reason` (body).

## The clients

One entry per client this repository declares, and one entry per method that
client exports. The set is read off `repo-policy.toml`'s `crates.clients`, so a
client added or removed moves what this section owes with it.

### printobserver-sdk

The Rust client of the surface above: a typed, async client and its request and
answer types. It depends on the contracts and an HTTP client and on nothing else
— a client that could reach the core or the server would be a second copy of the
server.

It exports no method yet. The crate is published and the toolchain around it is
in place; the methods land with the `sdks` node, and this section grows one entry
per method the moment they do, because the check behind this document reads the
client's own exports rather than a list written here.

This repository also carries Python and Node client packages, at
`python/printobserver-sdk` and `npm/printobserver-sdk`. Neither exports anything
yet and neither is in `repo-policy.toml`'s declared client list, so neither owes
an entry here; both join that list when the `sdks` node writes them, and this
section grows an entry for each of them when they do.

The `printobserver-cli` distributions on those two registries are a different
thing again: they carry the command-line program rather than a client of this
API. `AGENTS.md`'s "The end-user install path" is where they are declared.
