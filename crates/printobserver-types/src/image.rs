//! The handle an image travels in context under.
//!
//! The record of an image — where it was fetched from, how many bytes it is,
//! where it lives — is the supervision domain's own; what every domain and
//! every client must agree on is only how an image is named on the envelope,
//! and that is this.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::ImageId;

/// The handle an image travels in context under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRef {
    /// The image's identifier.
    pub id: ImageId,
    /// The SHA-256 of its bytes, lowercase hexadecimal.
    pub sha256: String,
}
