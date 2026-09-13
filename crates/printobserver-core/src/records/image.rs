//! Images fetched for a print.
//!
//! The handle an image travels in context under is the envelope's own
//! ([`ImageRef`](printobserver_types::ImageRef)); the record of it is this
//! domain's.

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventId, ImageId, PrintId, Timestamp};

/// One image, stored beside the event it arrived with.
///
/// `relative_path` is relative to the configured state directory, so that the
/// store stays portable. Materializing an image resolves that to an absolute
/// path; nothing in this system renders image bytes into JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
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
