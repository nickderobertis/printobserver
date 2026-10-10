//! What a print took from a spool, and the durable identity of each debit.
//!
//! A charge is keyed rather than counted: every debit carries the
//! [`ChargeKey`] naming exactly which stretch of which print on which tool it
//! is for, so a replay of the same observation finds the key already charged
//! and debits nothing twice. Filament a tool fed after a filament change nobody
//! confirmed is held as a [`PendingCharge`] until somebody says which spool fed
//! it, so a spool that really was fed is never left undebited.

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{PrintId, Timestamp};

/// How a print ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PrintOutcome {
    /// It ran to its end.
    Completed,
    /// Somebody cancelled it.
    Cancelled,
    /// It stopped on an error.
    Failed,
}

/// How the length a charge debits was worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum ChargeMethod {
    /// A completed print: the length the file's analysis gives.
    SlicerEstimate,
    /// The extrusion the file's own moves add up to, up to a position in it.
    GcodeExtrusion,
    /// The analysed length scaled by how far into the file the print got; the
    /// fallback used only when the file itself could not be read.
    FileFraction,
    /// A completed print this system never saw: the file's analysed length.
    UnobservedEstimate,
}

/// The durable identity of one debit, so that a replay can never debit twice.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "key",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum ChargeKey {
    /// One segment of a print this system watched, on one tool.
    Print {
        /// The print.
        print_id: PrintId,
        /// The tool, in the printer's own numbering.
        tool: i64,
        /// Which segment: zero from the print's start, and one more each time
        /// a mid-print swap or a filament-change signal closes the current one.
        segment: u32,
    },
    /// One print this system never saw, on one tool.
    Unobserved {
        /// Where the file lives, in the printer's own vocabulary.
        origin: String,
        /// The file that was printed.
        file_name: String,
        /// The file's tally generation, which advances each time the printer's
        /// tally for the file is reset.
        generation: u32,
        /// The print's ordinal in the file's tally — completed and failed
        /// prints counted up to and including this one — within that
        /// generation.
        print_number: i64,
        /// The tool, in the printer's own numbering.
        tool: i64,
    },
}

impl ChargeKey {
    /// The one spelling this key is stored under.
    ///
    /// `print:<print_id>:<tool>:<segment>` for a watched print and
    /// `unobserved:<origin>:<generation>:<print_number>:<tool>:<file_name>`
    /// for one this system never saw — the file name last, because it is the
    /// one part that may itself carry a colon.
    #[must_use]
    pub fn canonical(&self) -> String {
        match self {
            Self::Print {
                print_id,
                tool,
                segment,
            } => format!("print:{print_id}:{tool}:{segment}"),
            Self::Unobserved {
                origin,
                file_name,
                generation,
                print_number,
                tool,
            } => format!("unobserved:{origin}:{generation}:{print_number}:{tool}:{file_name}"),
        }
    }
}

/// How far into its file a print had got, and when that was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FilePosition {
    /// The byte the printer had read the file up to.
    pub bytes: i64,
    /// The file's size in bytes, when it was known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    /// When the position was read.
    pub observed_at: Timestamp,
}

/// Filament a tool fed after a filament change nobody has confirmed, held until
/// somebody says which spool fed it.
///
/// A length rather than grams, because the grams follow the spool that takes
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PendingCharge {
    /// The key of the print segment the filament was fed in.
    pub charge_key: ChargeKey,
    /// The print it was fed in.
    pub print_id: PrintId,
    /// The tool that fed it.
    pub tool: i64,
    /// How much was fed, in millimetres.
    pub length_mm: f64,
    /// How that length was worked out.
    pub method: ChargeMethod,
    /// The file's analysed length for the tool, in millimetres, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysed_length_mm: Option<f64>,
    /// How far into the file the segment ran, when that was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<FilePosition>,
    /// Anything a reader needs to make sense of the figure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When it was held.
    pub pended_at: Timestamp,
}

/// One pending charge a spool command took.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PendingTaken {
    /// The key it is charged under.
    pub charge_key: ChargeKey,
    /// The print it was fed in.
    pub print_id: PrintId,
}
