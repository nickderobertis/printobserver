//! `printobserver-types`.
//!
//! Owns: the vocabulary every domain and every client must agree on, and only
//! that — the identity rule ([`ids`]: the exported [`identifier!`] macro, its
//! error, the UUID-v7 rule, and the three identifiers the envelope reaches),
//! the representation rules ([`Timestamp`], [`RawBytes`], [`FileName`],
//! [`Reported`] and [`Range`]), the event log's envelope ([`event`], with the
//! image handle [`ImageRef`] it carries), and the schema toolkit
//! ([`contract`]). It holds data and total functions over that data, never
//! I/O, and (until the next step of the domain cut moves them with the printer
//! and supervisor ports) `PrinterState`, `Adjustable` and the session types.
//!
//! A type belongs here only if adding or changing one domain's concept does
//! not require editing it. The event log is the case in point: [`event`]
//! declares the envelope — an open [`EventKind`] name and an opaque payload —
//! and **no kind**. Each kind is declared by the domain that owns the event, as
//! a payload type implementing [`EventPayload`] under a `KIND` of its own, so
//! that a domain adding an event edits its own crate and nothing central. The
//! same holds for a domain's own shapes: a provider's wire format lives in its
//! adapter, a port's vocabulary in the port, and the supervision domain's
//! records — the print, the action, the intervention, the policy, the image —
//! and the identifiers it mints live in the supervision domain, each declared
//! into the schema set through [`TypeContract::of`] rather than through a
//! list here.
//!
//! May depend on: no crate of this workspace. It is the root of the graph, so a
//! type it does not hold is a type the rest of the workspace cannot agree on.
//!
//! # One authoritative source per restated fact
//!
//! The types here are declared once and imported by every consumer rather than
//! re-declared. The JSON Schemas the clients and the agent's structured answer
//! are validated against are **generated from these types** by the
//! `printobserver-types:schemas` graph target, which the gate runs: a schema
//! checked into `schemas/` that no longer matches what these types generate
//! fails that target. Nothing transcribes a schema by hand, and nothing
//! restates a field.
//!
//! [`contract`] is where that generation reads the declared set from: it is
//! this crate's own claim about which types it declares and what a canonical
//! value of each looks like.
//!
//! # The representations, stated once
//!
//! * **Identifiers.** Every identifier this system mints is a UUID version 7,
//!   minted by the store, held as a distinct newtype per record, and serialized
//!   as its lowercase hyphenated string. See [`ids`]; a domain declares its own
//!   through the [`identifier!`] macro.
//! * **Numbers.** Every temperature, factor, multiplier, percentage and
//!   fraction is a 64-bit float. Every byte count and every integer identifier
//!   an external system supplies is a 64-bit signed integer. Every duration is
//!   whole seconds as a 64-bit signed integer.
//! * **Timestamps.** Every timestamp is an instant in UTC, serialized as an RFC
//!   3339 string with a zero offset. See [`Timestamp`].
//! * **Optionality.** An optional field absent means *the source did not report
//!   it*, and is never rendered as a zero, an empty string, or a default.
//!   Absent serializes as absent and parses back as absent.
//! * **Reported numbers.** A numeric field a domain declares a plausibility
//!   range for is typed [`Reported<f64>`]. See [`reported`] for the whole rule,
//!   including why a non-finite value is out of range and why one is refused
//!   on emission; the ranges themselves are the declaring domain's.
//!
//! # Why `serde`, `schemars` and `serde_json` are re-exported
//!
//! The four port crates depend on this crate and on nothing else, and each of
//! them declares request and answer shapes that cross a process boundary and so
//! must derive the same traits these types derive. They reach the derive macros
//! through [`serde`], [`schemars`] and [`serde_json`] here rather than by
//! declaring a dependency of their own.

pub mod adjustable;
pub mod contract;
pub mod event;
pub mod file_name;
pub mod ids;
pub mod image;
pub mod printer;
pub mod raw;
pub mod reported;
pub mod session;
pub mod timestamp;

pub use adjustable::{Adjustable, AdjustableError};
pub use contract::{
    EVENT_KIND_MARKER, Sample, TypeContract, WireField, declared, event_schema_of, schema_of,
    wire_fields,
};
pub use event::{
    EventBody, EventKind, EventKindError, EventPayload, EventRecord, EventSource, KIND_PATTERN,
};
pub use file_name::{FileName, FileNameError, FileNameRefusal, SEPARATORS};
pub use ids::{EventId, IdentifierError, ImageId, PrintId};
pub use image::ImageRef;
pub use printer::PrinterState;
pub use raw::RawBytes;
pub use reported::{Range, Reported};
pub use session::{SessionPhase, SupervisionSession};
pub use timestamp::{Timestamp, TimestampError};

pub use schemars;
pub use serde;
pub use serde_json;
