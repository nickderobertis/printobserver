//! The supervision session one print is watched through.
//!
//! A session is what [`SupervisorPort::run_turn`](crate::SupervisorPort::run_turn)
//! opens or continues and what a [`TurnOutcome`](crate::TurnOutcome) answers
//! with, so it is declared beside the port whose turns run in it; the
//! supervision domain persists it, and the composition root serves it, but
//! neither adds a word to it.

use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{PrintId, Timestamp};

/// Whether a turn opened a session or continued one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum SessionPhase {
    /// This turn opened the session.
    Created,
    /// This turn continued a session already open.
    Continued,
}

/// The supervision session keyed to one print.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SupervisionSession {
    /// The print this session watches.
    pub print_id: PrintId,
    /// The session's own name in the harness.
    pub session_name: String,
    /// The identity the harness ran the session under.
    pub harness_identity: String,
    /// When the session was opened.
    pub created_at: Timestamp,
    /// When the last turn ran.
    pub last_turn_at: Timestamp,
    /// When the session was closed, if it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Timestamp>,
    /// Why it was closed, if it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,
}

impl Sample for SessionPhase {
    fn sample_full() -> Self {
        Self::Created
    }
}

impl Sample for SupervisionSession {
    fn sample_full() -> Self {
        Self {
            print_id: PrintId::sample_full(),
            session_name: "print-0191f0a0".to_owned(),
            harness_identity: "printobserver-supervisor".to_owned(),
            created_at: Timestamp::sample_full(),
            last_turn_at: later_instant(),
            closed_at: Some(later_instant()),
            close_reason: Some("the print ended".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            closed_at: None,
            close_reason: None,
            ..Self::sample_full()
        }
    }
}

/// A later fixed instant than the one the type crate's sample carries.
fn later_instant() -> Timestamp {
    "2026-03-01T12:30:00Z"
        .parse()
        .expect("a fixed RFC 3339 instant")
}
