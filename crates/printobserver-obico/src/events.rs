//! The event kinds and the source name this adapter writes.
//!
//! Obico's two bodies — a failure alert and a printer notification — are each
//! written down under a kind of this adapter's own, declared here and nowhere
//! else. The supervision core never reads either by kind: it correlates on the
//! [`ProviderPrint`](printobserver_vision_api::ProviderPrint) the adapter hands
//! it beside the body, and carries the body through as it was read.

use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventPayload, EventSource, Timestamp};

/// The source name of an event Obico posted.
pub const OBICO_SOURCE: &str = "obico";

/// The source of an event Obico posted, over its webhook.
#[must_use]
pub fn obico_source() -> EventSource {
    EventSource::new(OBICO_SOURCE)
}

/// The kind of printer notification Obico sent, normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum ObicoNotificationType {
    /// A print started.
    Started,
    /// A print finished.
    Done,
    /// A print was cancelled.
    Cancelled,
    /// A print was paused.
    Paused,
    /// A print was resumed.
    Resumed,
    /// The printer is waiting for a filament change.
    FilamentChange,
    /// A heater cooled down.
    HeaterCooled,
    /// A heater reached its target.
    HeaterTarget,
}

impl Sample for ObicoNotificationType {
    fn sample_full() -> Self {
        Self::Started
    }
}

/// Obico reported a print failure.
///
/// The two instants are optional because Obico's own field for each is a Unix
/// timestamp number, an empty string, or absent, and the last two both mean the
/// producer reported no instant. An absent field here is that, never an epoch
/// date standing in for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ObicoFailureAlertPayload {
    /// Whether Obico called it a warning rather than a failure.
    pub is_warning: bool,
    /// Whether Obico paused the print itself.
    pub print_paused: bool,
    /// Obico's own identifier for the print, when it named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obico_print_id: Option<i64>,
    /// The file being printed, when Obico named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// When the print started, when Obico reported an instant for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<Timestamp>,
    /// When the print ended, when Obico reported an instant for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

impl EventPayload for ObicoFailureAlertPayload {
    const KIND: &'static str = "obico_failure_alert";
}

/// A later fixed instant, for a sample's end.
fn later_instant() -> Timestamp {
    "2026-03-01T12:30:00Z"
        .parse()
        .expect("a fixed RFC 3339 instant")
}

impl Sample for ObicoFailureAlertPayload {
    fn sample_full() -> Self {
        Self {
            is_warning: true,
            print_paused: false,
            obico_print_id: Some(4211),
            file_name: Some("benchy.gcode".to_owned()),
            started_at: Some(Timestamp::sample_full()),
            ended_at: Some(later_instant()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            obico_print_id: None,
            file_name: None,
            started_at: None,
            ended_at: None,
            ..Self::sample_full()
        }
    }
}

/// Obico sent a printer notification.
///
/// The two instants are optional for the same reason
/// [`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of
/// the print's fields when the notification is about no print at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ObicoPrinterNotificationPayload {
    /// Which notification it is.
    pub notification_type: ObicoNotificationType,
    /// Obico's own identifier for the print, when the notification is about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obico_print_id: Option<i64>,
    /// The file being printed, when the notification is about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// When the print started, when Obico reported an instant for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<Timestamp>,
    /// When the print ended, when Obico reported an instant for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

impl EventPayload for ObicoPrinterNotificationPayload {
    const KIND: &'static str = "obico_printer_notification";
}

impl Sample for ObicoPrinterNotificationPayload {
    fn sample_full() -> Self {
        Self {
            notification_type: ObicoNotificationType::Started,
            obico_print_id: Some(4211),
            file_name: Some("benchy.gcode".to_owned()),
            started_at: Some(Timestamp::sample_full()),
            ended_at: Some(later_instant()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            obico_print_id: None,
            file_name: None,
            started_at: None,
            ended_at: None,
            ..Self::sample_full()
        }
    }
}
