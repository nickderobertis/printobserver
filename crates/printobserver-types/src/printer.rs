//! The state a source reports a printer or a print to be in.
//!
//! [`PrinterState`] is the one printer-domain word the whole workspace still
//! has to agree on: the print record and the store's trait name it, so it
//! stays here until the step that moves those. The snapshots that carry it are
//! the printer port's own.

use serde::{Deserialize, Serialize};

use schemars::JsonSchema;

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
