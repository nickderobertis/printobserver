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
//!
//! The printer timeline — every print's start and end, every charge against a
//! spool and every change to the spool ledger — is the sixteen kinds
//! [`TIMELINE_KINDS`] lists. A spool event is written with no print unless it
//! happened during one, in which case it carries that print's.

use crate::records::{
    AcknowledgementDisposition, ActionId, Actor, ChargeKey, ChargeMethod, ClearedMark,
    FilePosition, InterventionId, InterventionOutcome, LoadedSpool, PendingCharge, PendingTaken,
    PolicyDecision, Preflight, PrintAction, PrintOutcome, SpoolCommand, SpoolId, SpoolRecord,
    UnverifiedReason,
};
use printobserver_printer_api::{Adjustable, JobSnapshot, PrinterSnapshot, PrinterState};
use printobserver_supervisor_api::AgentAssessment;
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventId, EventKind, EventPayload, EventSource, Timestamp};

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

/// Somebody took a fresh look at the print.
///
/// Written down before the camera is asked for a frame, so that a frame the
/// camera would not give is recorded against a look that is already in the
/// history; the frame itself, when there is one, is this event's image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct CameraLookPayload {
    /// How long the look waited for something to happen before it was taken,
    /// in whole seconds.
    pub waited_s: u32,
    /// The events for the print that arrived while it waited, and that it
    /// handed to whoever looked, oldest first.
    #[serde(default)]
    pub delivered: Vec<EventId>,
}

impl EventPayload for CameraLookPayload {
    const KIND: &'static str = "camera_look";
}

impl Sample for CameraLookPayload {
    fn sample_full() -> Self {
        Self {
            waited_s: 60,
            delivered: vec![EventId::sample_full()],
        }
    }

    fn sample_minimal() -> Self {
        Self {
            waited_s: 0,
            delivered: Vec::new(),
        }
    }
}

/// The supervisor looked in on an active print on its own schedule, with no
/// detector having said anything about it.
///
/// What a failure detector never alerts on — a progress that stopped moving, a
/// heater whose target fell to nothing mid-print, a fan cut at a bridge, a wall
/// going thin — is still on the machine and in the camera's frame, so every
/// active print is given one of these each interval and a turn on it. It
/// carries what the printer reported as it was taken; the frame the camera gave
/// at the same moment, untagged by any detector, is this event's image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PeriodicObservationPayload {
    /// How often these are taken, in whole seconds.
    pub interval_s: u64,
    /// The printer's state as the observation was taken, absent when it could
    /// not be read — which the history records as a port failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<PrinterSnapshot>,
    /// The job the printer reported as the observation was taken, absent when
    /// it could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobSnapshot>,
}

impl EventPayload for PeriodicObservationPayload {
    const KIND: &'static str = "periodic_observation";
}

impl Sample for PeriodicObservationPayload {
    fn sample_full() -> Self {
        Self {
            interval_s: 120,
            printer: Some(PrinterSnapshot::sample_full()),
            job: Some(JobSnapshot::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            interval_s: 120,
            printer: None,
            job: None,
        }
    }
}

/// Where a port failed while an event was being handled.
///
/// A closed set of exactly the sites at which a failure has nowhere else to be
/// recorded. The printer's action methods record theirs on the
/// [`ActionRecord`](crate::records::ActionRecord) the request minted, and a restoring
/// call records its own on the [`Intervention`](crate::records::Intervention) it was
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
    /// Taking a fresh frame from the camera, for a look at the print or for a
    /// periodic observation of it.
    CameraLook,
    /// Telling the detector that paused the print its detection was handled.
    DetectorAcknowledgement,
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

/// Every kind of the printer timeline: prints starting and ending, what they
/// took from a spool, and every change to the spool ledger.
pub const TIMELINE_KINDS: [&str; 16] = [
    PrintStartedPayload::KIND,
    PrintEndedPayload::KIND,
    FilamentConsumedPayload::KIND,
    SpoolRegisteredPayload::KIND,
    SpoolLoadedPayload::KIND,
    SpoolUnloadedPayload::KIND,
    SpoolAdjustedPayload::KIND,
    SpoolRetiredPayload::KIND,
    SpoolCommandRejectedPayload::KIND,
    SpoolUnverifiedPayload::KIND,
    FilamentChangeSignalledPayload::KIND,
    UnaccountedPrintPayload::KIND,
    PreflightCheckedPayload::KIND,
    ChargePendedPayload::KIND,
    PrintUntrackedPayload::KIND,
    FileTallyRebaselinedPayload::KIND,
];

/// A fixed spool identifier the timeline's samples share.
fn sample_spool_id() -> SpoolId {
    SpoolId::sample_full()
}

/// The reason the timeline's samples of a spool command carry.
fn sample_reason() -> String {
    "the spool on the shelf changed".to_owned()
}

/// A print started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PrintStartedPayload {
    /// The file being printed, when it is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// The action that started it; absent when the print was observed rather
    /// than started through this system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_by: Option<ActionId>,
    /// The spool loaded on each tool as it started.
    pub loaded: Vec<LoadedSpool>,
    /// The filament check made before it started, when one was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight: Option<Preflight>,
}

