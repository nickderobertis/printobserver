//! The state a source reports a printer or a print to be in.
//!
//! The snapshots this port answers report it, and the supervision domain's
//! print record carries the state its print is in; both are read against the
//! one vocabulary declared here, in the domain whose machine reports it.

use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// The state a source reports a printer or a print to be in.
///
/// The `unknown` arm exists so that a state nobody anticipated is recorded
/// carrying the source's own word for it rather than lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
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
