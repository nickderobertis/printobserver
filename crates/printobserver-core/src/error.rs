//! The one error type core answers its callers with.

use printobserver_printer_api::PrinterError;
use printobserver_store_api::StoreError;
use printobserver_supervisor_api::SupervisorError;
use printobserver_types::PrintId;
use printobserver_vision_api::VisionError;

/// Why core could not do what it was asked.
///
/// A port's own error is carried rather than flattened into a message, because
/// a caller that has somewhere to put a failure needs to know which port failed
/// and how — the store's initial event append in particular, which is the one
/// failure core cannot record anywhere and so answers to its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// The store failed, in its own words.
    Store(StoreError),
    /// The printer failed, in its own words.
    Printer(PrinterError),
    /// The vision port failed, in its own words.
    Vision(VisionError),
    /// The supervising agent's harness failed, in its own words.
    Supervisor(SupervisorError),
    /// There is no print by that identifier.
    NoSuchPrint {
        /// The identifier nothing was found for.
        print_id: PrintId,
    },
    /// An instant this crate computed is not representable.
    Unrepresentable {
        /// What could not be represented.
        detail: String,
    },
}

impl core::fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "the store failed: {error}"),
            Self::Printer(error) => write!(formatter, "the printer failed: {error}"),
            Self::Vision(error) => write!(formatter, "the vision port failed: {error}"),
            Self::Supervisor(error) => write!(formatter, "the supervisor failed: {error}"),
            Self::NoSuchPrint { print_id } => write!(formatter, "there is no print {print_id}"),
            Self::Unrepresentable { detail } => {
                write!(formatter, "an instant is not representable: {detail}")
            }
        }
    }
}

impl core::error::Error for CoreError {}

impl From<StoreError> for CoreError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
