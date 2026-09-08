//! What `OctoPrint` answers with, and the normalized snapshots it becomes.
//!
//! Every field of both snapshot contracts is filled from the field
//! `OctoPrint` reports it in, and is left absent when `OctoPrint` reports
//! nothing. Three fields of [`PrinterSnapshot`] are always absent against this
//! source and that is a fact about the source rather than a gap here:
//! `GET /api/printer` reports no applied feedrate factor, no applied flowrate
//! factor and no fan setting, and there is no other endpoint that does.

use std::collections::BTreeMap;

use printobserver_types::reported::{
    COMPLETION_RANGE, HEATER_ACTUAL_C_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE,
};
use printobserver_types::{
    HeaterSnapshot, JobSnapshot, PrinterSnapshot, PrinterState, Reported, Timestamp,
};
use serde::Deserialize;

use crate::convert::{fraction_of_completion, whole_seconds};

/// The prefix `OctoPrint` spells a tool's number after, in its temperature map
/// and in the `targets` map the tool endpoint takes.
pub(crate) const TOOL_PREFIX: &str = "tool";

/// What `GET /api/printer` answers with.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct PrinterPayload {
    /// The state block, when the instance reports one.
    #[serde(default)]
    state: Option<StatePayload>,
    /// The temperature block, keyed by heater name.
    #[serde(default)]
    temperature: BTreeMap<String, serde_json::Value>,
}

/// The state block of `GET /api/printer`.
#[derive(Debug, Default, Deserialize)]
struct StatePayload {
    /// The instance's own word for the state.
    #[serde(default)]
    text: Option<String>,
    /// The flags the state is decided from.
    #[serde(default)]
    flags: FlagsPayload,
}

/// The flags `OctoPrint` reports a printer's state as.
///
/// Held as the map `OctoPrint` sends rather than as a field per flag: the set
/// has grown across `OctoPrint` releases, and a flag this mapping does not read
/// is one that should arrive and be ignored rather than one that fails a read.
type FlagsPayload = BTreeMap<String, serde_json::Value>;

/// Whether one flag is set, treating one the instance did not send as unset.
fn flag(flags: &FlagsPayload, name: &str) -> bool {
    flags.get(name).and_then(serde_json::Value::as_bool) == Some(true)
}

/// What `GET /api/job` answers with.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct JobPayload {
    /// The job block.
    #[serde(default)]
    job: Option<JobBody>,
    /// The progress block.
    #[serde(default)]
    progress: Option<ProgressBody>,
    /// The instance's own word for the job's state.
    #[serde(default)]
    state: Option<String>,
    /// The error the instance reports, when it reports one.
    #[serde(default)]
    error: Option<String>,
}

/// The job block of `GET /api/job`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobBody {
    /// The file being printed.
    #[serde(default)]
    file: Option<FileBody>,
    /// The whole print's estimated duration, in seconds.
    #[serde(default)]
    estimated_print_time: Option<f64>,
}

/// The file block of `GET /api/job`.
#[derive(Debug, Default, Deserialize)]
struct FileBody {
    /// The file's name.
    #[serde(default)]
    name: Option<String>,
    /// Where the file lives, in `OctoPrint`'s own vocabulary.
    #[serde(default)]
    origin: Option<String>,
    /// The file's size in bytes.
    #[serde(default)]
    size: Option<i64>,
}

/// The progress block of `GET /api/job`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressBody {
    /// How far through the print is, as a percentage.
    #[serde(default)]
    completion: Option<f64>,
    /// How long the print has been running, in seconds.
    #[serde(default)]
    print_time: Option<i64>,
    /// How long the print has left, in seconds.
    #[serde(default)]
    print_time_left: Option<i64>,
}

/// One heater, as `OctoPrint` reports it.
#[derive(Debug, Default, Deserialize)]
struct HeaterPayload {
    /// The temperature the heater is at.
    #[serde(default)]
    actual: Option<f64>,
    /// The temperature it is driving towards.
    #[serde(default)]
    target: Option<f64>,
    /// The offset applied to its target.
    #[serde(default)]
    offset: Option<f64>,
}

impl PrinterPayload {
    /// The normalized snapshot this payload denotes, observed now.
    pub(crate) fn snapshot(self, observed_at: Timestamp) -> PrinterSnapshot {
        let state = self.state.unwrap_or_default();
        PrinterSnapshot {
            connection: connection_of(&state),
            tools: tools_of(&self.temperature),
            bed: heater_of(self.temperature.get("bed")),
            chamber: heater_of(self.temperature.get("chamber")),
            // `GET /api/printer` reports no applied factor and no fan setting,
            // so these three are absent against this source, always.
            feedrate_factor: None,
            flowrate_factor: None,
            fan_percent: None,
            observed_at,
        }
    }
}