impl EventPayload for PrintStartedPayload {
    const KIND: &'static str = "print_started";
}

impl Sample for PrintStartedPayload {
    fn sample_full() -> Self {
        Self {
            file_name: Some("benchy.gcode".to_owned()),
            started_by: Some(ActionId::sample_full()),
            loaded: vec![LoadedSpool::sample_full()],
            preflight: Some(Preflight::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            file_name: None,
            started_by: None,
            loaded: Vec::new(),
            preflight: None,
        }
    }
}

/// A print ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PrintEndedPayload {
    /// How it ended.
    pub outcome: PrintOutcome,
    /// The state the printer was in as it ended.
    pub state: PrinterState,
    /// Why it ended, in the words the end was recorded with.
    pub reason: String,
    /// How far into its file it last read, when that was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_position: Option<FilePosition>,
}

impl EventPayload for PrintEndedPayload {
    const KIND: &'static str = "print_ended";
}

impl Sample for PrintEndedPayload {
    fn sample_full() -> Self {
        Self {
            outcome: PrintOutcome::Cancelled,
            state: PrinterState::Cancelling,
            reason: "the operator cancelled it".to_owned(),
            last_position: Some(FilePosition::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            outcome: PrintOutcome::Completed,
            state: PrinterState::Operational,
            reason: "finished".to_owned(),
            last_position: None,
        }
    }
}

/// Filament was charged against a spool, or recorded uncharged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FilamentConsumedPayload {
    /// The durable identity of this debit.
    pub charge_key: ChargeKey,
    /// The tool that fed it.
    pub tool: i64,
    /// The spool charged; absent when nothing was tracked on the tool, and the
    /// filament was recorded uncharged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spool_id: Option<SpoolId>,
    /// How the length was worked out.
    pub method: ChargeMethod,
    /// How much was fed, in millimetres.
    pub length_mm: f64,
    /// What that length weighs, in grams, when a spool gave its diameter and
    /// density.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grams: Option<f64>,
    /// The diameter the grams were worked out with, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter_mm: Option<f64>,
    /// The density the grams were worked out with, in grams per cubic
    /// centimetre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density_g_cm3: Option<f64>,
    /// The file's analysed length for the tool, in millimetres, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysed_length_mm: Option<f64>,
    /// How far into the file the charge runs, when that was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<FilePosition>,
    /// The spool's figure before the charge, in grams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_before_g: Option<f64>,
    /// The spool's figure after it, in grams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_after_g: Option<f64>,
    /// Anything a reader needs to make sense of the figure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl EventPayload for FilamentConsumedPayload {
    const KIND: &'static str = "filament_consumed";
}

