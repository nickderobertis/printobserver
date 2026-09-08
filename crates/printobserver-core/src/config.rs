//! What a running supervisor is configured with.

use std::time::Duration;

use printobserver_types::{PrintId, SafetyEnvelope};

/// The placeholder [`CoreConfig::context_command`] substitutes the print for.
pub const PRINT_ID_PLACEHOLDER: &str = "{print_id}";

/// How often the expiry driver looks for a bounded intervention that is due.
pub const DEFAULT_EXPIRY_POLL: Duration = Duration::from_millis(250);

/// How many of a print's own events a supervision turn is given.
pub const DEFAULT_RECENT_EVENTS: u32 = 20;

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
        }
    }

    /// The context command for one print.
    #[must_use]
    pub fn context_command_for(&self, print_id: PrintId) -> String {
        self.context_command
            .replace(PRINT_ID_PLACEHOLDER, &print_id.to_string())
    }
}
