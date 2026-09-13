//! `printobserver-vision-api`.
//!
//! Owns: the port external observations arrive through — the trait for
//! normalizing a received body into an event under the adapter's own kind and
//! for retrieving the image one names, the shapes those methods carry, that
//! port's own error type, and the one event kind this port itself declares:
//! [`MalformedExternalEventPayload`], a body no adapter could read, written
//! down.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
//!
//! # This port is normalization and retrieval only
//!
//! Ingress transport belongs to the server: nothing here listens, routes or
//! authenticates. A body arrives here already received, and what leaves is
//! either a [`NormalizedAlert`] or a [`VisionError`] saying why one could not
//! be made.
//!
//! # This port is provider-neutral
//!
//! A [`NormalizedAlert`] carries an [`EventBody`] under whatever kind the
//! adapter declares, and beside it the one thing the supervision domain reads
//! off an alert: [`ProviderPrint`], the provider's own identifier for the print
//! the alert is about. The supervision core correlates on that and never on a
//! provider's payload by kind, which is what lets a second provider's adapter
//! land without the core hearing of it.
//!
//! # Why the methods answer a boxed future
//!
//! Every method is asynchronous, and the trait is dyn-compatible and shareable
//! across threads, because the supervision core holds every port behind
//! `Arc<dyn Port>`. An `async fn` in a trait is not dyn-compatible, so each
//! method answers a [`BoxFuture`] instead.

use core::future::Future;
use core::pin::Pin;

use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventBody, EventKind, EventPayload, EventSource, RawBytes, Timestamp};

/// A future this port's methods answer with, in the one shape a trait object
/// can carry.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The provider's own identifier for the print an alert is about.
///
/// Held in the provider's representation — a 64-bit integer, which is what
/// every provider this system has met uses — and named for what it is rather
/// than for the provider, so that the supervision domain correlates on it
/// without knowing whose it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ProviderPrint {
    /// The provider's own identifier for the print.
    pub id: i64,
    /// The file the provider named, when it named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// One external body, read into an event under the adapter's own kind.
///
/// `kind` and `payload` are the [`EventBody`] flattened into this shape, so an
/// alert's wire form is the pair the store holds. `print` is what the
/// supervision domain correlates on; an alert about no print carries none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub struct NormalizedAlert {
    /// Where it came from.
    pub source: EventSource,
    /// When it was received.
    pub received_at: Timestamp,
    /// The kind and the payload, flattened into this shape's own fields.
    #[serde(flatten)]
    pub body: EventBody,
    /// The bytes exactly as received.
    pub raw: RawBytes,
    /// The image this alert names, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// The provider's own identifier for the print this alert is about, and
    /// the file it named, when the alert is about a print at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print: Option<ProviderPrint>,
}

impl NormalizedAlert {
    /// Which event this is.
    #[must_use]
    pub const fn kind(&self) -> &EventKind {
        &self.body.kind
    }
}

// The written-down form of `VisionError::Malformed`, which is why this port
// declares it rather than any one adapter. Said here rather than in the
// description, which is the contract's and travels into every client.
/// An external body arrived that could not be read.
///
/// This kind always carries its `raw` bytes, and it exists so that an alert
/// this system cannot read is written down rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct MalformedExternalEventPayload {
    /// One line saying why the body could not be read.
    pub detail: String,
}

impl EventPayload for MalformedExternalEventPayload {
    const KIND: &'static str = "malformed_external_event";
}

impl Sample for MalformedExternalEventPayload {
    fn sample_full() -> Self {
        Self {
            detail: "the body is not JSON: expected value at line 1 column 1".to_owned(),
        }
    }
}

/// One image, as it was retrieved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FetchedImage {
    /// The image's bytes, exactly as served.
    pub bytes: RawBytes,
    /// The content type it was served as.
    pub content_type: String,
}

/// Why a body could not be read, or an image could not be retrieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisionError {
    /// The body could not be read, and here it is.
    Malformed {
        /// One line saying why it could not be read.
        detail: String,
        /// The bytes exactly as received, so that the body is not lost.
        raw: RawBytes,
    },
    /// The source took too long.
    TimedOut,
    /// What arrived is larger than this port will read.
    TooLarge {
        /// The limit, in bytes.
        limit: i64,
    },
    /// What arrived is a content type this port does not accept.
    UnacceptableContentType {
        /// The content type that arrived.
        content_type: String,
    },
    /// The source could not be reached at all.
    Unreachable {
        /// What went wrong reaching it.
        detail: String,
    },
}

impl core::fmt::Display for VisionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed { detail, raw } => {
                write!(formatter, "the body could not be read ({detail}); {raw}")
            }
            Self::TimedOut => formatter.write_str("the source took too long"),
            Self::TooLarge { limit } => {
                write!(
                    formatter,
                    "what arrived is larger than the {limit} byte limit"
                )
            }
            Self::UnacceptableContentType { content_type } => {
                write!(
                    formatter,
                    "{content_type} is not a content type this port accepts"
                )
            }
            Self::Unreachable { detail } => {
                write!(formatter, "the source is unreachable: {detail}")
            }
        }
    }
}

impl core::error::Error for VisionError {}

/// The port external observations arrive through.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because the supervision core holds it behind
/// `Arc<dyn VisionPort>`.
pub trait VisionPort: Send + Sync {
    /// Read a received body into an event under this adapter's own kind.
    fn normalize(
        &self,
        body: RawBytes,
        content_type: Option<String>,
    ) -> BoxFuture<'_, Result<NormalizedAlert, VisionError>>;

    /// Retrieve the image a source URL names.
    fn fetch_image(&self, source_url: String) -> BoxFuture<'_, Result<FetchedImage, VisionError>>;
}
