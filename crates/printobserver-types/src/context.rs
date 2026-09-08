//! The one aggregate a caller and the agent both read.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::EventRecord;
use crate::image::ImageRef;
use crate::intervention::Intervention;
use crate::manifest::JobManifest;
use crate::policy::EffectiveBounds;
use crate::print::PrintRecord;
use crate::printer::{JobSnapshot, PrinterSnapshot};

/// Everything a supervision turn is given about one print.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
