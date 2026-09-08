//! The bytes an externally sourced event arrived as.
//!
//! Held exactly as received and serialized as a base64 string, because JSON
//! denotes no byte sequence: it is what makes the history auditable when a
//! normalization turns out to have been wrong.

use core::fmt;
use std::borrow::Cow;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Bytes exactly as received, serialized as a base64 string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct RawBytes(Vec<u8>);

impl RawBytes {
    /// Hold these bytes.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// The bytes, exactly as received.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for RawBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&STANDARD.encode(&self.0))
    }
}

impl Serialize for RawBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for RawBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD
            .decode(text.as_bytes())
            .map(Self)
            .map_err(D::Error::custom)
    }
}

impl JsonSchema for RawBytes {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("RawBytes")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "title": "RawBytes",
            "description": "Bytes exactly as received, base64-encoded.",
            "contentEncoding": "base64"
        })
    }
}
