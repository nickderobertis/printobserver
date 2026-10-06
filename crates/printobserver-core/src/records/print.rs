//! The print itself: `PrintObserver`'s own record of one job being watched.

use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{PrintId, Range, Timestamp};

/// One adjustable whose manifest range was wider than the envelope's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
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
/// The provider's own print id is carried beside this record's identifier
/// rather than as it, because a print may be observed before the provider
/// that reports it has one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PrintRecord {
    /// This print's identifier, minted by the store.
    pub id: PrintId,
    /// The provider's own identifier for the print, whichever provider
    /// reported it, when one has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_print_id: Option<i64>,
    /// The file being printed, as the source reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// The state the print is in.
    pub state: PrinterState,
    /// When the print was opened.
    pub opened_at: Timestamp,
    /// When the printer's job this print records began, as the printer's own
    /// running time put it at the latest read that found the job running or
    /// paused: the instant of that read, less the running time it reported.
    ///
    /// `OctoPrint` counts a pause into the running time until the job resumes
    /// and then takes the pause back out, so a pause moves this later: a read
    /// putting it further than `listing::JOB_IDENTITY_TOLERANCE_S` from the one
    /// recorded resets it, rather than it being fixed once. Absent when no read has
    /// found this print's job reporting a running time — a print opened by an
    /// alert or a start before any read, and every print recorded before this
    /// was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_started_at: Option<Timestamp>,
    /// The longest running time, in whole seconds, the printer reported for
    /// this print's job at a read that found it printing rather than paused.
    ///
    /// The time a job has spent printing only grows, pause or no pause, so a
    /// job of this print's file reporting less — by more than
    /// `listing::JOB_IDENTITY_TOLERANCE_S` — is a later job. Absent until a
    /// read finds the job printing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_print_time_s: Option<i64>,
    /// When it ended, if it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
    /// Why it ended, if it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    /// Every manifest range this print narrowed to the envelope's.
    pub narrowings: Vec<ManifestNarrowing>,
}
