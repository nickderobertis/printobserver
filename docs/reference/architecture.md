# The architecture

What each crate of this workspace owns, the edge table the dependency rule is,
and the two structural rules the whole design rests on. This document covers the
crates — one entry each — why core names no implementation crate, and why the
agent reaches this system the way an operator does.

Everything asserted here is asserted against the tree rather than as prose: the
crates named below are the ones the workspace declares, and the dependency
direction stated is the one the manifests carry. A check reads both beside this
document and refuses one that names a crate the workspace does not have, omits
one it does, or asserts an edge the manifests contradict.

## The crates

Twelve crates, cut by domain rather than by layer: one contract crate every
other crate agrees on, three ports each with its own vocabulary, the
supervision domain with its records and the persistence interfaces it needs,
three adapters and one store behind those, and the composition roots and
clients that choose what runs and talk to it. Each crate owns the concepts of
its domain, so that a change to one provider's wire format or event payload
edits that provider's crate and rebuilds it and the composition roots, and
nothing else.

### printobserver-types

The cross-domain contract, and only that: what every domain and every client
must agree on, and nothing a domain adding a concept has to edit. The identity
rule — the exported `identifier!` macro, its error, the UUID-v7 rule, and the
three identifiers the envelope reaches (`PrintId`, `EventId`, `ImageId`); the
representation rules (`Timestamp`, `RawBytes`, `FileName`, `Reported` and
`Range`); the event log's envelope — an open `EventKind` name, an `EventSource`
name, an opaque payload, and the `ImageRef` handle an image travels under, with
no kind and no source declared here; and the schema toolkit. Data and total
functions over data, never I/O. It is the root of the graph and depends on no
crate of this workspace.

### printobserver-printer-api

The port the physical printer speaks through: reading printer and job state, and
asking the machine for one of the bounded things the action vocabulary names —
together with the printer domain's vocabulary: the state a printer reports, the
closed set of adjustables its setters change, the snapshots those reads answer
and the plausibility ranges their reported numbers are read against. Depends on
the contracts alone.

### printobserver-vision-api

The port external observations arrive through: normalizing a received body into
an event under the adapter's own kind, beside the provider-neutral print
correlation the supervision core reads, and retrieving the image that body
names. It declares the one kind of its own — a body no adapter could read,
written down. Depends on the contracts alone.

### printobserver-supervisor-api

The port the supervising agent is reached through: running one supervision turn,
and closing a session; the assessment vocabulary a turn answers with, whose
generated schema is the one artifact the agent's answer is constrained by; the
session a turn opens or continues; and the two kinds a session's opening and
closing are written down under. Depends on the contracts alone.

### printobserver-octoprint

The OctoPrint adapter — the one implementation of the printer port. It is the
only crate in this workspace that may construct an OctoPrint request: everything
above it is written as though printers were normal, so that one vendor's own
surface has exactly one place it has to be kept right.

### printobserver-obico

The Obico adapter — the one implementation of the vision port. It declares the
nine wire shapes Obico's webhook notification plugin posts, beside the sample
bodies that model them, reads a posted body into an event under one of the two
kinds it declares for itself, fetches the snapshot that body names, and writes
both down through the ingress — which writes through the supervision domain's
own store traits, the print, event and image stores and no other. The
supervision core never reads its kinds by name; it correlates on the print the
adapter hands over beside the body.

### printobserver-oneharness

The oneharness adapter — the one implementation of the supervisor port. It
drives an agentic coding harness through OneHarness's own Rust crate API, in
process, and it spawns nothing. The skill it sends as the system prompt, the
template it fills for one turn, and the reference documents that skill links to
are assets it ships — the composition root writes them into the state directory
beside one another, because an installed host has no checkout to read them from.

### printobserver-store-sqlite

The SQLite store — the one durable implementation of the store traits the
supervision domain declares, with its schema, its migrations and the
content-addressed image files beside the database; and, beside it, the
in-memory implementation of the same traits that one conformance suite holds
both to. It depends on the domain whose traits it implements, never the other
way round.

### printobserver-core

The supervision domain. Its logic: the loop that turns printer state and vision
observations into supervisory decisions, the policy that decides which of them
may reach the printer, the effective bounds a print runs under, and the expiry
of bounded interventions. Its records: the print (whose external correlation is
`provider_print_id`, the provider's own identifier for it, whichever provider
reported it), the action an actor asks for and the record of it, the bounded
intervention, the manifest, the safety envelope and the decision policy takes,
the image stored beside an event, and the two identifiers it mints, `ActionId`
and `InterventionId`, through the contract crate's exported rule. The print's
context, the one aggregate a caller and the agent both read. The seven event
kinds and three source names that logic writes. And the persistence interfaces
it needs, one trait per aggregate it persists — prints with their manifests and
narrowings, the event log, images, actions with the interventions they open,
sessions — so that a consumer names only the aggregates it touches and a column
one aggregate gains is a change nothing else rebuilds against.

### printobserver-server

