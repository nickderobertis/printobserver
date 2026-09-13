//! What a supervision turn answers with.
//!
//! [`AgentAssessment`] is the shape [`TurnOutcome`](crate::TurnOutcome)
//! carries: the agent's written record of one turn, produced by the adapter
//! behind this port against the schema this type generates — which is the one
//! artifact the agent's answer may be constrained by. It is declared here,
//! beside the port whose answer carries it, so that what an assessment says
//! is the supervisor port's to change and nothing central's.

use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// How sure the agent is.
///
/// A closed vocabulary rather than a number, because a number invites a
/// precision the agent does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
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
/// `PrintAction` that policy rules on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
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

impl Sample for Confidence {
    fn sample_full() -> Self {
        Self::Medium
    }
}

impl Sample for AgentAssessment {
    fn sample_full() -> Self {
        Self {
            summary: "the first layer is down and adhesion looks even".to_owned(),
            confidence: Confidence::Medium,
            should_continue: true,
            did: "slowed the feedrate to 80% for ten minutes".to_owned(),
            why: "the extrusion width was widening on the long edges".to_owned(),
            escalating: false,
        }
    }
}
