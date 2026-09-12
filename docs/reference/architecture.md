# The architecture

What each crate of this workspace owns, and the two structural rules the whole
design rests on. This document covers the crates — one entry each — why core
names no implementation crate, and why the agent reaches this system the way an
operator does.

Everything asserted here is asserted against the tree rather than as prose: the
crates named below are the ones the workspace declares, and the dependency
direction stated is the one the manifests carry. A check reads both beside this
document and refuses one that names a crate the workspace does not have, omits
one it does, or asserts an edge the manifests contradict.

## The crates

Thirteen crates in three layers and one root: the contracts, the four ports and
the four adapters behind them, the supervision logic, and the two composition
roots that choose what runs.

### printobserver-types

The vocabulary every domain and every client must agree on: the identity and
representation rules, the event log's envelope — an open `EventKind` name and an
opaque payload, with no kind declared here — and the schema toolkit, together
with the printer, policy and supervision records the later steps of the domain
cut have yet to move to their owners. A type belongs here only if adding or
changing one domain's concept does not require editing it. Data and total
functions over data, never I/O. It is the root of the graph and depends on no
crate of this workspace.

### printobserver-printer-api

The port the physical printer speaks through: reading printer and job state, and
asking the machine for one of the bounded things the action vocabulary names.
Depends on the contracts alone.

### printobserver-vision-api

The port external observations arrive through: normalizing a received body into
an event under the adapter's own kind, beside the provider-neutral print
correlation the supervision core reads, and retrieving the image that body
names. It declares the one kind of its own — a body no adapter could read,
written down. Depends on the contracts alone.

### printobserver-supervisor-api

The port the supervising agent is reached through: running one supervision turn,
and closing a session; and the two kinds a session's opening and closing are
written down under. Depends on the contracts alone.

### printobserver-store-api

The port durable state speaks through: recording and replaying observations,
decisions and job history. Depends on the contracts alone.

### printobserver-octoprint

The OctoPrint adapter — the one implementation of the printer port. It is the
only crate in this workspace that may construct an OctoPrint request: everything
above it is written as though printers were normal, so that one vendor's own
surface has exactly one place it has to be kept right.

### printobserver-obico

The Obico adapter — the one implementation of the vision port. It reads the body
Obico's webhook notification plugin posts into an event under one of the two
kinds it declares for itself, fetches the snapshot that body names, and writes
both down through the ingress. The supervision core never reads its kinds by
name; it correlates on the print the adapter hands over beside the body.

### printobserver-oneharness

The oneharness adapter — the one implementation of the supervisor port. It
drives an agentic coding harness through OneHarness's own Rust crate API, in
process, and it spawns nothing. The skill it sends as the system prompt, the
template it fills for one turn, and the reference documents that skill links to
are assets it ships — the composition root writes them into the state directory
beside one another, because an installed host has no checkout to read them from.

### printobserver-store-sqlite

The SQLite adapter — the one durable implementation of the store port, with its
schema, its migrations and the content-addressed image files beside the
database.

### printobserver-core

The supervision logic: the loop that turns printer state and vision observations
into supervisory decisions, the policy that decides which of them may reach the
printer, the effective bounds a print runs under, and the expiry of bounded
interventions — together with the seven event kinds and three source names that
logic writes, declared here and nowhere central.

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
on the contracts and an HTTP client and on neither the core nor the server.

### printobserver

The `printobserver` command — the single installable artifact. Its `server`
subcommand runs the supervisor; every other subcommand is one request to an
already-running one. It builds its whole command surface by folding over what
the server declares, so it holds no list of commands or options of its own.

## Why core names no implementation crate

`printobserver-core` may depend on the contracts and on the four ports, and on
no implementation crate. No implementation crate may depend on another.
`printobserver-server` and `printobserver` are the composition roots and are the
only crates permitted to name an implementation.

Stated against the manifests rather than as prose: `printobserver-core` depends
on `printobserver-printer-api`, `printobserver-store-api`,
`printobserver-supervisor-api`, `printobserver-types` and
`printobserver-vision-api`, and on no implementation crate. A check reads that
sentence beside the workspace's own manifests and refuses one they contradict.

The reason is what happens when the rule is absent. Core is the layer this
product's correctness lives in; the moment it can name an implementation,
swapping one becomes a change to core, and the thing that decides whether a
2 000-watt heater may be turned up is being edited every time a vendor's API
moves. An edge between two adapters is the same failure one level down: a hidden
coupling between two vendors, in a crate that is supposed to adapt exactly one.

It is not a convention. `just check-repo` reads every crate's manifest and
refuses an edge that breaks it, and the same check refuses a `printobserver`
that names either vendor adapter — because a command that could reach a printer
without the supervisor in between is the one thing this program exists to make
impossible.

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
