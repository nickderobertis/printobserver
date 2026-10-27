//! `printobserver-printer-api`.
//!
//! Owns: the port the physical printer speaks through — the trait for reading
//! printer and job state and for asking the machine to do one of the bounded
//! things the action vocabulary names, plus that port's own error type — and
//! the printer domain's vocabulary: the state a printer reports ([`state`],
//! declared in `printobserver-types` and re-exported here), the closed set of
//! things an adjustment may change ([`adjustable`]), the snapshots those reads
//! answer ([`snapshot`]), what the printer knows of the files it holds and the
//! filament they use ([`file`]) and the plausibility ranges their reported
//! numbers are read against ([`ranges`]). A change to what a
//! printer reports edits this crate and the adapters behind it, and nothing
//! central.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
//!
//! # What this port does not expose
//!
//! There is no method that accepts a command string, and that absence is the
//! contract rather than an omission: no method here admits a byte sequence at
//! all, and the only string anywhere in the closure reachable from a method's
//! parameters is the one [`FileName`] wraps, which only
//! [`PrinterPort::start`] and the three reads of one file —
//! [`PrinterPort::file_facts`], [`PrinterPort::extruded_to`] and
//! [`PrinterPort::printed_extent`] — take. Every value a method takes is already
//! bounded by the time it arrives: policy has ruled on it before it gets here.
//!
//! [`PrinterPort::send_command`] is no exception to that: what it takes is a
//! [`PrinterCommand`], a closed vocabulary each adapter renders to its own
//! line, so the set of lines that can reach a machine through it is the set of
//! variants declared here and nothing a caller writes.
//!
//! # What the printer pushes
//!
//! [`PrinterPort::push_events`] opens the printer's own push channel and
//! answers what it pushes ([`push`]): a reset, an error it reported, and each
//! line of its terminal. Those are the facts a poll of the printer's state
//! cannot see — a reset while idle changes nothing a read reports — and the
//! stream ends when the channel drops, so a reader knows when it is reading
//! nothing.
//!
//! # Why the methods answer a boxed future
//!
//! Every method is asynchronous, and the trait is dyn-compatible and shareable
//! across threads, because the supervision core holds every port behind
//! `Arc<dyn Port>`. An `async fn` in a trait is not dyn-compatible, so each
//! method answers a [`BoxFuture`] instead: the same asynchrony, in the one
//! shape a trait object can carry.

pub mod adjustable;
pub mod file;
pub mod push;
pub mod ranges;
pub mod snapshot;
pub mod state;

use core::future::Future;
use core::pin::Pin;

use printobserver_types::FileName;

pub use adjustable::{Adjustable, AdjustableError};
pub use file::{FileFacts, FilePrint, Footprint, PrintedExtent, ToolFilament};
pub use push::{
    AxisPosition, BoxStream, HomeAxes, PrinterCommand, PrinterErrorReason, PrinterPush, Stream,
};
pub use ranges::{
    COMPLETION_RANGE, FAN_PERCENT_RANGE, FEEDRATE_FACTOR_RANGE, FLOWRATE_FACTOR_RANGE,
    HEATER_ACTUAL_C_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE, RANGED_FIELDS,
    RangedField,
};
pub use snapshot::{ErrorSource, HeaterSnapshot, JobError, JobSnapshot, PrinterSnapshot};
pub use state::PrinterState;

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

    /// Read what the printer knows of one file it holds.
    ///
    /// A printer that cannot answer is refused with status 501, which is what
    /// an implementation that does not override this answers.
    fn file_facts(&self, file_name: FileName) -> BoxFuture<'_, Result<FileFacts, PrinterError>> {
        let _ = file_name;
        Box::pin(async { Err(not_supported("file_facts")) })
    }

    /// Read every file the printer holds that carries a print tally.
    ///
    /// A printer that cannot answer is refused with status 501, which is what
    /// an implementation that does not override this answers.
    fn printed_files(&self) -> BoxFuture<'_, Result<Vec<FileFacts>, PrinterError>> {
        Box::pin(async { Err(not_supported("printed_files")) })
    }

    /// Read the filament each tool is fed by one file's own moves, from its
    /// start up to a byte position in it.
    ///
    /// A printer that cannot answer is refused with status 501, which is what
    /// an implementation that does not override this answers.
    fn extruded_to(
        &self,
        file_name: FileName,
        position_bytes: i64,
    ) -> BoxFuture<'_, Result<Vec<ToolFilament>, PrinterError>> {
        let _ = (file_name, position_bytes);
        Box::pin(async { Err(not_supported("extruded_to")) })
    }

    /// Read what one file printed from its start up to a byte position in it:
    /// the highest Z an extruding move reached and the XY rectangle of every
    /// extruding move. Nothing when no move up to the position extruded.
    ///
    /// A printer that cannot answer is refused with status 501, which is what
    /// an implementation that does not override this answers.
    fn printed_extent(
        &self,
        file_name: FileName,
        position_bytes: i64,
    ) -> BoxFuture<'_, Result<Option<PrintedExtent>, PrinterError>> {
        let _ = (file_name, position_bytes);
        Box::pin(async { Err(not_supported("printed_extent")) })
    }

    /// Read where the print head is now, as the printer reports it.
    ///
    /// A position says nothing about whether an axis was homed. A printer that
    /// cannot answer is refused with status 501, which is what an
    /// implementation that does not override this answers.
    fn read_position(&self) -> BoxFuture<'_, Result<AxisPosition, PrinterError>> {
        Box::pin(async { Err(not_supported("read_position")) })
    }

    /// Open the printer's push channel, answering what it pushes from now on.
    ///
    /// The stream ends when the channel drops; opening it again is how a
    /// reader reconnects. A printer that cannot push is refused with status
    /// 501, which is what an implementation that does not override this
    /// answers.
    fn push_events(&self) -> BoxFuture<'_, Result<BoxStream<'static, PrinterPush>, PrinterError>> {
        Box::pin(async { Err(not_supported("push_events")) })
    }

    /// Send one command of the closed vocabulary [`PrinterCommand`] names,
    /// rendered by the implementation to its own line.
    ///
    /// What the printer answers arrives, when it answers anything, on the push
    /// channel. A printer that cannot take one is refused with status 501,
    /// which is what an implementation that does not override this answers.
    fn send_command(&self, command: PrinterCommand) -> BoxFuture<'_, Result<(), PrinterError>> {
        let _ = command;
        Box::pin(async { Err(not_supported("send_command")) })
    }
}

/// What a printer that does not implement one of the optional methods answers.
fn not_supported(method: &str) -> PrinterError {
    PrinterError::Refused {
        status: 501,
        detail: format!("{method} is not supported by this printer"),
    }
}
