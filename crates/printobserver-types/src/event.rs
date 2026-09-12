//! The event log's envelope: an open kind name and an opaque payload.
//!
//! An [`EventRecord`] is what the store holds and the server serves, and the
//! one thing every domain and every client agree on about it is its shape:
//! the fields every event carries, a `kind` naming which event it is, and a
//! `payload` whose form is the kind's own. Nothing here lists the kinds. Each
//! is declared by the domain that owns the event — a payload type implementing
//! [`EventPayload`] under a [`KIND`](EventPayload::KIND) of its own — so a
//! domain that adds an event edits its own crate and nothing central.
//!
//! # Why the log is open
//!
//! A closed pair — one enum of kinds and one enum of payloads, a variant per
//! domain's events — made the crate every other crate builds against the place
//! every domain's payload had to be written, and let a reader match on another
//! domain's kind by name. The envelope makes neither possible: a reader of the
//! log carries a record through, filters by name, or reads a typed payload out
//! of it with [`EventBody::read`] and is answered nothing when the kind is
//! another's. A record under a kind no crate of this workspace declares — a
//! row written by a newer server, say — survives a round trip unchanged.
//!
//! # The wire form, which is unchanged
//!
//! [`EventBody`] is flattened into the record, so the wire form of a record is
//! still the fields `id`, `print_id`, `source`, `received_at`, `image`, `kind`,
//! `payload` and `raw`, and `{"kind": .., "payload": ..}` is the text the
//! closed pair's tagged form serialized to.

use core::fmt;
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{EventId, PrintId};
use crate::image::ImageRef;
use crate::raw::RawBytes;
use crate::timestamp::Timestamp;

/// The pattern every kind name matches: lowercase `snake_case`.
pub const KIND_PATTERN: &str = "^[a-z][a-z0-9_]*$";

/// The name one kind of event is written down under.
///
/// Lowercase `snake_case`, declared by the domain that owns the event as the
/// [`KIND`](EventPayload::KIND) of its payload type. It serializes as the bare
/// string, and it is ordered and hashable so that a filter can hold a set of
/// them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventKind(String);

impl EventKind {
    /// The kind this name spells.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The name, as it is written down.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl JsonSchema for EventKind {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EventKind")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "title": "EventKind",
            "description": "The name one kind of event is written down under: lowercase snake_case, declared by the domain that owns the event.",
            "pattern": KIND_PATTERN
        })
    }
}

/// Where an event came from, as the bare string each domain declares for itself.
///
/// Nothing here lists the sources; each domain declares its own name as a
/// constant beside the events it raises.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventSource(String);

impl EventSource {
    /// The source this name spells.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The name, as it is written down.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EventSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl JsonSchema for EventSource {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EventSource")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "title": "EventSource",
            "description": "Where an event came from, as the bare string the domain that raised it declares for itself."
        })
    }
}

/// One typed payload declared under a kind name, by the domain that owns it.
///
/// [`KIND`](Self::KIND) is the one authoritative source of the kind's name:
/// the schema marker is written from it by [`event_schema_of`], the store's
/// kind column is written from it, and the clients' tables are generated from
/// the marker.
pub trait EventPayload: Serialize + DeserializeOwned + JsonSchema {
    /// The name this payload's events are written down under.
    const KIND: &'static str;

    /// The kind this payload belongs to.
    #[must_use]
    fn kind() -> EventKind {
        EventKind::new(Self::KIND)
    }
}

/// The kind and the payload, as the log holds them.
///
/// The pair `{"kind": .., "payload": ..}`: the kind is the name a payload type
/// declares, and the payload is that type's value, held as JSON so that a body
/// under a kind this crate has never heard of is carried rather than refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventBody {
    /// Which event this is.
    pub kind: EventKind,
    /// What it carries, in the form its kind declares.
    pub payload: Value,
}

impl EventBody {
    /// The body of one typed payload, under the kind its type declares.
    ///
    /// # Errors
    ///
    /// Returns the serialization failure when the payload cannot be rendered
    /// as JSON, which a payload type declared with `serde` never is.
    pub fn of<P: EventPayload>(payload: &P) -> Result<Self, serde_json::Error> {
        Ok(Self {
            kind: P::kind(),
            payload: serde_json::to_value(payload)?,
        })
    }

    /// Whether this body is under the kind `P` declares.
    #[must_use]
    pub fn is<P: EventPayload>(&self) -> bool {
        self.kind.as_str() == P::KIND
    }

    /// The payload as `P`, when this body is under the kind `P` declares.
    ///
    /// Answers nothing for another kind's body, and the parse failure for a
    /// body under the right kind whose payload is not of the type.
    #[must_use]
    pub fn read<P: EventPayload>(&self) -> Option<Result<P, serde_json::Error>> {
        self.is::<P>()
            .then(|| serde_json::from_value(self.payload.clone()))
    }
}

/// One event, as the store holds it and the server serves it.
///
/// `raw` holds the bytes exactly as received for an externally sourced event
/// and is absent for an internally raised one — it is what makes the history
/// auditable when a normalization turns out to be wrong. `print_id` is
/// optional, because an externally sourced event may name no print this system
/// knows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventRecord {
    /// This event's identifier, minted by the store.
    pub id: EventId,
    /// The print it belongs to, when it belongs to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_id: Option<PrintId>,
    /// Where it came from.
    pub source: EventSource,
    /// When it was received.
    pub received_at: Timestamp,
    /// The image it arrived with, when it arrived with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
    /// The kind and the payload, flattened into the record's own fields.
    #[serde(flatten)]
    pub body: EventBody,
    /// The bytes exactly as received, for an externally sourced event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawBytes>,
}

impl EventRecord {
    /// Which event this is.
    #[must_use]
    pub const fn kind(&self) -> &EventKind {
        &self.body.kind
    }

    /// The payload as `P`, when this record is under the kind `P` declares.
    ///
    /// Answers nothing for a record of another kind; see [`EventBody::read`].
    #[must_use]
    pub fn payload_as<P: EventPayload>(&self) -> Option<Result<P, serde_json::Error>> {
        self.body.read::<P>()
    }
}
