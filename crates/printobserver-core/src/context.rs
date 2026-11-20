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
use printobserver_types::EventPayload;
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
/// now; the agent's own assessment, which its conversation already holds; and
/// the bookkeeping no supervision decision reads.
const UNREAD_PAYLOAD_FIELDS: [&str; 8] = [
    "printer",
    "job",
    "assessment",
    "interval_s",
    "session_name",
    "harness_identity",
    "delivered",
    "waited_s",
];

/// What a supervision turn reads about one print: [`PrintContext`] with each
/// fact stated once, as it is now, and nothing no supervision decision reads.
///
/// Left out of it, each because no step of the skill, no check and no recorded
/// case decides anything by it: the print's file, provider and times; the
/// printer's and the job's own states (the print states it once); every heater's
/// offset, the chamber and the time they were read; the printer's reported
/// feedrate, flowrate and fan (the slicer's plan states the flow and fan in
/// force, and no step reads the feedrate); the job's file, size, progress, time
/// and position (the plan states where the print is); the manifest (its limits
/// are the bounds); and the detector's health beyond its status — since when,
/// and why — which only the status read carries.
///
/// The printer and the job are stated here and nowhere in the events; each
/// event is what happened, when, and what it changed; and asked `since` an
/// event, only the events newer than it are carried, so a turn that has read
/// the rest is not handed them again. `newest_event` is what the next read
/// asks `since`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentContext {
    /// The print.
    pub print: AgentPrint,
    /// The printer's heaters now, when they could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<AgentPrinter>,
    /// The error the job stopped on, when it stopped on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_error: Option<JobError>,
    /// The limits in force, so that the agent can see them before it asks.
    pub bounds: EffectiveBounds,
    /// The interventions still active.
    pub interventions: Vec<Intervention>,
    /// What the failure detector's status is for this print: whether it is
    /// scoring the print and has flagged it, or what it is otherwise.
    pub detector: DetectorHealthStatus,
    /// This print's events newer than the one asked `since`, or its recent
    /// ones when none was named, newest first.
    pub recent_events: Vec<AgentEvent>,
    /// The most recent image, whose path is answered beside the context and
    /// whose record states its digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_image: Option<ImageRef>,
    /// The newest event of this print's, which a later read asks `since`:
    /// absent only when the print has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest_event: Option<EventId>,
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
    /// What the printer was last found doing with it, or the state it ended in.
    pub state: PrinterState,
    /// Why it ended, once it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
}

/// The printer's heaters as they are now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct AgentPrinter {
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

/// One event of the print's, as a turn reads it: what happened, when, and the
/// facts it changed.
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
    /// What it carried, less the snapshots the context states once, the
    /// agent's own assessment and the bookkeeping no decision reads.
    #[serde(default, skip_serializing_if = "is_empty_payload")]
    pub payload: Value,
}

/// Whether a payload says nothing.
fn is_empty_payload(payload: &Value) -> bool {
    match payload {
        Value::Null => true,
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

/// The events a context read asked `since` one carries: that event, and the
/// print's events newer than it, newest first.
#[derive(Debug, Clone, Copy)]
pub struct Since<'events> {
    /// The event named.
    pub event: EventId,
    /// Every event of the print's newer than it, newest first.
    pub newer: &'events [EventRecord],
}

impl PrintContext {
    /// What a supervision turn reads of this context: its recent events, or
    /// those `since` names when it names any.
    #[must_use]
    pub fn for_agent(&self, since: Option<Since<'_>>) -> AgentContext {
        let heater = |heater: &HeaterSnapshot| AgentHeater {
            actual_c: in_range(heater.actual_c.as_ref()),
            target_c: in_range(heater.target_c.as_ref()),
        };
        let (events, newest_event) = match since {
            Some(since) => (
                since.newer,
                Some(since.newer.first().map_or(since.event, |newest| newest.id)),
            ),
            None => (
                self.recent_events.as_slice(),
                self.recent_events.first().map(|newest| newest.id),
            ),
        };
        AgentContext {
            print: AgentPrint {
                id: self.print.id,
                state: self.print.state.clone(),
                end_reason: self.print.end_reason.clone(),
            },
            printer: self.printer.as_ref().map(|printer| AgentPrinter {
                tools: printer.tools.iter().map(heater).collect(),
                bed: printer.bed.as_ref().map(heater),
            }),
            job_error: self.job.as_ref().and_then(|job| job.error.clone()),
            bounds: self.bounds.clone(),
            interventions: self.interventions.clone(),
            detector: self.detector.status(),
            recent_events: events.iter().map(agent_event).collect(),
            latest_image: self.latest_image.clone(),
            newest_event,
            slicer: self.slicer.clone(),
        }
    }
}

/// The facts a supervision turn is handed up front: the context read as the
/// turn began, as [`AgentContext`] answers it — and no field that answer does
/// not carry — with its events cut to those newer than the agent's last
/// answer, newest first: what happened since the agent last spoke, which its
/// own conversation does not hold. A print the agent has not answered on yet
/// keeps the whole window.
#[must_use]
pub fn turn_facts(
    context: &AgentContext,
) -> Option<printobserver_types::serde_json::Map<String, Value>> {
    let mut facts = context.clone();
    if let Some(answered) = facts.recent_events.iter().position(|event| {
        event.kind.as_str() == <crate::kinds::AgentAssessmentPayload as EventPayload>::KIND
    }) {
        facts.recent_events.truncate(answered);
    }
    match printobserver_types::serde_json::to_value(facts) {
        Ok(Value::Object(fields)) => Some(fields),
        _ => None,
    }
}

/// The events newer than `since`, newest first, when `events` — newest first —
/// holds it.
#[must_use]
pub fn newer_than(events: &[EventRecord], since: EventId) -> Option<&[EventRecord]> {
    events
        .iter()
        .position(|event| event.id == since)
        .map(|at| &events[..at])
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
