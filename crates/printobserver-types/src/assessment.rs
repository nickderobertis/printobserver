//! What a supervision turn answers with.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How sure the agent is.
///
/// A closed vocabulary rather than a number, because a number invites a
/// precision the agent does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Confidence {
    /// Not sure.
    Low,
    /// Fairly sure.
    Medium,
    /// Sure.
    High,
}

/// The agent's written record of one supervision turn.
///
/// This is deliberately not how the agent acts: acting is a
/// [`PrintAction`](crate::PrintAction) that policy rules on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentAssessment {
    /// One line saying what is happening.
    pub summary: String,
    /// How sure the agent is.
    pub confidence: Confidence,
    /// Whether the print should carry on.
    pub should_continue: bool,
    /// What the agent did.
    pub did: String,
    /// Why it did it.
    pub why: String,
    /// Whether the agent is escalating to a person.
    pub escalating: bool,
}