impl Sample for FilamentConsumedPayload {
    fn sample_full() -> Self {
        Self {
            charge_key: ChargeKey::sample_full(),
            tool: 0,
            spool_id: Some(sample_spool_id()),
            method: ChargeMethod::GcodeExtrusion,
            length_mm: 1_000.0,
            grams: Some(2.98),
            diameter_mm: Some(1.75),
            density_g_cm3: Some(1.24),
            analysed_length_mm: Some(5_120.0),
            position: Some(FilePosition::sample_full()),
            remaining_before_g: Some(745.48),
            remaining_after_g: Some(742.5),
            note: Some("charged up to the filament change".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            charge_key: ChargeKey::sample_full(),
            tool: 0,
            spool_id: None,
            method: ChargeMethod::SlicerEstimate,
            length_mm: 5_120.0,
            grams: None,
            diameter_mm: None,
            density_g_cm3: None,
            analysed_length_mm: None,
            position: None,
            remaining_before_g: None,
            remaining_after_g: None,
            note: None,
        }
    }
}

/// A spool was registered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolRegisteredPayload {
    /// The spool, as it was registered.
    pub spool: SpoolRecord,
    /// Who registered it.
    pub actor: Actor,
    /// Why.
    pub reason: String,
}

impl EventPayload for SpoolRegisteredPayload {
    const KIND: &'static str = "spool_registered";
}

impl Sample for SpoolRegisteredPayload {
    fn sample_full() -> Self {
        Self {
            spool: SpoolRecord::sample_full(),
            actor: Actor::Operator,
            reason: sample_reason(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            spool: SpoolRecord::sample_minimal(),
            ..Self::sample_full()
        }
    }
}

/// A spool was loaded onto a tool, or confirmed as never having left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolLoadedPayload {
    /// The spool loaded.
    pub spool_id: SpoolId,
    /// The tool, in the printer's own numbering.
    pub tool: i64,
    /// The spool the load swapped off the tool, when it was occupied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced: Option<SpoolId>,
    /// The pending charges the load took, each also its own
    /// `filament_consumed` event under its key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub took_pending: Vec<PendingTaken>,
    /// Every mark the load cleared, on either spool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleared_marks: Vec<ClearedMark>,
    /// Who loaded it.
    pub actor: Actor,
    /// Why.
    pub reason: String,
}

impl EventPayload for SpoolLoadedPayload {
    const KIND: &'static str = "spool_loaded";
}

impl Sample for SpoolLoadedPayload {
    fn sample_full() -> Self {
        Self {
            spool_id: sample_spool_id(),
            tool: 0,
            replaced: Some(ClearedMark::sample_full().spool_id),
            took_pending: vec![PendingTaken::sample_full()],
            cleared_marks: vec![ClearedMark::sample_full()],
            actor: Actor::Operator,
            reason: sample_reason(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            replaced: None,
            took_pending: Vec::new(),
            cleared_marks: Vec::new(),
            ..Self::sample_full()
        }
    }
}

/// A spool was unloaded from its tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolUnloadedPayload {
    /// The spool unloaded.
    pub spool_id: SpoolId,
    /// The tool it left.
    pub tool: i64,
    /// Who unloaded it.
    pub actor: Actor,
    /// Why.
    pub reason: String,
}

impl EventPayload for SpoolUnloadedPayload {
    const KIND: &'static str = "spool_unloaded";
}

impl Sample for SpoolUnloadedPayload {
    fn sample_full() -> Self {
        Self {
            spool_id: sample_spool_id(),
            tool: 0,
            actor: Actor::Operator,
            reason: sample_reason(),
        }
    }
}

/// A spool's remaining figure was corrected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolAdjustedPayload {
    /// The spool corrected.
    pub spool_id: SpoolId,
    /// Its figure before, in grams, after any pending charge it took.
    pub remaining_before_g: f64,
    /// Its figure after, in grams.
    pub remaining_after_g: f64,
    /// The scale reading the figure was worked out from, when it was given as
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gross_g: Option<f64>,
    /// The pending charges the correction took first, each also its own
    /// `filament_consumed` event under its key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub took_pending: Vec<PendingTaken>,
    /// Every mark the correction cleared.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleared_marks: Vec<ClearedMark>,
    /// Who corrected it.
    pub actor: Actor,
    /// Why.
    pub reason: String,
}

impl EventPayload for SpoolAdjustedPayload {
    const KIND: &'static str = "spool_adjusted";
}

