//! The one instant representation this vocabulary has.
//!
//! Every timestamp is an instant in UTC, serialized as an RFC 3339 string with
//! a zero offset. There is no local-time representation anywhere in this crate:
//! a string carrying a non-zero offset parses to the instant it denotes and
//! re-emits at a zero offset, and a string carrying no offset at all is refused
//! rather than guessed at.

use core::fmt;
use core::str::FromStr;
use std::borrow::Cow;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Why a string is not an instant this vocabulary can hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampError {
    /// What is wrong with the string.
    detail: String,
}

impl TimestampError {
    /// What is wrong with the string.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for TimestampError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "not an RFC 3339 instant: {}", self.detail)
    }
}

impl core::error::Error for TimestampError {}

/// An instant in UTC, serialized as an RFC 3339 string with a zero offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// The instant now.
    #[must_use]
    pub fn now() -> Self {
        Self(Utc::now())
    }

    /// The instant this many seconds after the Unix epoch.
    ///
    /// # Errors
    ///
    /// Returns an error when the count of seconds denotes no representable
    /// instant.
    pub fn from_unix_seconds(seconds: i64) -> Result<Self, TimestampError> {
        DateTime::from_timestamp(seconds, 0)
            .map(Self)
            .ok_or_else(|| TimestampError {
                detail: format!(
                    "{seconds} seconds from the Unix epoch is not a representable instant"
                ),
            })
    }

    /// The instant this many seconds after this one.
    ///
    /// Sub-second precision is kept rather than dropped: an adjustment asked to
    /// stand for a minute stands for a minute, rather than for a minute less
    /// however far into a second it happened to be made.
    ///
    /// # Errors
    ///
    /// Returns an error when the result is not a representable instant.
    pub fn plus_seconds(self, seconds: i64) -> Result<Self, TimestampError> {
        // `try_seconds` rather than `seconds`: a count of seconds that is not a
        // representable span is answered here rather than raised, because the
        // caller is a duration somebody asked for.
        TimeDelta::try_seconds(seconds)
            .and_then(|span| self.0.checked_add_signed(span))
            .map(Self)
            .ok_or_else(|| TimestampError {
                detail: format!("{seconds} seconds after {self} is not a representable instant"),
            })
    }

    /// The instant as the UTC date-time it is.
    #[must_use]
    pub const fn as_utc(&self) -> &DateTime<Utc> {
        &self.0
    }
}

impl From<DateTime<Utc>> for Timestamp {
    fn from(instant: DateTime<Utc>) -> Self {
        Self(instant)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `use_z` is what makes the offset a literal zero rather than `+00:00`,
        // and `AutoSi` keeps whatever sub-second precision the instant carries
        // so that an instant survives a round trip unchanged.
        write!(
            formatter,
            "{}",
            self.0.to_rfc3339_opts(SecondsFormat::AutoSi, true)
        )
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        DateTime::parse_from_rfc3339(text)
            .map(|offset| Self(offset.with_timezone(&Utc)))
            .map_err(|error| TimestampError {
                detail: format!("{text:?}: {error}"),
            })
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

impl JsonSchema for Timestamp {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Timestamp")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "format": "date-time",
            "title": "Timestamp",
            "description": "An instant in UTC, as an RFC 3339 string with a zero offset."
        })
    }
}
