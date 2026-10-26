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
    SpoolRefused {
        /// Why, in a form a client acts on without reading `message`.
        cause: SpoolRefusalCause,
        /// Why, in a sentence naming what to do instead, for a person.
        message: String,
    },
}

/// One field of a spool command, as the command names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum SpoolField {
    /// The operator's own label for the spool.
    Name,
    /// The material, such as `PLA`.
    Material,
    /// The filament's diameter, in millimetres.
    DiameterMm,
    /// The material's density, in grams per cubic centimetre.
    DensityGCm3,
    /// The net filament on the spool when full, in grams.
    InitialG,
    /// The filament on the spool now, in grams.
    RemainingG,
    /// The empty spool's own weight, in grams.
    TareG,
    /// A scale reading of the spool, in grams.
    GrossG,
}

/// Why the spool ledger refused a command, each cause carrying what a client
/// needs to choose the next valid request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "cause",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum SpoolRefusalCause {
    /// No spool was ever registered under this identifier.
    UnknownSpool {
        /// The identifier the command named.
        spool_id: SpoolId,
    },
    /// The spool is retired, and takes no further command.
    RetiredSpool {
        /// The retired spool.
        spool_id: SpoolId,
    },
    /// The spool named is already on the tool, and nothing is pending there or
    /// marked on it for a load to confirm.
    NothingToConfirm {
        /// The tool the spool is already on.
        tool: i64,
    },
    /// A figure the command gave is not one the field can hold: not a number,
    /// or out of the field's range.
    InvalidAdjustmentFigure {
        /// The field the figure was given for.
        field: SpoolField,
        /// The figure given; always a number, because a figure JSON cannot
        /// carry is refused as it would be written rather than written as
        /// `null`.
        #[serde(serialize_with = "a_number")]
        value: f64,
    },
    /// A scale reading was given for a spool with no tare, so it cannot be
    /// turned into a figure.
    TareRequired {
        /// The spool with no tare.
        spool_id: SpoolId,
    },
    /// The material has no known density, so the command needs one.
    DensityRequired {
        /// The spool, when the command was about one that exists; absent for
        /// a registration.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spool_id: Option<SpoolId>,
        /// The material, as given and trimmed.
        material: String,
    },
    /// A field the command needs was empty or absent.
    MissingField {
        /// The field; for a correction naming neither figure, `remaining_g`,
        /// which `gross_g` may stand in for.
        field: SpoolField,
    },
    /// The command gave fields only one of which it may carry.
    ConflictingFields {
        /// Every field given that conflicts, in the command's own order.
        fields: Vec<SpoolField>,
    },
    /// The command named a tool the printer cannot have.
    UnknownTool {
        /// The tool named.
        tool: i64,
    },
    /// The spool is loaded on no tool, so there is nothing to unload.
    NotLoaded {
        /// The spool.
        spool_id: SpoolId,
    },
    /// The ledger refused the write the command made, because the spools
    /// changed under it; reading them again shows how.
    LedgerRefused,
}

/// Write a figure as the number it is, refusing one JSON has no number for:
/// written as `null`, it would be a refusal no reader could read back.
// serde hands a `serialize_with` a reference to the field; suppressions.toml has the reason.
#[expect(clippy::trivially_copy_pass_by_ref)]
fn a_number<S: printobserver_types::serde::Serializer>(
    value: &f64,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use printobserver_types::serde::ser::Error as _;
    if value.is_finite() {
        serializer.serialize_f64(*value)
    } else {
        Err(S::Error::custom(format!(
            "the figure {value} has no number JSON can carry"
        )))
    }
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