impl JobPayload {
    /// The normalized snapshot this payload denotes.
    pub(crate) fn snapshot(self) -> JobSnapshot {
        let job = self.job.unwrap_or_default();
        let file = job.file.unwrap_or_default();
        let progress = self.progress.unwrap_or_default();
        JobSnapshot {
            file_name: file.name,
            file_origin: file.origin,
            size_bytes: file.size,
            estimated_print_time_s: job.estimated_print_time.map(whole_seconds),
            completion: progress
                .completion
                .map(|value| Reported::new(fraction_of_completion(value), COMPLETION_RANGE)),
            print_time_s: progress.print_time,
            print_time_left_s: progress.print_time_left,
            state: job_state_of(self.state.as_deref()),
            error: self.error,
        }
    }
}

/// The state `OctoPrint`'s flags denote.
///
/// The order is the whole of the mapping: an error is an error whatever else is
/// set, a closed connection is offline, and a printer that is cancelling or
/// pausing still has `printing` set, so those are read before it.
fn connection_of(state: &StatePayload) -> PrinterState {
    let flags = &state.flags;
    if flag(flags, "error") {
        PrinterState::Error
    } else if flag(flags, "closedOrError") {
        PrinterState::Offline
    } else if flag(flags, "cancelling") {
        PrinterState::Cancelling
    } else if flag(flags, "paused") || flag(flags, "pausing") {
        PrinterState::Paused
    } else if flag(flags, "printing") {
        PrinterState::Printing
    } else if flag(flags, "operational") {
        PrinterState::Operational
    } else {
        PrinterState::Unknown(state.text.clone().unwrap_or_default())
    }
}

/// The state `OctoPrint`'s own word for a job denotes.
///
/// The words are `OctoPrint`'s, and the ones this vocabulary has no arm for
/// are carried through as `Unknown` in its own spelling rather than guessed at.
fn job_state_of(text: Option<&str>) -> PrinterState {
    let Some(text) = text else {
        return PrinterState::Unknown(String::new());
    };
    match text {
        "Operational" | "Ready" | "Finishing" => PrinterState::Operational,
        "Printing" | "Starting" | "Resuming" | "Printing from SD" | "Sending file to SD" => {
            PrinterState::Printing
        }
        "Paused" | "Pausing" => PrinterState::Paused,
        "Cancelling" => PrinterState::Cancelling,
        "Error" => PrinterState::Error,
        other if other.starts_with("Offline") || other.starts_with("Closed") => {
            PrinterState::Offline
        }
        other => PrinterState::Unknown(other.to_owned()),
    }
}

/// The tool heaters, indexed by the number `OctoPrint` spells in the key.
///
/// A gap in the numbering is filled with a heater reporting nothing, so that the
/// index into this vector is the printer's own tool number rather than a
/// position in whatever subset was reported.
fn tools_of(temperature: &BTreeMap<String, serde_json::Value>) -> Vec<HeaterSnapshot> {
    let mut numbered: BTreeMap<usize, HeaterSnapshot> = BTreeMap::new();
    for (name, value) in temperature {
        let Some(number) = name.strip_prefix(TOOL_PREFIX) else {
            continue;
        };
        let Ok(number) = number.parse::<usize>() else {
            continue;
        };
        if let Some(heater) = heater_of(Some(value)) {
            numbered.insert(number, heater);
        }
    }
    let Some(highest) = numbered.keys().next_back().copied() else {
        return Vec::new();
    };
    (0..=highest)
        .map(|number| numbered.remove(&number).unwrap_or_default())
        .collect()
}

/// One heater, when the value is one `OctoPrint` reported.
fn heater_of(value: Option<&serde_json::Value>) -> Option<HeaterSnapshot> {
    let payload: HeaterPayload = serde_json::from_value(value?.clone()).ok()?;
    Some(HeaterSnapshot {
        actual_c: payload
            .actual
            .map(|value| Reported::new(value, HEATER_ACTUAL_C_RANGE)),
        target_c: payload
            .target
            .map(|value| Reported::new(value, HEATER_TARGET_C_RANGE)),
        offset_c: payload
            .offset
            .map(|value| Reported::new(value, HEATER_OFFSET_C_RANGE)),
    })
}
