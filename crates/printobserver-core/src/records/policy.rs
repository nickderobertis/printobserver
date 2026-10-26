//! What an actor is allowed to ask for, and the decision taken on one request.

use std::collections::BTreeMap;

use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_types::Range;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

use super::action::{ActionKind, ActorClass};
use super::ids::{InterventionId, SpoolId};
use super::spool::UnverifiedReason;

/// Server configuration: what any actor may ask for at all.
///
/// This is the operator's safety envelope. It is a different contract from the
/// plausibility ranges [`Reported`](printobserver_types::Reported) fields carry, is narrower
/// by orders of magnitude, and the two are never intersected, compared or
/// substituted for one another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SafetyEnvelope {
    /// The range each adjustable may be set to, inclusive.
    pub allowed: BTreeMap<Adjustable, Range>,
    /// The actions each actor class may request at all.
    pub actions: BTreeMap<ActorClass, Vec<ActionKind>>,
    /// The minimum interval between two agent actions that change the machine,
    /// in whole seconds. An acknowledgement neither waits on it nor restarts it.
    pub agent_min_interval_s: i64,
}

/// The envelope intersected with the print's manifest.
///
/// This is what context reports, so that the agent can see its own limits
/// before it asks. Computing one is the supervision core's; this crate declares
/// the shape and nothing that produces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct EffectiveBounds {
    /// The range each adjustable may be set to, inclusive.
    pub allowed: BTreeMap<Adjustable, Range>,
}

/// Why a request was refused.
///
/// Each rejection is a distinct variant, so a consumer distinguishes them by
/// matching rather than by reading a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum RejectionReason {
    /// The value asked for is outside the range allowed for that adjustable.
    OutOfBounds {
        /// The adjustable that was asked for.
        adjustable: Adjustable,
        /// The value that was asked for.
        requested: f64,
        /// The range that was allowed.
        allowed: Range,
    },
    /// This actor class may not request this action at all.
    ActorMayNotRequest {
        /// The class of the actor that asked.
        actor_class: ActorClass,
        /// The action they asked for.
        action: ActionKind,
    },
    /// There is no active print to act on.
    NoActivePrint,
    /// The printer is not in a state this action is valid from.
    InvalidFromState {
        /// The state the printer is in.
        state: PrinterState,
    },
    /// The agent's minimum interval has not elapsed.
    MinIntervalNotElapsed {
        /// The minimum interval, in whole seconds.
        interval_s: i64,
        /// How long it has been since the last agent action, in whole seconds.
        since_last_s: i64,
    },
    /// The adjustable is not one this printer has.
    UnsupportedAdjustable {
        /// The adjustable this printer cannot express.
        adjustable: Adjustable,
    },
    /// A start was refused because the spool on one tool holds less than the
    /// print needs and its buffer.
    InsufficientFilament {
        /// The tool whose spool is short.
        tool: i64,
        /// The spool loaded on it.
        spool_id: SpoolId,
        /// That spool's name.
        spool_name: String,
        /// What the print needs from it, in grams.
        required_g: f64,
        /// The margin asked for beyond that, in grams.
        buffer_g: f64,
        /// What the spool holds, in grams.
        remaining_g: f64,
        /// The spool's oldest mark, when the short spool is also unverified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unverified: Option<UnverifiedReason>,
    },
    /// A start was refused because the spool on one tool is unverified.
    UnverifiedSpool {
        /// The tool whose spool is unverified.
        tool: i64,
        /// The spool loaded on it.
        spool_id: SpoolId,
        /// That spool's name.
        spool_name: String,
        /// The spool's oldest mark.
        reason: UnverifiedReason,
    },
    /// A start was refused because `OctoPrint`'s analysis of the file had not
    /// appeared when the start's wait for it elapsed.
    AnalysisUnavailable {
        /// The file that has no analysis.
        file_name: String,
        /// How long the start waited for one, in whole seconds.
        waited_s: u64,
    },
    /// A close was refused because the print record has already ended.
    PrintAlreadyEnded,
    /// A close was refused because the printer shows a job of the record's
    /// file printing, paused or cancelling.
    PrintStillActive {
        /// The state the printer reported that job in.
        state: PrinterState,
    },
    /// A close was refused because an intervention for the print has neither
    /// expired nor been restored.
    InterventionStillActive {
        /// The intervention still standing.
        intervention_id: InterventionId,
    },
    /// A spool command could not be carried out.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    SpoolRefused {
        /// Why, in a sentence naming what to do instead.
        detail: String,
    },
}

/// The decision policy took on one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PolicyDecision {
    /// The request may proceed.
    Accepted,
    /// The request may not, for this reason.
    Rejected(RejectionReason),
}
