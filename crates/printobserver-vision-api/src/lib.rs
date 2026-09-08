//! `printobserver-vision-api`.
//!
//! Owns: the port external observations arrive through — the trait for
//! normalizing a received body into this system's own event vocabulary and for
//! retrieving the image one names, the two shapes those methods carry, and that
//! port's own error type.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
//!
//! # This port is normalization and retrieval only
//!
//! Ingress transport belongs to the server: nothing here listens, routes or
//! authenticates. A body arrives here already received, and what leaves is
//! either a [`NormalizedAlert`] in this system's own vocabulary or a
//! [`VisionError`] saying why one could not be made.
//!
//! # Why the methods answer a boxed future
//!
//! Every method is asynchronous, and the trait is dyn-compatible and shareable
//! across threads, because the supervision core holds all four ports behind
//! `Arc<dyn Port>`. An `async fn` in a trait is not dyn-compatible, so each
//! method answers a [`BoxFuture`] instead.

use core::future::Future;
use core::pin::Pin;

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventKind, EventPayload, EventSource, RawBytes, Timestamp};

/// A future this port's methods answer with, in the one shape a trait object
/// can carry.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One external body, read into this system's own event vocabulary.
///
/// `kind` and `payload` are the one closed pair
/// [`EventPayload`](printobserver_types::EventPayload) declares, so a
/// normalization naming one kind while carrying another's payload is
/// unrepresentable here as it is in the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub struct NormalizedAlert {
    /// Where it came from.
    pub source: EventSource,
    /// When it was received.
    pub received_at: Timestamp,
    /// The kind and the payload, which are one closed pair.
    #[serde(flatten)]
    pub payload: EventPayload,
    /// The bytes exactly as received.
    pub raw: RawBytes,
    /// The image this alert names, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

impl NormalizedAlert {
    /// Which event this is, read off the payload it carries.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        self.payload.kind()
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
    /// Read a received body into this system's own event vocabulary.
    fn normalize(
        &self,
        body: RawBytes,
        content_type: Option<String>,
    ) -> BoxFuture<'_, Result<NormalizedAlert, VisionError>>;

    /// Retrieve the image a source URL names.
    fn fetch_image(&self, source_url: String) -> BoxFuture<'_, Result<FetchedImage, VisionError>>;
}
