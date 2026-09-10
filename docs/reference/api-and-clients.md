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

The Rust client is typed and blocking. Its request and response types and its
operation methods are generated from the committed schemas. It carries its own
HTTP transport and depends on neither the core nor the server. Construct a
`Client` with an address and an `Actor`; the actor accompanies every action.

Every generated mutating method requires a reason and refuses an empty or
whitespace-only reason before sending a request. Adjustment methods also take
`duration_s: Option<i64>`. A policy refusal returns `ClientError::Rejected`
carrying a `Rejection` with `reason`, `requested`, `allowed`, and the complete
action answer. Image answers preserve absolute paths on the server's host.

The Python distribution `printobserver-sdk` and npm distribution
`@printobserver/sdk`, implemented at `python/printobserver-sdk` and
`npm/printobserver-sdk`, expose the same generated operation names and response
shapes. Their package metadata records the server contract version, as does
Rust's `CONTRACT_VERSION`. The `printobserver-cli` registry distributions carry
the command-line program; they are separate from these client libraries.

#### new

`Client::new(address, actor)` constructs a client without making a request.
The address accepts `http://host:port` or `host:port`.

#### with_credential

`client.with_credential(credential)` returns the client configured to send the
credential as a bearer token.

#### address

`client.address()` reads the configured host and port.

#### actor

`client.actor()` reads the actor used by generated action methods.

#### status

`client.status(print_id)` calls the status operation and returns `StatusAnswer`.

#### context

`client.context(print_id)` returns `ContextAnswer`, including the latest image's
materialized path on the server's host.

#### image

`client.image(image_id)` returns `ImageAnswer` with the image record and path.

#### history

`client.history(print_id, limit)` returns `HistoryAnswer`; `limit` is optional.

#### manifest_get

`client.manifest_get(print_id)` returns the print's `ManifestAnswer`.

#### manifest_set

`client.manifest_set(print_id, reason, manifest)` writes the manifest and returns
`ManifestAnswer`.

#### pause

`client.pause(print_id, reason)` pauses the print and returns `ActionAnswer`.

#### resume

`client.resume(print_id, reason)` resumes the print and returns `ActionAnswer`.

#### cancel

`client.cancel(print_id, reason)` cancels the print and returns `ActionAnswer`.

#### start_print

`client.start_print(print_id, file_name, manifest, reason)` starts the named file
under its manifest and returns `ActionAnswer`.

#### set_feedrate_factor

`client.set_feedrate_factor(print_id, factor, reason, duration_s)` requests a
feedrate multiplier and returns `ActionAnswer`.

#### set_flowrate_factor

`client.set_flowrate_factor(print_id, factor, reason, duration_s)` requests a
flowrate multiplier and returns `ActionAnswer`.

#### set_tool_target_c

`client.set_tool_target_c(print_id, reason, target_c, tool, duration_s)` requests
a tool temperature and returns `ActionAnswer`.

#### set_bed_target_c

`client.set_bed_target_c(print_id, reason, target_c, duration_s)` requests a bed
temperature and returns `ActionAnswer`.

#### set_fan_percent

`client.set_fan_percent(print_id, percent, reason, duration_s)` requests a fan
percentage and returns `ActionAnswer`.

#### acknowledge_failure

`client.acknowledge_failure(print_id, disposition, event_id, reason)` records the
failure acknowledgement and returns `ActionAnswer`.

#### call

`client.call(method, path, query, body)` is the generic transport entry used by
generated methods. It deserializes a successful answer into the caller's chosen
type and maps policy and printer refusals into `ClientError`. Applications use
the generated methods above for their typed arguments and reason validation.

#### of

`Rejection::of(answer)` extracts a typed rejection from an `ActionAnswer`, or
returns `None` when its decision is not a rejection.

#### reason_given

`reason_given(reason)` validates a reason locally, returning
`ClientError::NoReason` for an empty or whitespace-only value.

#### as_value

`as_value(value)` serializes a request value to JSON, returning
`ClientError::Unsendable` if serialization fails.

#### send

The private transport module's `send` function writes one HTTP request and
reads its status and body, rejecting an incomplete response. It is an internal
helper rather than a crate-root export; callers use `Client`.