impl Sample for SpoolAdjustedPayload {
    fn sample_full() -> Self {
        Self {
            spool_id: sample_spool_id(),
            remaining_before_g: 742.5,
            remaining_after_g: 700.0,
            gross_g: Some(950.0),
            took_pending: vec![PendingTaken::sample_full()],
            cleared_marks: vec![ClearedMark {
                spool_id: sample_spool_id(),
                reason: UnverifiedReason::sample_full(),
            }],
            actor: Actor::Operator,
            reason: sample_reason(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            gross_g: None,
            took_pending: Vec::new(),
            cleared_marks: Vec::new(),
            ..Self::sample_full()
        }
    }
}

/// A spool was retired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolRetiredPayload {
    /// The spool retired.
    pub spool_id: SpoolId,
    /// Its figure as it was retired, in grams.
    pub remaining_g: f64,
    /// Who retired it.
    pub actor: Actor,
    /// Why.
    pub reason: String,
}

impl EventPayload for SpoolRetiredPayload {
    const KIND: &'static str = "spool_retired";
}

impl Sample for SpoolRetiredPayload {
    fn sample_full() -> Self {
        Self {
            spool_id: sample_spool_id(),
            remaining_g: 12.5,
            actor: Actor::Operator,
            reason: "the spool is empty".to_owned(),
        }
    }
}

/// A spool command was refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolCommandRejectedPayload {
    /// The command, as it was asked for.
    pub command: SpoolCommand,
    /// The whole decision, carrying which refusal it was.
    pub decision: PolicyDecision,
}

impl EventPayload for SpoolCommandRejectedPayload {
    const KIND: &'static str = "spool_command_rejected";
}

impl Sample for SpoolCommandRejectedPayload {
    fn sample_full() -> Self {
        Self {
            command: SpoolCommand::sample_full(),
            decision: PolicyDecision::Rejected(crate::records::RejectionReason::SpoolRefused {
                detail: "PEEK has no known density, so the spool needs one".to_owned(),
            }),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            command: SpoolCommand::sample_minimal(),
            ..Self::sample_full()
        }
    }
}

/// A spool was marked as one whose figure is in doubt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolUnverifiedPayload {
    /// The spool marked.
    pub spool_id: SpoolId,
    /// Why its figure is in doubt.
    pub reason: UnverifiedReason,
}

impl EventPayload for SpoolUnverifiedPayload {
    const KIND: &'static str = "spool_unverified";
}

impl Sample for SpoolUnverifiedPayload {
    fn sample_full() -> Self {
        Self {
            spool_id: sample_spool_id(),
            reason: UnverifiedReason::sample_full(),
        }
    }
}

/// A filament change was signalled during a print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FilamentChangeSignalledPayload {
    /// The tool the change was signalled for, when it is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<i64>,
    /// The spool that was on it, when one was tracked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spool_id: Option<SpoolId>,
    /// The kind of the alert that carried the signal.
    pub source_kind: EventKind,
    /// The position the spool was charged up to, when one was known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<FilePosition>,
    /// Whether that position was read when the signal was handled, rather than
    /// being the last one recorded; never with no position at all.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub position_read_at_signal: bool,
}

impl EventPayload for FilamentChangeSignalledPayload {
    const KIND: &'static str = "filament_change_signalled";
}

impl Sample for FilamentChangeSignalledPayload {
    fn sample_full() -> Self {
        Self {
            tool: Some(0),
            spool_id: Some(sample_spool_id()),
            source_kind: EventKind::new("obico_printer_notification").expect("a kind name"),
            position: Some(FilePosition::sample_full()),
            position_read_at_signal: true,
        }
    }

    fn sample_minimal() -> Self {
        Self {
            tool: None,
            spool_id: None,
            position: None,
            position_read_at_signal: false,
            ..Self::sample_full()
        }
    }
}

/// A print this system never saw was found in the printer's own tally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct UnaccountedPrintPayload {
    /// The file that was printed.
    pub file_name: String,
    /// When it was printed, when the printer says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printed_at: Option<Timestamp>,
    /// Whether it completed.
    pub success: bool,
    /// Whether it was charged against a spool.
    pub charged: bool,
}

