//! The identifiers this system mints, one distinct newtype per record.
//!
//! Every one of them is a UUID version 7, minted by the store, and serialized
//! as its lowercase hyphenated string. Parsing is strict in both directions a
//! permissive UUID reader is loose in: a spelling other than the lowercase
//! hyphenated one is refused rather than normalized, and a version field other
//! than 7 is refused rather than carried, so an identifier this system did not
//! mint cannot enter it by parse.
//!
//! An identifier belonging to an external system — Obico's own print id, an
//! `OctoPrint` tool number — is held in that system's own representation and
//! named for it, and is never one of these.

use core::fmt;
use core::str::FromStr;
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use uuid::Uuid;

/// The length of a hyphenated UUID: the one spelling these identifiers take.
const HYPHENATED_LEN: usize = 36;

/// The UUID version every identifier this system mints carries.
const REQUIRED_VERSION: usize = 7;

/// Why a string is not one of this system's identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierError {
    /// The type name the string was being read as.
    type_name: &'static str,
    /// What is wrong with the string.
    detail: String,
}

impl IdentifierError {
    /// The type name the string was being read as.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// What is wrong with the string.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} is not valid: {}",
            self.type_name, self.detail
        )
    }
}

impl core::error::Error for IdentifierError {}

/// Read the one spelling this system accepts, refusing every other.
fn parse_identifier(type_name: &'static str, text: &str) -> Result<Uuid, IdentifierError> {
    let refuse = |detail: String| IdentifierError { type_name, detail };
    if text.len() != HYPHENATED_LEN {
        return Err(refuse(format!(
            "expected the lowercase hyphenated spelling of {HYPHENATED_LEN} characters, \
             found {} characters in {text:?}",
            text.len()
        )));
    }
    if text.chars().any(|character| character.is_ascii_uppercase()) {
        return Err(refuse(format!(
            "expected the lowercase spelling, found an uppercase character in {text:?}"
        )));
    }
    let uuid = Uuid::parse_str(text)
        .map_err(|error| refuse(format!("{text:?} is not a UUID: {error}")))?;
    if uuid.get_version_num() != REQUIRED_VERSION {
        return Err(refuse(format!(
            "expected a version {REQUIRED_VERSION} UUID, found version {} in {text:?}",
            uuid.get_version_num()
        )));
    }
    Ok(uuid)
}

/// Declare one identifier newtype: minting, display, strict parsing and schema.
macro_rules! identifier {
    ($name:ident, $what:literal) => {
        #[doc = concat!("PrintObserver's own identifier for ", $what, ".")]
        ///
        /// A UUID version 7, minted by the store, serialized as its lowercase
        /// hyphenated string, and refused on parse in any other spelling or at
        /// any other version.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Mint a fresh identifier. This is the store's to call.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// The identifier as the UUID it is.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}", self.0.as_hyphenated())
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                parse_identifier(stringify!($name), text).map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(&self.0.as_hyphenated())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse().map_err(D::Error::custom)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "format": "uuid",
                    "title": stringify!($name),
                    "description": concat!(
                        "A lowercase hyphenated version 7 UUID identifying ", $what, "."
                    ),
                    "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$"
                })
            }
        }
    };
}

identifier!(PrintId, "one print");
identifier!(EventId, "one event");
identifier!(ImageId, "one image");
identifier!(ActionId, "one requested action");
identifier!(InterventionId, "one bounded intervention");
