//! `printobserver-printer-api`.
//!
//! Owns: the port the physical printer speaks through — the trait for reading
//! printer and job state and for asking the machine to do one of the bounded
//! things the action vocabulary names, plus that port's own error type.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
//!
//! # What this port does not expose
//!
//! There is no method that accepts a command string, and that absence is the
//! contract rather than an omission: no method here admits a byte sequence at
//! all, and the only string anywhere in the closure reachable from a method's
//! parameters is the one [`FileName`] wraps, which exactly one method —
//! [`PrinterPort::start`] — takes. Every value a method takes is already
//! bounded by the time it arrives: policy has ruled on it before it gets here.
//!
//! # Why the methods answer a boxed future
//!
//! Every method is asynchronous, and the trait is dyn-compatible and shareable
//! across threads, because the supervision core holds all four ports behind
//! `Arc<dyn Port>`. An `async fn` in a trait is not dyn-compatible, so each
//! method answers a [`BoxFuture`] instead: the same asynchrony, in the one
//! shape a trait object can carry.

use core::future::Future;
use core::pin::Pin;

use printobserver_types::{Adjustable, FileName, JobSnapshot, PrinterSnapshot};

/// A future this port's methods answer with, in the one shape a trait object
/// can carry.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why the printer did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrinterError {
    /// The printer could not be reached at all.
    Unreachable {
        /// What went wrong reaching it.
        detail: String,
    },
    /// The printer refused the credentials.
    Unauthorized {
        /// What the printer said about them.
        detail: String,
    },
    /// The printer refused the request, in its own words.
    Refused {
        /// The status the source answered with.
        status: u16,
        /// What the source said.
        detail: String,
    },
    /// The printer is not in a state this request is valid from.
    StateConflict {
        /// What the source said about the state.
        detail: String,
    },
    /// This printer cannot express that adjustable at all.
    Unsupported {
        /// The adjustable this printer does not have.
        adjustable: Adjustable,
    },
    /// The printer answered something this port could not read.
    Malformed {
        /// What could not be read.
        detail: String,
    },
}

impl core::fmt::Display for PrinterError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreachable { detail } => {
                write!(formatter, "the printer is unreachable: {detail}")
            }
            Self::Unauthorized { detail } => {
                write!(formatter, "the printer refused the credentials: {detail}")
            }
            Self::Refused { status, detail } => {
                write!(
                    formatter,
                    "the printer refused the request with {status}: {detail}"
                )
            }
            Self::StateConflict { detail } => {
                write!(
                    formatter,
                    "the printer is not in a state this is valid from: {detail}"
                )
            }
            Self::Unsupported { adjustable } => {
                write!(formatter, "this printer has no {adjustable}")
            }
            Self::Malformed { detail } => {
                write!(
                    formatter,
                    "the printer answered something unreadable: {detail}"
                )
            }
        }
    }
}

impl core::error::Error for PrinterError {}

/// The port the physical printer speaks through.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because the supervision core holds it behind
/// `Arc<dyn PrinterPort>`.
pub trait PrinterPort: Send + Sync {
    /// Read the printer's current state.
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>>;

    /// Read the job the printer reports it is running.
    fn job(&self) -> BoxFuture<'_, Result<JobSnapshot, PrinterError>>;

    /// Start a print of a named file.
    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Pause the print.
    fn pause(&self) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Resume the print.
    fn resume(&self) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Cancel the print.
    fn cancel(&self) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Set the feedrate multiplier, where one means one hundred percent.
    fn set_feedrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Set the flowrate multiplier, where one means one hundred percent.
    fn set_flowrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Set one tool's target temperature, in degrees Celsius.
    fn set_tool_target_c(
        &self,
        tool: i64,
        target_c: f64,
    ) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Set the bed's target temperature, in degrees Celsius.
    fn set_bed_target_c(&self, target_c: f64) -> BoxFuture<'_, Result<(), PrinterError>>;

    /// Set the part-cooling fan, in percent.
    fn set_fan_percent(&self, percent: f64) -> BoxFuture<'_, Result<(), PrinterError>>;
}