The long-running service: the HTTP surface clients call, the Obico ingress the
failure detector posts to, the supervision loop's lifecycle, and the composition
root that chooses which implementation backs each port. It declares the public
operations once, and the router is folded over that declaration; and it declares
the one kind a restart writes, the startup reconciliation its `reconcile` module
alone records.

### printobserver-sdk

The Rust client of the server's HTTP surface, generated from the schema set
under `schemas/<crate>/` beside the Python and Node clients. It carries the
event envelope open — a kind name and an opaque payload — with a generated
table from kind name to payload type and a typed accessor over it. It depends
on an HTTP client and on no crate of this workspace: what it agrees with the
server on is the schema set, not a type.

### printobserver

The `printobserver` command — the single installable artifact. Its `server`
subcommand runs the supervisor; every other subcommand is one request to an
already-running one. It builds its whole command surface by folding over what
the server declares, so it holds no list of commands or options of its own.

## Why core names no implementation crate

The dependency rule is an edge table rather than a set of layers:
`repo-policy.toml`'s `crates.may_depend_on` names, for every crate of the
workspace, the crates it may depend on across every dependency table, and
`just check-repo` refuses a manifest edge the row does not admit, a row naming
a crate the workspace lacks, and a crate the table omits.

| Crate | May depend on |
| --- | --- |
| `printobserver-types` | — |
| `printobserver-printer-api` | `printobserver-types` |
| `printobserver-vision-api` | `printobserver-types` |
| `printobserver-supervisor-api` | `printobserver-types` |
| `printobserver-core` | `printobserver-types`, `printobserver-printer-api`, `printobserver-vision-api`, `printobserver-supervisor-api` |
| `printobserver-octoprint` | `printobserver-printer-api`, `printobserver-types` |
| `printobserver-obico` | `printobserver-vision-api`, `printobserver-core`, `printobserver-types` |
| `printobserver-oneharness` | `printobserver-supervisor-api`, `printobserver-types` |
| `printobserver-store-sqlite` | `printobserver-core`, `printobserver-supervisor-api`, `printobserver-printer-api`, `printobserver-types` |
| `printobserver-server` | every crate above |
| `printobserver-sdk` | — |
| `printobserver` | `printobserver-server`, `printobserver-sdk`, `printobserver-types` |

Stated against the manifests rather than as prose: `printobserver-core` depends
on `printobserver-printer-api`, `printobserver-supervisor-api`,
`printobserver-types` and `printobserver-vision-api`, and on no adapter and no
store. A check reads that sentence beside the workspace's own manifests and
refuses one they contradict.

Three consequences the table is drawn for. What rebuilds when one provider
changes: a change to one provider's wire format or event payload edits that
provider's crate and rebuilds it and the composition roots — the only crates
that name `printobserver-octoprint`, `printobserver-obico`,
`printobserver-oneharness` or `printobserver-store-sqlite` are
`printobserver-server` and, through it, `printobserver`. What sits under
everything: `printobserver-types` is what every crate agrees on, and nothing in
it names a domain — no enum with a variant per provider, no list of kinds, no
list of sources — so a domain that adds an event kind, an aggregate or a source
name edits its own crate and regenerates the clients. And what a reader of the
log may assume about a kind it does not know: nothing beyond the envelope. The
log is open; the store, the server, a client and a test carry a record of an
unknown kind through, filter by its name, or read a typed payload out of it and
get nothing back when the kind is another's, and no reader matches the log
exhaustively.

The reason is what happens when the rule is absent. Core is the layer this
product's correctness lives in; the moment it can name an implementation,
swapping one becomes a change to core, and the thing that decides whether a
2 000-watt heater may be turned up is being edited every time a vendor's API
moves. An edge between two adapters is the same failure one level down: a hidden
coupling between two vendors, in a crate that is supposed to adapt exactly one.
And a store the domain depended on, rather than one that depends on the domain,
would put every column of every aggregate in a crate every consumer rebuilds
against — which is why the store traits live in the domain and the SQLite store
implements them.

It is not a convention. `just check-repo` reads every crate's manifest and
refuses an edge the table does not admit, and the same check refuses a
`printobserver` that names either vendor adapter — because a command that could
reach a printer without the supervisor in between is the one thing this program
exists to make impossible.

## Why the agent reaches this system the way an operator does

The supervising agent has no surface of its own. It works through the
`printobserver` command — the same commands, the same options, the same policy,
the same record. The `command-surface.md` inventory is shared by both actors;
there is no agent-only command.

Three things follow, and they are the reason for the arrangement.

Nothing an agent can do is a path a person cannot audit. Every request either of
them makes is recorded with its actor, its reason and the decision taken on it,
in one history that a person reads with the same command the agent reads it
with.

The action vocabulary is closed at the type level, so there is no G-code field,
no free-form command and no escape hatch anywhere in it — which is what makes it
safe to put an agent near a machine that can set itself on fire. A second,
"internal" surface for the agent would be a second place that closure would have
to hold.

And what the agent may ask for is narrowed by configuration rather than by code:
the safety envelope grants actions per actor class, so an operator can give an
agent less than they have themselves without anything being rebuilt.
