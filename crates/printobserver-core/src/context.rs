//! The one aggregate a caller and the agent both read.
//!
//! [`PrintContext`] is the supervision domain's own view of one print: the
//! record, the printer and job as the printer port last reported them, the
//! manifest and the bounds in force, the interventions still active and the
//! events most recently written down. The loop collects it when it handles an
//! event, holds it for the turn that event prompts, and answers it to the
//! `context` operation, so what the agent reasons about is the state the event
//! was handled at rather than whatever the machine has drifted to since.
//!
//! It is declared here rather than centrally because every field of it is
//! this domain's to collect: a domain that adds a fact a turn should see edits
//! this crate and regenerates the clients, and nothing central.

use printobserver_printer_api::{JobSnapshot, PrinterSnapshot};
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{
    EffectiveBounds, EventRecord, ImageRef, Intervention, JobManifest, PrintRecord,
};

/// Everything a supervision turn is given about one print.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PrintContext {
    /// The print itself.
    pub print: PrintRecord,
    /// The printer, when a snapshot could be taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<PrinterSnapshot>,
    /// The job, when a snapshot could be taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobSnapshot>,
    /// The manifest, when the print has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<JobManifest>,
    /// The limits in force, so that the agent can see them before it asks.
    pub bounds: EffectiveBounds,
    /// The interventions still active.
    pub interventions: Vec<Intervention>,
    /// A bounded list of this print's events, newest first.
    pub recent_events: Vec<EventRecord>,
    /// The most recent image, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_image: Option<ImageRef>,
}

impl Sample for PrintContext {
    fn sample_full() -> Self {
        Self {
            print: PrintRecord::sample_full(),
            printer: Some(PrinterSnapshot::sample_full()),
            job: Some(JobSnapshot::sample_full()),
            manifest: Some(JobManifest::sample_full()),
            bounds: EffectiveBounds::sample_full(),
            interventions: vec![Intervention::sample_full()],
            recent_events: vec![EventRecord::sample_full()],
            latest_image: Some(ImageRef::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            print: PrintRecord::sample_minimal(),
            printer: None,
            job: None,
            manifest: None,
            bounds: EffectiveBounds::sample_full(),
            interventions: vec![],
            recent_events: vec![],
            latest_image: None,
        }
    }
}
