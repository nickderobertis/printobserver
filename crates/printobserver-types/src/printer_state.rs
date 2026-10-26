//! The state a source reports a printer or a print to be in.
//!
//! Two ports speak it: the printer port's snapshots report it, and the
//! supervisor port's turn situation hands it to the agent. Neither port may
//! depend on the other, so the one vocabulary both read is declared here, and
//! `printobserver-printer-api` re-exports it where its own users name it. The
//! supervision domain's print record carries the same state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::contract::Sample;

/// The state a source reports a printer or a print to be in.
///
/// The `unknown` arm exists so that a state nobody anticipated is recorded
/// carrying the source's own word for it rather than lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PrinterState {
    /// Connected and idle.
    Operational,
    /// Printing, but paused.
    Paused,
    /// Printing.
    Printing,
    /// Cancelling a print.
    Cancelling,
    /// In an error state.
    Error,
    /// Not reachable.
    Offline,
    /// A state this vocabulary does not name, in the source's own word for it.
    Unknown(String),
}

impl Sample for PrinterState {
    fn sample_full() -> Self {
        Self::Printing
    }
}
