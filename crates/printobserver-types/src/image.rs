//! Images fetched for a print, and the handle one travels in context under.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{EventId, ImageId, PrintId};
use crate::timestamp::Timestamp;

/// One image, stored beside the event it arrived with.
///
/// `relative_path` is relative to the configured state directory, so that the
/// store stays portable. Materializing an image resolves that to an absolute
/// path; nothing in this system renders image bytes into JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRecord {
    /// This image's identifier, minted by the store.
    pub id: ImageId,
    /// The print it belongs to.
    pub print_id: PrintId,
    /// The event it arrived with.
    pub event_id: EventId,
    /// Where it was fetched from, when it was fetched from somewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// When it was fetched.
    pub fetched_at: Timestamp,
    /// The content type it was served as.
    pub content_type: String,
    /// How many bytes it is.
    pub byte_len: i64,
    /// The SHA-256 of its bytes, lowercase hexadecimal.
    pub sha256: String,
    /// Where it lives, relative to the configured state directory.
    pub relative_path: String,
}

/// The handle an image travels in context under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRef {
    /// The image's identifier.
    pub id: ImageId,
    /// The SHA-256 of its bytes, lowercase hexadecimal.
    pub sha256: String,
}
