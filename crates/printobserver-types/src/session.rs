//! The supervision session one print is watched through.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::PrintId;
use crate::timestamp::Timestamp;

/// Whether a turn opened a session or continued one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionPhase {
    /// This turn opened the session.
    Created,
    /// This turn continued a session already open.
    Continued,
}

/// The supervision session keyed to one print.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
