//! The identity rule, and the three identifiers the event envelope reaches.
//!
//! Every identifier this system mints is a UUID version 7, minted by the
//! store, and serialized as its lowercase hyphenated string. Parsing is strict
//! in both directions a permissive UUID reader is loose in: a spelling other
//! than the lowercase hyphenated one is refused rather than normalized, and a
//! version field other than 7 is refused rather than carried, so an identifier
//! this system did not mint cannot enter it by parse.
//!
//! The rule is the [`identifier!`](crate::identifier) macro, and it is
//! exported: a domain that mints an identifier of its own declares it through
//! the macro, in its own crate, and gets the same minting, display, parsing
//! and schema every other identifier has. Declared here are only the three the
//! envelope itself reaches — a print, an event, an image — because those are
//! what every domain and every client must agree on. An identifier one domain
//! keys its own records by is that domain's to declare.
//!
//! An identifier belonging to an external system — a provider's own print id,
//! a firmware's tool number — is held in that system's own representation and
//! named for it, and is never one of these.

use core::fmt;

pub use uuid::Uuid;

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
///
/// This is the parse half of the rule every identifier the [`identifier!`]
/// macro declares applies; it is public so that the macro can reach it from
/// another crate, and is nothing a caller needs by hand.
///
/// # Errors
///
/// Returns [`IdentifierError`] naming the type and what is wrong with the
/// string: a spelling other than the lowercase hyphenated one, text that is
/// not a UUID at all, or a UUID at a version other than 7.
pub fn parse_identifier(type_name: &'static str, text: &str) -> Result<Uuid, IdentifierError> {
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
///
/// `identifier!(Name, "what it identifies")` declares `Name` as a UUID version
/// 7 newtype, minted by [`new`](Self::new), displayed and serialized as its
/// lowercase hyphenated string, refused on parse in any other spelling or at
/// any other version, and emitting the schema every identifier of this system
/// emits. The type crate declares the three the envelope reaches through it;
/// a domain declares its own the same way, in its own crate.
#[macro_export]
macro_rules! identifier {
    ($name:ident, $what:literal) => {
        #[doc = concat!("PrintObserver's own identifier for ", $what, ".")]
        ///
        /// A UUID version 7, minted by the store, serialized as its lowercase
        /// hyphenated string, and refused on parse in any other spelling or at
        /// any other version.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($crate::ids::Uuid);

        impl $name {
            /// Mint a fresh identifier. This is the store's to call.
            #[must_use]
            pub fn new() -> Self {
                Self($crate::ids::Uuid::now_v7())
            }

            /// The identifier as the UUID it is.
            #[must_use]
            pub const fn as_uuid(&self) -> &$crate::ids::Uuid {
                &self.0
            }
        }

        impl ::core::default::Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::write!(formatter, "{}", self.0.as_hyphenated())
            }
        }

        impl ::core::str::FromStr for $name {
            type Err = $crate::IdentifierError;

            fn from_str(text: &str) -> ::core::result::Result<Self, Self::Err> {
                $crate::ids::parse_identifier(stringify!($name), text).map(Self)
            }
        }

        impl $crate::serde::Serialize for $name {
            fn serialize<S: $crate::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.collect_str(&self.0.as_hyphenated())
            }
        }

        impl<'de> $crate::serde::Deserialize<'de> for $name {
            fn deserialize<D: $crate::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<Self, D::Error> {
                let text = <::std::string::String as $crate::serde::Deserialize>::deserialize(
                    deserializer,
                )?;
                text.parse()
                    .map_err(<D::Error as $crate::serde::de::Error>::custom)
            }
        }

        impl $crate::schemars::JsonSchema for $name {
            fn schema_name() -> ::std::borrow::Cow<'static, str> {
                ::std::borrow::Cow::Borrowed(stringify!($name))
            }

            fn json_schema(
                _generator: &mut $crate::schemars::SchemaGenerator,
            ) -> $crate::schemars::Schema {
                $crate::schemars::json_schema!({
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
