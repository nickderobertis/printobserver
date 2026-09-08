//! The history: one closed pair of an event kind and the payload it carries.
//!
//! [`EventKind`] and [`EventPayload`] are one closed pair rather than two
//! fields that happen to agree. Every kind has exactly one payload variant and
//! every payload variant has exactly one kind, so a consumer that matches the
//! kind knows the payload's shape, and a mismatched pair is unrepresentable
//! rather than merely undocumented: an [`EventRecord`] carries the payload, the
//! `kind` on the wire is that payload's own tag, and a serialized record naming
//! one kind while carrying another's payload fails to parse.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::action::{AcknowledgementDisposition, PrintAction};
use crate::adjustable::Adjustable;
use crate::assessment::AgentAssessment;
use crate::ids::{ActionId, EventId, InterventionId, PrintId};
use crate::image::ImageRef;
use crate::intervention::InterventionOutcome;
use crate::policy::PolicyDecision;
use crate::raw::RawBytes;
use crate::timestamp::Timestamp;

/// Where an event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EventSource {
    /// Obico, over its webhook.
    Obico,
    /// A person.
    Operator,
    /// The supervising agent.
    Agent,
    /// The supervisor itself.
    System,
}

/// The kind of printer notification Obico sent, normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
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

/// Obico reported a print failure.
///
/// The two instants are optional because Obico's own field for each is a Unix
/// timestamp number, an empty string, or absent, and the last two both mean the
/// producer reported no instant. An absent field here is that, never an epoch
/// date standing in for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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

/// Obico sent a printer notification.
///
/// The two instants are optional for the same reason
/// [`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of
/// the print's fields when the notification is about no print at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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

/// An external body arrived that could not be read.
///
/// This kind always carries its `raw` bytes, and it exists so that an alert
/// this system cannot read is written down rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MalformedExternalEventPayload {
    /// One line saying why the body could not be read.
    pub detail: String,
}

/// An actor asked for an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRequestedPayload {
    /// The action's identifier.
    pub action_id: ActionId,
    /// What was asked for.
    pub action: PrintAction,
    /// Who asked.
    pub actor: crate::action::Actor,
}

/// An accepted action reached the printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionExecutedPayload {
    /// The action's identifier.
    pub action_id: ActionId,
    /// The bounded intervention it opened, when it opened one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intervention_id: Option<InterventionId>,
}

/// Policy refused an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRejectedPayload {
    /// The action's identifier.
    pub action_id: ActionId,
    /// The whole decision, carrying which rejection it was.
    pub decision: PolicyDecision,
}

/// A bounded intervention expired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InterventionExpiredPayload {
    /// The intervention's identifier.
    pub intervention_id: InterventionId,
    /// What it had changed.
    pub adjustable: Adjustable,
    /// What became of it.
    pub outcome: InterventionOutcome,
}

/// A supervision session was opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupervisionSessionOpenedPayload {
    /// The session's own name in the harness.
    pub session_name: String,
    /// The identity the harness ran it under.
    pub harness_identity: String,
}

/// A supervision session was closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupervisionSessionClosedPayload {
    /// The session's own name in the harness.
    pub session_name: String,
    /// Why it was closed.
    pub close_reason: String,
}

/// The agent wrote down what it made of a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentAssessmentPayload {
    /// The session the turn ran in.
    pub session_name: String,
    /// What the agent answered with.
    pub assessment: AgentAssessment,
}

/// An operator acknowledged an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperatorAcknowledgementPayload {
    /// The event being acknowledged.
    pub acknowledged_event_id: EventId,
    /// What the operator asked for next.
    pub disposition: AcknowledgementDisposition,
}

/// Which event this is, without its payload.
///
/// Every kind here has exactly one [`EventPayload`] variant, and the spellings
/// are the same on the wire.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EventKind {
    /// Obico reported a print failure.
    ObicoFailureAlert,
    /// Obico sent a printer notification.
    ObicoPrinterNotification,
    /// An external body arrived that could not be read.
    MalformedExternalEvent,
    /// An actor asked for an action.
    ActionRequested,
    /// An accepted action reached the printer.
    ActionExecuted,
    /// Policy refused an action.
    ActionRejected,
    /// A bounded intervention expired.
    InterventionExpired,
    /// A supervision session was opened.
    SupervisionSessionOpened,
    /// A supervision session was closed.
    SupervisionSessionClosed,
    /// The agent wrote down what it made of a turn.
    AgentAssessment,
    /// An operator acknowledged an event.
    OperatorAcknowledgement,
}

