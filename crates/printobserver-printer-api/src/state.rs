//! The state a source reports a printer or a print to be in.
//!
//! The vocabulary is declared in `printobserver-types`, because the supervisor
//! port hands the same state to the agent and may not depend on this one; it is
//! re-exported here so that everything reading it through this port names it
//! where it always has.

pub use printobserver_types::PrinterState;
