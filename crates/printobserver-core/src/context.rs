//! The one aggregate a caller and the agent both read.
//!
//! [`PrintContext`] is the supervision domain's own view of one print: the
//! record, the printer and job as the printer port last reported them, the
//! manifest and the bounds in force, the interventions still active, the
//! events most recently written down — the print's own, and what the printer
//! watch recorded of the machine since the print opened — the failure
//! detector's health, and what the slicer planned at the job's current file
//! position ([`crate::gcode_index`]). The loop collects it when it handles an
//! event, holds it for the turn that event prompts, and answers it to the
//! `context` operation, so what the agent reasons about is the state the event
//! was handled at rather than whatever the machine has drifted to since.
//!
//! It is declared here rather than centrally because every field of it is
//! this domain's to collect: a domain that adds a fact a turn should see edits
//! this crate and regenerates the clients, and nothing central.

use crate::gcode_index::SlicerPlan;
use crate::records::{EffectiveBounds, Intervention, JobManifest, PrintRecord};
use crate::watch::{DetectorHealth, DetectorHealthStatus};
use printobserver_printer_api::{
    HeaterSnapshot, JobError, JobSnapshot, PrinterSnapshot, PrinterState,
};
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::serde_json::Value;
use printobserver_types::{
    EventId, EventKind, EventRecord, ImageRef, PrintId, Reported, Timestamp,
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
    /// The failure detector's health, and since when it has been so.
    #[serde(default)]
    pub detector: DetectorHealth,
    /// What the slicer planned at the job's current file position, read from
    /// the print's own G-code: absent when the file could not be read, or the
    /// job is not this print's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slicer: Option<SlicerPlan>,
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
            detector: DetectorHealth::sample_full(),
            slicer: Some(SlicerPlan::sample_full()),
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
            detector: DetectorHealth::sample_minimal(),
            slicer: None,
        }
    }
}

/// The payload fields a context's recent events leave out: the printer and job
/// snapshots an observation took, which the context states once as they are
/// now, and the bookkeeping no supervision decision reads.
const UNREAD_PAYLOAD_FIELDS: [&str; 6] = [
    "printer",
    "job",
    "interval_s",
    "session_name",
    "delivered",
    "waited_s",
];

/// What a supervision turn reads about one print: [`PrintContext`] with each
/// fact stated once, as it is now, and nothing no supervision decision reads.
///
/// The printer and the job are stated here and nowhere in the events; each
/// event is what it was and when; and asked `since` an event, only the events
/// newer than it are carried, so a turn that has read the rest is not handed
/// them again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentContext {
    /// The print.
    pub print: AgentPrint,
    /// The printer now, when it could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<AgentPrinter>,
    /// The job now, when it could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<AgentJob>,
    /// The manifest, when the print has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<JobManifest>,
    /// The limits in force, so that the agent can see them before it asks.
    pub bounds: EffectiveBounds,
    /// The interventions still active.
    pub interventions: Vec<Intervention>,
    /// This print's recent events, newest first, past the one asked `since`.
    pub recent_events: Vec<AgentEvent>,
    /// What the failure detector's last health check found.
    pub detector: AgentDetector,
    /// What the slicer planned at the job's current file position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slicer: Option<SlicerPlan>,
}

/// The print a context is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentPrint {
    /// Its identifier.
    pub id: PrintId,
    /// What the printer was last found doing with it.
    pub state: PrinterState,
}

/// The printer as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentPrinter {
    /// What it is doing.
    pub connection: PrinterState,
    /// Each tool's heater, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AgentHeater>,
    /// The bed's heater, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed: Option<AgentHeater>,
}

/// One heater, in degrees Celsius: each reading absent when the printer did not
/// report it, or reported one outside what a heater can read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentHeater {
    /// The temperature it is at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_c: Option<f64>,
    /// The temperature it is driving towards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_c: Option<f64>,
}

/// The job as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentJob {
    /// What the printer is doing with it.
    pub state: PrinterState,
    /// The file it prints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// How far through it is, from zero to one, when that was read in range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<f64>,
    /// How long it has been printing, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_time_s: Option<i64>,
    /// How far into its file the printer has read, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_position_bytes: Option<i64>,
    /// The error it stopped on, when it stopped on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JobError>,
}

/// One event of the print's, as a turn reads it: what it was and when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentEvent {
    /// Its identifier, which a later read asks `since`.
    pub id: EventId,
    /// What kind of event it is.
    pub kind: EventKind,
    /// When it was received.
    pub received_at: Timestamp,
    /// What it carried, less the snapshots the context states once and the
    /// bookkeeping no decision reads.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

/// The failure detector, as a decision reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentDetector {
    /// What its last health check found.
    pub status: DetectorHealthStatus,
}

impl PrintContext {
    /// What a supervision turn reads of this context, carrying only the events
    /// newer than `since` when it names one.
    #[must_use]
    pub fn for_agent(&self, since: Option<EventId>) -> AgentContext {
        let heater = |heater: &HeaterSnapshot| AgentHeater {
            actual_c: in_range(heater.actual_c.as_ref()),
            target_c: in_range(heater.target_c.as_ref()),
        };
        AgentContext {
            print: AgentPrint {
                id: self.print.id,
                state: self.print.state.clone(),
            },
            printer: self.printer.as_ref().map(|printer| AgentPrinter {
                connection: printer.connection.clone(),
                tools: printer.tools.iter().map(heater).collect(),
                bed: printer.bed.as_ref().map(heater),
            }),
            job: self.job.as_ref().map(|job| AgentJob {
                state: job.state.clone(),
                file_name: job.file_name.clone(),
                completion: in_range(job.completion.as_ref()),
                print_time_s: job.print_time_s,
                file_position_bytes: job.file_position_bytes,
                error: job.error.clone(),
            }),
            manifest: self.manifest.clone(),
            bounds: self.bounds.clone(),
            interventions: self.interventions.clone(),
            recent_events: self
                .recent_events
                .iter()
                .filter(|event| since.is_none_or(|since| event.id > since))
                .map(agent_event)
                .collect(),
            detector: AgentDetector {
                status: self.detector.status(),
            },
            slicer: self.slicer.clone(),
        }
    }
}

/// A reading in range, as its value alone.
fn in_range(reported: Option<&Reported<f64>>) -> Option<f64> {
    reported
        .filter(|reported| !reported.out_of_range())
        .map(Reported::value)
}

/// One event, as a turn reads it.
fn agent_event(event: &EventRecord) -> AgentEvent {
    let mut payload = event.body.payload.clone();
    if let Value::Object(fields) = &mut payload {
        for unread in UNREAD_PAYLOAD_FIELDS {
            fields.remove(unread);
        }
    }
    AgentEvent {
        id: event.id,
        kind: event.kind().clone(),
        received_at: event.received_at,
        payload,
    }
}

impl Sample for AgentContext {
    fn sample_full() -> Self {
        PrintContext::sample_full().for_agent(None)
    }

    fn sample_minimal() -> Self {
        PrintContext::sample_minimal().for_agent(None)
    }
}
