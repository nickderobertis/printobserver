//! The wire shapes the self-hosted Obico webhook notification plugin sends.
//!
//! These are held here as this repository's committed claim about an external
//! producer, beside the three samples that model what it sends. What
//! reconciles that claim against the real producer is the scheduled Obico tier,
//! which drives a live self-hosted Obico and fails naming any field that has
//! moved.
//!
//! The producer sends three shapes: the failure alert, a printer notification
//! about a print, which carries `print` and `img_url`, and a printer
//! notification not about a print, which carries neither.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{Error as _, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The one `type` a failure alert's event object carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ObicoFailureEventType {
    /// Obico's own spelling of a print failure.
    #[serde(rename = "PrintFailure")]
    PrintFailure,
}

/// The `type` a printer notification's event object carries.
///
/// These are the producer's own spellings; the normalized vocabulary is
/// [`ObicoNotificationType`](crate::ObicoNotificationType).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ObicoEventType {
    /// A print started.
    PrintStarted,
    /// A print finished.
    PrintDone,
    /// A print was cancelled.
    PrintCancelled,
    /// A print was paused.
    PrintPaused,
    /// A print was resumed.
    PrintResumed,
    /// The printer is waiting for a filament change.
    FilamentChange,
    /// A heater cooled down.
    HeaterCooledDown,
    /// A heater reached its target.
    HeaterTargetReached,
}

/// A Unix timestamp number, or the empty string the producer sends instead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ObicoTimestamp {
    /// Seconds since the Unix epoch.
    Seconds(f64),
    /// The producer sent the empty string, meaning it has no instant to send.
    NotReported,
}

impl Serialize for ObicoTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Seconds(seconds) => serializer.serialize_f64(*seconds),
            Self::NotReported => serializer.serialize_str(""),
        }
    }
}

/// The two forms the producer sends, before the empty string is given meaning.
#[derive(Deserialize)]
#[serde(untagged)]
enum ObicoTimestampWire {
    /// A JSON number.
    Seconds(f64),
    /// A JSON string, which the producer sends only as the empty one.
    Text(String),
}

impl<'de> Deserialize<'de> for ObicoTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match ObicoTimestampWire::deserialize(deserializer)? {
            ObicoTimestampWire::Seconds(seconds) => Ok(Self::Seconds(seconds)),
            ObicoTimestampWire::Text(text) if text.is_empty() => Ok(Self::NotReported),
            ObicoTimestampWire::Text(text) => Err(D::Error::invalid_value(
                Unexpected::Str(&text),
                &"a Unix timestamp number or an empty string",
            )),
        }
    }
}

impl JsonSchema for ObicoTimestamp {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ObicoTimestamp")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "title": "ObicoTimestamp",
            "description": "A Unix timestamp number, or the empty string the producer sends \
                            when it has no instant to send.",
            "anyOf": [
                { "type": "number" },
                { "type": "string", "maxLength": 0 }
            ]
        })
    }
}

/// The `printer` object both shapes carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObicoPrinterInfo {
    /// Obico's own identifier for the printer.
    pub id: i64,
    /// The name Obico shows the printer under.
    pub name: String,
}

/// The `print` object, which the producer sends when there is a print.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObicoPrintInfo {
    /// Obico's own identifier for the print.
    pub id: i64,
    /// The file being printed.
    pub filename: String,
    /// When the print started.
    pub started_at: ObicoTimestamp,
    /// When the print ended.
    pub ended_at: ObicoTimestamp,
}

/// The `event` object a failure alert carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObicoFailureEvent {
    /// Always the producer's own `PrintFailure`.
    #[serde(rename = "type")]
    pub event_type: ObicoFailureEventType,
    /// Whether the producer called it a warning rather than a failure.
    pub is_warning: bool,
    /// Whether the producer paused the print itself.
    pub print_paused: bool,
}

/// The `event` object a printer notification carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObicoNotificationEvent {
    /// Which notification it is, in the producer's own spelling.
    #[serde(rename = "type")]
    pub event_type: ObicoEventType,
    /// Whether the producer called it a warning.
    pub is_warning: bool,
    /// Whether the producer paused the print itself.
    pub print_paused: bool,
}

/// The whole body the producer sends for a print failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObicoFailureAlert {
    /// What happened.
    pub event: ObicoFailureEvent,
    /// Which printer it happened on.
    pub printer: ObicoPrinterInfo,
    /// Which print it happened to.
    pub print: ObicoPrintInfo,
    /// Where the snapshot that shows it can be fetched.
    pub img_url: String,
}

/// The whole body the producer sends for a printer notification.
///
/// `print` and `img_url` are present together when the notification is about a
/// print and absent together when it is not; the absent form is a shape the
/// producer sends rather than an incomplete copy of the present one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObicoPrinterNotification {
    /// What happened.
    pub event: ObicoNotificationEvent,
    /// Which printer it happened on.
    pub printer: ObicoPrinterInfo,
    /// Which print it is about, when it is about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print: Option<ObicoPrintInfo>,
    /// Where the snapshot can be fetched, when the notification carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_url: Option<String>,
}
