//! The wire shapes the self-hosted Obico webhook notification plugin sends.
//!
//! These are held here, in the one crate that reads them, as this repository's
//! committed claim about an external producer, beside the three samples that
//! model what it sends. What reconciles that claim against the real producer
//! is the scheduled Obico tier, which drives a live self-hosted Obico and fails
//! naming any field that has moved. A change to this producer's wire format
//! edits this crate and rebuilds it and the composition roots, and nothing
//! central.
//!
//! The producer sends three shapes: the failure alert, a printer notification
//! about a print, which carries `print` and `img_url`, and a printer
//! notification not about a print, which carries neither. One sample per shape
//! is committed under this crate's own `samples/obico/`, and the third is not
//! an incomplete copy of the second: it is what the producer sends when there
//! is no print, and its two absences are the whole reason it is committed.
//!
//! Each shape carries a [`Sample`] beside it, which is what this crate's
//! `schemas` test writes the checked-in schema set from and walks the wire
//! forms over.

use std::borrow::Cow;

use printobserver_types::contract::Sample;
use printobserver_types::schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use printobserver_types::serde::de::{Error as _, Unexpected};
use printobserver_types::serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The one `type` a failure alert's event object carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
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
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
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
#[serde(crate = "printobserver_types::serde", untagged)]
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
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ObicoPrinterInfo {
    /// Obico's own identifier for the printer.
    pub id: i64,
    /// The name Obico shows the printer under.
    pub name: String,
}

/// The `print` object, which the producer sends when there is a print.
///
/// Each instant has **three** input states rather than two, and the option and
/// the enum carry one each: a number is [`ObicoTimestamp::Seconds`], the empty
/// string the producer sends for a print that has not started or ended is
/// [`ObicoTimestamp::NotReported`], and a field the producer omits altogether
/// is `None`. The last two both mean the producer reported no instant, and
/// neither is refused and neither is an epoch date; they are held apart here
/// only so that a body round-trips back into exactly the form it arrived in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ObicoPrintInfo {
    /// Obico's own identifier for the print.
    pub id: i64,
    /// The file being printed.
    pub filename: String,
    /// When the print started, absent when the producer omits the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<ObicoTimestamp>,
    /// When the print ended, absent when the producer omits the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<ObicoTimestamp>,
}

/// The `event` object a failure alert carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
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
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
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
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
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
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
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

/// A fixed count of seconds, so that a sample is the same on every run.
const SAMPLE_SECONDS: f64 = 1_772_366_400.5;

/// The one snapshot address every sample names.
const SAMPLE_IMAGE_URL: &str = "https://obico.example/snapshots/4211.jpg";

impl Sample for ObicoFailureEventType {
    fn sample_full() -> Self {
        Self::PrintFailure
    }
}

impl Sample for ObicoEventType {
    fn sample_full() -> Self {
        Self::PrintStarted
    }
}

impl Sample for ObicoTimestamp {
    fn sample_full() -> Self {
        Self::Seconds(SAMPLE_SECONDS)
    }
}

impl Sample for ObicoPrinterInfo {
    fn sample_full() -> Self {
        Self {
            id: 17,
            name: "Prusa MK4".to_owned(),
        }
    }
}

impl Sample for ObicoPrintInfo {
    fn sample_full() -> Self {
        Self {
            id: 4211,
            filename: "benchy.gcode".to_owned(),
            started_at: Some(ObicoTimestamp::Seconds(SAMPLE_SECONDS)),
            ended_at: Some(ObicoTimestamp::NotReported),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            started_at: None,
            ended_at: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for ObicoFailureEvent {
    fn sample_full() -> Self {
        Self {
            event_type: ObicoFailureEventType::PrintFailure,
            is_warning: true,
            print_paused: false,
        }
    }
}

impl Sample for ObicoNotificationEvent {
    fn sample_full() -> Self {
        Self {
            event_type: ObicoEventType::PrintStarted,
            is_warning: false,
            print_paused: false,
        }
    }
}

impl Sample for ObicoFailureAlert {
    fn sample_full() -> Self {
        Self {
            event: ObicoFailureEvent::sample_full(),
            printer: ObicoPrinterInfo::sample_full(),
            print: ObicoPrintInfo::sample_full(),
            img_url: SAMPLE_IMAGE_URL.to_owned(),
        }
    }
}

impl Sample for ObicoPrinterNotification {
    fn sample_full() -> Self {
        Self {
            event: ObicoNotificationEvent::sample_full(),
            printer: ObicoPrinterInfo::sample_full(),
            print: Some(ObicoPrintInfo::sample_full()),
            img_url: Some(SAMPLE_IMAGE_URL.to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            print: None,
            img_url: None,
            ..Self::sample_full()
        }
    }
}
