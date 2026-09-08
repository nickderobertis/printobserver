//! What an actor is allowed to ask for, and the decision taken on one request.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::action::{ActionKind, ActorClass};
use crate::adjustable::Adjustable;
use crate::printer::PrinterState;
use crate::reported::Range;

/// Server configuration: what any actor may ask for at all.
///
/// This is the operator's safety envelope. It is a different contract from the
/// plausibility ranges [`Reported`](crate::Reported) fields carry, is narrower
/// by orders of magnitude, and the two are never intersected, compared or
/// substituted for one another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetyEnvelope {
    /// The range each adjustable may be set to, inclusive.
    pub allowed: BTreeMap<Adjustable, Range>,
    /// The actions each actor class may request at all.
    pub actions: BTreeMap<ActorClass, Vec<ActionKind>>,
    /// The minimum interval between agent actions, in whole seconds.
    pub agent_min_interval_s: i64,
}

/// The envelope intersected with the print's manifest.
///
/// This is what context reports, so that the agent can see its own limits
/// before it asks. Computing one is the supervision core's; this crate declares
/// the shape and nothing that produces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectiveBounds {
    /// The range each adjustable may be set to, inclusive.
    pub allowed: BTreeMap<Adjustable, Range>,
}

/// Why a request was refused.
///
/// Each rejection is a distinct variant, so a consumer distinguishes them by
/// matching rather than by reading a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
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
}

/// The decision policy took on one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyDecision {
    /// The request may proceed.
    Accepted,
    /// The request may not, for this reason.
    Rejected(RejectionReason),
}