impl EventPayload for UnaccountedPrintPayload {
    const KIND: &'static str = "unaccounted_print";
}

impl Sample for UnaccountedPrintPayload {
    fn sample_full() -> Self {
        Self {
            file_name: "benchy.gcode".to_owned(),
            printed_at: Some(Timestamp::sample_full()),
            success: true,
            charged: true,
        }
    }

    fn sample_minimal() -> Self {
        Self {
            printed_at: None,
            success: false,
            charged: false,
            ..Self::sample_full()
        }
    }
}

/// The filament check before a start was made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PreflightCheckedPayload {
    /// What the check found.
    pub preflight: Preflight,
    /// The start it was made for, when it was made for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<ActionId>,
}

impl EventPayload for PreflightCheckedPayload {
    const KIND: &'static str = "preflight_checked";
}

impl Sample for PreflightCheckedPayload {
    fn sample_full() -> Self {
        Self {
            preflight: Preflight::sample_full(),
            action_id: Some(ActionId::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            action_id: None,
            ..Self::sample_full()
        }
    }
}

/// Filament a tool fed after an unconfirmed filament change was held for the
/// next spool loaded onto that tool, or confirmed on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ChargePendedPayload {
    /// What was held.
    pub pending: PendingCharge,
}

impl EventPayload for ChargePendedPayload {
    const KIND: &'static str = "charge_pended";
}

impl Sample for ChargePendedPayload {
    fn sample_full() -> Self {
        Self {
            pending: PendingCharge::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            pending: PendingCharge::sample_minimal(),
        }
    }
}

/// A print that was already open when spool tracking was installed: it is
/// never charged, and this says why it carries no charge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PrintUntrackedPayload {
    /// Why the print carries no charge.
    pub reason: String,
}

impl EventPayload for PrintUntrackedPayload {
    const KIND: &'static str = "print_untracked";
}

impl Sample for PrintUntrackedPayload {
    fn sample_full() -> Self {
        Self {
            reason: "the print was already running when spool tracking was installed".to_owned(),
        }
    }
}

/// What became of the prints a file's new tally generation counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum RebaselineDisposition {
    /// They were accounted as prints this system never saw.
    Accounted,
    /// It could not be told which of them this system already settled, so the
    /// loaded spool was marked unverified rather than charged on a guess.
    Unverified,
}

impl Sample for RebaselineDisposition {
    fn sample_full() -> Self {
        Self::Unverified
    }
}

/// The printer's tally for a file went down — the file was deleted and
/// uploaded again — so the file's tally generation advanced, and the new count
/// is that generation's growth from zero.
///
/// The rebaseline itself marks no spool; only an `unverified` disposition does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FileTallyRebaselinedPayload {
    /// The file whose tally went down.
    pub file_name: String,
    /// Where it lives, in the printer's own vocabulary.
    pub origin: String,
    /// The generation the tally was in.
    pub old_generation: u32,
    /// The generation it is in now.
    pub new_generation: u32,
    /// The completed prints the old tally counted.
    pub old_successes: i64,
    /// The failed prints the old tally counted.
    pub old_failures: i64,
    /// The completed prints the new tally counts.
    pub new_successes: i64,
    /// The failed prints the new tally counts.
    pub new_failures: i64,
    /// What became of the prints the new generation counts.
    pub disposition: RebaselineDisposition,
}

impl EventPayload for FileTallyRebaselinedPayload {
    const KIND: &'static str = "file_tally_rebaselined";
}

impl Sample for FileTallyRebaselinedPayload {
    fn sample_full() -> Self {
        Self {
            file_name: "benchy.gcode".to_owned(),
            origin: "local".to_owned(),
            old_generation: 0,
            new_generation: 1,
            old_successes: 5,
            old_failures: 1,
            new_successes: 1,
            new_failures: 0,
            disposition: RebaselineDisposition::Unverified,
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self {
            disposition: RebaselineDisposition::Accounted,
            ..Self::sample_full()
        }]
    }
}
