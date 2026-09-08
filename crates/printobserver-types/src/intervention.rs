//! A bounded change, and what became of it when it expired.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::adjustable::Adjustable;
use crate::ids::{ActionId, InterventionId, PrintId};
use crate::timestamp::Timestamp;

/// What became of a bounded change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InterventionOutcome {
    /// It is still in force.
    StillActive,
    /// The prior value was put back.
    Restored,
    /// There was no prior value to put back.
    RestoreUnavailable,
    /// Putting the prior value back failed.
    RestoreFailed {
        /// Why it failed.
        reason: String,
    },
    /// Another intervention replaced it before it expired.
    Superseded {
        /// The intervention that replaced it.
        by: InterventionId,
    },
}

/// One adjustment made for a bounded time.
///
/// `prior_value` is read from the printer snapshot taken before the change, and
/// is absent when the printer reported none — in which case expiry restores
/// nothing and the outcome says so rather than guessing a default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Intervention {
    /// This intervention's identifier, minted by the store.
    pub id: InterventionId,
    /// The print it was made against.
    pub print_id: PrintId,
    /// The action that asked for it.
    pub action_id: ActionId,
    /// What it changed.
    pub adjustable: Adjustable,
    /// What the printer reported before the change, if it reported anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_value: Option<f64>,
    /// What it was changed to.
    pub applied_value: f64,
    /// When it was applied.
    pub applied_at: Timestamp,
    /// When it stops standing.
    pub expires_at: Timestamp,
    /// When the prior value was put back, if it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_at: Option<Timestamp>,
    /// What became of it.
    pub outcome: InterventionOutcome,
}
