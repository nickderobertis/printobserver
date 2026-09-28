//! What a running supervisor is configured with.

use std::time::Duration;

use crate::records::SafetyEnvelope;
use printobserver_types::PrintId;

/// The placeholder [`CoreConfig::context_command`] substitutes the print for.
pub const PRINT_ID_PLACEHOLDER: &str = "{print_id}";

/// How often the expiry driver looks for a bounded intervention that is due.
pub const DEFAULT_EXPIRY_POLL: Duration = Duration::from_millis(250);

/// How many of a print's own events a supervision turn is given.
pub const DEFAULT_RECENT_EVENTS: u32 = 20;

/// How long after the agent's last adjustment to a print the detector paused
/// that print is resumed, when its turn has not ended first.
///
/// Long enough for an agent to ask for the two or three changes it decided on
/// one after the other, so that they reach the print together as it moves
/// again; short enough that a paused hot end is not left sitting.
pub const DEFAULT_DETECTOR_RESUME_GRACE: Duration = Duration::from_secs(20);

/// The longest one look may wait for something to happen, in seconds.
///
/// Below the two minutes a supervising agent's own shell gives one command, so
/// that a look is never cut off by the tool that asked for it.
pub const MAX_LOOK_WAIT_S: u32 = 90;

/// The server configuration supervision reads.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreConfig {
    /// The operator's safety envelope: what any actor may ask for at all.
    pub envelope: SafetyEnvelope,
    /// The command a supervision turn runs to read the print's context.
    ///
    /// [`PRINT_ID_PLACEHOLDER`] in it is replaced with the print's own
    /// identifier, so that a turn reads the context of the print it is about.
    pub context_command: String,
    /// How many of the print's own events a turn is given, newest first.
    pub recent_events: u32,
    /// How often the expiry driver looks for an intervention that is due.
    pub expiry_poll: Duration,
    /// Where a fresh frame of the print is fetched from, when a camera is
    /// configured.
    pub camera_snapshot_url: Option<String>,
    /// How long after the agent's last adjustment a print the detector paused
    /// is resumed.
    pub detector_resume_grace: Duration,
}

impl CoreConfig {
    /// The configuration for one envelope, with this crate's own defaults.
    #[must_use]
    pub fn new(envelope: SafetyEnvelope, context_command: String) -> Self {
        Self {
            envelope,
            context_command,
            recent_events: DEFAULT_RECENT_EVENTS,
            expiry_poll: DEFAULT_EXPIRY_POLL,
            camera_snapshot_url: None,
            detector_resume_grace: DEFAULT_DETECTOR_RESUME_GRACE,
        }
    }

    /// The context command for one print.
    #[must_use]
    pub fn context_command_for(&self, print_id: PrintId) -> String {
        self.context_command
            .replace(PRINT_ID_PLACEHOLDER, &print_id.to_string())
    }
}
