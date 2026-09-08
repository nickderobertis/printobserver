//! The print itself: `PrintObserver`'s own record of one job being watched.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::adjustable::Adjustable;
use crate::ids::PrintId;
use crate::printer::PrinterState;
use crate::reported::Range;
use crate::timestamp::Timestamp;

/// One adjustable whose manifest range was wider than the envelope's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManifestNarrowing {
    /// The adjustable that was narrowed.
    pub adjustable: Adjustable,
    /// The range the manifest asked for.
    pub requested: Range,
    /// The range that stands.
    pub applied: Range,
}

/// One print, and the record supervision keys from.
///
/// Obico's own print id is carried beside this record's identifier rather than
/// as it, because a print may be observed before Obico has one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrintRecord {
    /// This print's identifier, minted by the store.
    pub id: PrintId,
    /// Obico's own identifier for the print, when Obico has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obico_print_id: Option<i64>,
    /// The file being printed, as the source reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// The state the print is in.
    pub state: PrinterState,
    /// When the print was opened.
    pub opened_at: Timestamp,
    /// When it ended, if it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
    /// Why it ended, if it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    /// Every manifest range this print narrowed to the envelope's.
    pub narrowings: Vec<ManifestNarrowing>,
}
