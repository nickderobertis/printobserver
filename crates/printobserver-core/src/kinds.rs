//! The event kinds and source names the supervision domain writes.
//!
//! Each kind here is one the supervision loop, the action chokepoint or the
//! expiry sweep raises itself, declared under a [`KIND`](EventPayload::KIND)
//! of its own and written down through the envelope
//! [`printobserver_types::event`] declares. Nothing outside this crate names
//! these kinds to write them; a reader elsewhere reads one back through
//! [`EventRecord::payload_as`](printobserver_types::EventRecord::payload_as)
//! and is answered nothing for a record of another kind.
//!
//! The three source names are the actors this domain writes events on behalf
//! of: the supervisor itself, a person, and the supervising agent.

use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{
    AcknowledgementDisposition, ActionId, Actor, Adjustable, AgentAssessment, EventId,
    EventPayload, EventSource, InterventionId, InterventionOutcome, PolicyDecision, PrintAction,
};

/// The source name of an event the supervisor raised itself.
pub const SYSTEM_SOURCE: &str = "system";

/// The source name of an event a person raised.
pub const OPERATOR_SOURCE: &str = "operator";

/// The source name of an event the supervising agent raised.
pub const AGENT_SOURCE: &str = "agent";

/// The source of an event the supervisor raised itself.
#[must_use]
pub fn system_source() -> EventSource {
    EventSource::new(SYSTEM_SOURCE)
}

/// The source of an event a person raised.
#[must_use]
pub fn operator_source() -> EventSource {
    EventSource::new(OPERATOR_SOURCE)
}

/// The source of an event the supervising agent raised.
#[must_use]
pub fn agent_source() -> EventSource {
    EventSource::new(AGENT_SOURCE)
}

/// An actor asked for an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ActionRequestedPayload {
    /// The action's identifier.
    pub action_id: ActionId,
    /// What was asked for.
    pub action: PrintAction,
    /// Who asked.
    pub actor: Actor,
}

impl EventPayload for ActionRequestedPayload {
    const KIND: &'static str = "action_requested";
}

impl Sample for ActionRequestedPayload {
    fn sample_full() -> Self {
        Self {
            action_id: ActionId::sample_full(),
            action: PrintAction::sample_full(),
            actor: Actor::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            action: PrintAction::sample_minimal(),
            ..Self::sample_full()
        }
    }
}

/// An accepted action reached the printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ActionExecutedPayload {
    /// The action's identifier.
    pub action_id: ActionId,
    /// The bounded intervention it opened, when it opened one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intervention_id: Option<InterventionId>,
}

impl EventPayload for ActionExecutedPayload {
    const KIND: &'static str = "action_executed";
}

impl Sample for ActionExecutedPayload {
    fn sample_full() -> Self {
        Self {
            action_id: ActionId::sample_full(),
            intervention_id: Some(InterventionId::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            action_id: ActionId::sample_full(),
            intervention_id: None,
        }
    }
}

/// Policy refused an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ActionRejectedPayload {
    /// The action's identifier.
    pub action_id: ActionId,
    /// The whole decision, carrying which rejection it was.
    pub decision: PolicyDecision,
}

impl EventPayload for ActionRejectedPayload {
    const KIND: &'static str = "action_rejected";
}

impl Sample for ActionRejectedPayload {
    fn sample_full() -> Self {
        Self {
            action_id: ActionId::sample_full(),
            decision: PolicyDecision::sample_full(),
        }
    }
}

/// A bounded intervention expired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct InterventionExpiredPayload {
    /// The intervention's identifier.
    pub intervention_id: InterventionId,
    /// What it had changed.
    pub adjustable: Adjustable,
    /// What became of it.
    pub outcome: InterventionOutcome,
}

impl EventPayload for InterventionExpiredPayload {
    const KIND: &'static str = "intervention_expired";
}

impl Sample for InterventionExpiredPayload {
    fn sample_full() -> Self {
        Self {
            intervention_id: InterventionId::sample_full(),
            adjustable: Adjustable::Feedrate,
            outcome: InterventionOutcome::Restored,
        }
    }
}

/// The agent wrote down what it made of a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentAssessmentPayload {
    /// The session the turn ran in.
    pub session_name: String,
    /// What the agent answered with.
    pub assessment: AgentAssessment,
}

impl EventPayload for AgentAssessmentPayload {
    const KIND: &'static str = "agent_assessment";
}

impl Sample for AgentAssessmentPayload {
    fn sample_full() -> Self {
        Self {
            session_name: "print-0191f0a0".to_owned(),
            assessment: AgentAssessment::sample_full(),
        }
    }
}

/// An operator acknowledged an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct OperatorAcknowledgementPayload {
    /// The event being acknowledged.
    pub acknowledged_event_id: EventId,
    /// What the operator asked for next.
    pub disposition: AcknowledgementDisposition,
}

impl EventPayload for OperatorAcknowledgementPayload {
    const KIND: &'static str = "operator_acknowledgement";
}

impl Sample for OperatorAcknowledgementPayload {
    fn sample_full() -> Self {
        Self {
            acknowledged_event_id: EventId::sample_full(),
            disposition: AcknowledgementDisposition::Watch,
        }
    }
}

/// Where a port failed while an event was being handled.
///
/// A closed set of exactly the sites at which a failure has nowhere else to be
/// recorded. The printer's action methods record theirs on the
/// [`ActionRecord`](printobserver_types::ActionRecord) the request minted, and a restoring
/// call records its own on the [`Intervention`](printobserver_types::Intervention) it was
/// expiring; those are not sites here, because a second record of them would be
/// a second version of one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PortFailureSite {
    /// Reading the printer's own state.
    PrinterSnapshot,
    /// Reading the job the printer reports it is running.
    PrinterJob,
    /// Writing the image the event arrived with.
    ImageWrite,
    /// Running the supervision turn the event prompted.
    SupervisionTurn,
}

impl Sample for PortFailureSite {
    fn sample_full() -> Self {
        Self::PrinterSnapshot
    }
}

/// A port failed while one event was being handled.
///
/// The event is named rather than implied, so that a reader holding an event's
/// identifier reaches every failure recorded while that event was being
/// handled. A failure recorded here is one the handling survived: the event is
/// already in the history by the time any of these sites is reached, and the
/// loop goes on to handle the next event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PortFailurePayload {
    /// The event whose handling reached the failing call.
    pub event_id: EventId,
    /// Where it failed.
    pub site: PortFailureSite,
    /// What the port said about it, in the port's own words.
    pub detail: String,
}

impl EventPayload for PortFailurePayload {
    const KIND: &'static str = "port_failure";
}

impl Sample for PortFailurePayload {
    fn sample_full() -> Self {
        Self {
            event_id: EventId::sample_full(),
            site: PortFailureSite::sample_full(),
            detail: "the printer is unreachable: connection refused".to_owned(),
        }
    }
}