impl EventKind {
    /// Every kind this vocabulary declares.
    ///
    /// The whole vocabulary, so that a test walking the kinds reads them off
    /// this type rather than off a list of its own.
    pub const ALL: [Self; 11] = [
        Self::ObicoFailureAlert,
        Self::ObicoPrinterNotification,
        Self::MalformedExternalEvent,
        Self::ActionRequested,
        Self::ActionExecuted,
        Self::ActionRejected,
        Self::InterventionExpired,
        Self::SupervisionSessionOpened,
        Self::SupervisionSessionClosed,
        Self::AgentAssessment,
        Self::OperatorAcknowledgement,
    ];
}

/// What an event carries, tagged by the kind it belongs to.
///
/// The wire shape is the pair `kind` and `payload`, which is why an
/// [`EventRecord`] carries this one value rather than two fields that could
/// disagree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum EventPayload {
    /// Obico reported a print failure.
    ObicoFailureAlert(ObicoFailureAlertPayload),
    /// Obico sent a printer notification.
    ObicoPrinterNotification(ObicoPrinterNotificationPayload),
    /// An external body arrived that could not be read.
    MalformedExternalEvent(MalformedExternalEventPayload),
    /// An actor asked for an action.
    ActionRequested(ActionRequestedPayload),
    /// An accepted action reached the printer.
    ActionExecuted(ActionExecutedPayload),
    /// Policy refused an action.
    ActionRejected(ActionRejectedPayload),
    /// A bounded intervention expired.
    InterventionExpired(InterventionExpiredPayload),
    /// A supervision session was opened.
    SupervisionSessionOpened(SupervisionSessionOpenedPayload),
    /// A supervision session was closed.
    SupervisionSessionClosed(SupervisionSessionClosedPayload),
    /// The agent wrote down what it made of a turn.
    AgentAssessment(AgentAssessmentPayload),
    /// An operator acknowledged an event.
    OperatorAcknowledgement(OperatorAcknowledgementPayload),
}

impl EventPayload {
    /// The kind this payload belongs to, and there is exactly one.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::ObicoFailureAlert(_) => EventKind::ObicoFailureAlert,
            Self::ObicoPrinterNotification(_) => EventKind::ObicoPrinterNotification,
            Self::MalformedExternalEvent(_) => EventKind::MalformedExternalEvent,
            Self::ActionRequested(_) => EventKind::ActionRequested,
            Self::ActionExecuted(_) => EventKind::ActionExecuted,
            Self::ActionRejected(_) => EventKind::ActionRejected,
            Self::InterventionExpired(_) => EventKind::InterventionExpired,
            Self::SupervisionSessionOpened(_) => EventKind::SupervisionSessionOpened,
            Self::SupervisionSessionClosed(_) => EventKind::SupervisionSessionClosed,
            Self::AgentAssessment(_) => EventKind::AgentAssessment,
            Self::OperatorAcknowledgement(_) => EventKind::OperatorAcknowledgement,
        }
    }
}

/// One event, as the store holds it.
///
/// `raw` holds the bytes exactly as received for an externally sourced event
/// and is absent for an internally raised one — it is what makes the history
/// auditable when a normalization turns out to be wrong. `print_id` is
/// optional, because an externally sourced event may name no print this system
/// knows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventRecord {
    /// This event's identifier, minted by the store.
    pub id: EventId,
    /// The print it belongs to, when it belongs to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_id: Option<PrintId>,
    /// Where it came from.
    pub source: EventSource,
    /// When it was received.
    pub received_at: Timestamp,
    /// The image it arrived with, when it arrived with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
    /// The kind and the payload, which are one closed pair.
    #[serde(flatten)]
    pub payload: EventPayload,
    /// The bytes exactly as received, for an externally sourced event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawBytes>,
}

impl EventRecord {
    /// Which event this is, read off the payload it carries.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        self.payload.kind()
    }
}
