//! What `OctoPrint` answers with, and the normalized snapshots it becomes.
//!
//! Every field of both snapshot contracts, and of the facts of a file, is
//! filled from the field `OctoPrint` reports it in, and is left absent when
//! `OctoPrint` reports nothing. Three fields of [`PrinterSnapshot`] are always absent against this
//! source and that is a fact about the source rather than a gap here:
//! `GET /api/printer` reports no applied feedrate factor, no applied flowrate
//! factor and no fan setting, and there is no other endpoint that does.

use std::collections::BTreeMap;

use printobserver_printer_api::{
    COMPLETION_RANGE, HEATER_ACTUAL_C_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE,
    HeaterSnapshot, JobSnapshot, PrinterSnapshot,
};
use printobserver_printer_api::{
    ErrorSource, FileFacts, FilePrint, JobError, PrinterState, ToolFilament,
};
use printobserver_types::{FileName, Reported, Timestamp};
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
    /// The filament the file's analysis says each tool uses, keyed `tool<n>`.
    #[serde(default)]
    filament: Option<FilamentPayload>,
}

/// The filament `OctoPrint`'s analysis reports, keyed by `tool<n>`.
///
/// Held as values rather than as a struct per tool, because a key that names
/// no tool, or a tool `OctoPrint` reports as null, is one to skip rather than
/// one that fails the whole read.
type FilamentPayload = BTreeMap<String, serde_json::Value>;

/// One tool's filament, as `OctoPrint`'s analysis reports it.
#[derive(Debug, Default, Deserialize)]
struct ToolFilamentPayload {
    /// The length, in millimetres.
    #[serde(default)]
    length: Option<f64>,
    /// The volume, in cubic centimetres.
    #[serde(default)]
    volume: Option<f64>,
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
    /// How far into the file the printer has read, in bytes.
    #[serde(default)]
    filepos: Option<i64>,
    /// How long the print has been running, in seconds.
    #[serde(default)]
    print_time: Option<i64>,
    /// How long the print has left, in seconds.
    #[serde(default)]
    print_time_left: Option<i64>,
}

/// One entry of `GET /api/files/local/<name>`, or of a listing: a file or a
/// folder.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilePayload {
    /// Its name, which a separator, a NUL or nothing at all is refused as.
    name: FileName,
    /// Where it lives.
    origin: Origin,
    /// Whether it is a file or a folder.
    #[serde(rename = "type", default)]
    kind: EntryKind,
    /// Its size in bytes.
    #[serde(default)]
    size: Option<i64>,
    /// `OctoPrint`'s analysis of it, once the analysis queue has reached it.
    #[serde(default)]
    gcode_analysis: Option<AnalysisPayload>,
    /// Its print tallies, once it has been printed.
    #[serde(default)]
    prints: Option<PrintsPayload>,
    /// The links `OctoPrint` reaches it at.
    #[serde(default)]
    refs: Option<RefsPayload>,
    /// What a folder holds.
    #[serde(default)]
    children: Vec<FilePayload>,
}

/// Where `OctoPrint` keeps a file: its own storage, or the printer's card.
///
/// The two are all `OctoPrint`'s file API has, so a third is a malformed
/// answer rather than a file to pass on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Origin {
    /// `local`: the instance's own storage.
    Local,
    /// `sdcard`: the printer's card.
    Sdcard,
}

impl Origin {
    /// `OctoPrint`'s own word for it, which is what the contract carries.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Sdcard => "sdcard",
        }
    }
}

/// How a print ended, as the last print of a file reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "bool")]
enum Outcome {
    /// It completed.
    Completed,
    /// It was cancelled, or failed.
    NotCompleted,
}

impl From<bool> for Outcome {
    fn from(success: bool) -> Self {
        if success {
            Self::Completed
        } else {
            Self::NotCompleted
        }
    }
}

/// What one entry of `OctoPrint`'s storage is.
///
/// Only the folder is told apart, because only a folder is not a file; every
/// other word `OctoPrint` spells a type with, today `machinecode` and `model`,
/// is a file.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EntryKind {
    /// A folder, holding entries of its own.
    Folder,
    /// Anything else: a file.
    #[default]
    #[serde(other)]
    File,
}

/// What `GET /api/files?recursive=true` answers with.
#[derive(Debug, Deserialize)]
pub(crate) struct ListingPayload {
    /// Every entry at the top of every origin, folders holding their own; a
    /// listing without it is no listing.
    files: Vec<FilePayload>,
}

/// The analysis block of a file.
#[derive(Debug, Default, Deserialize)]
struct AnalysisPayload {
    /// The filament each tool uses.
    #[serde(default)]
    filament: Option<FilamentPayload>,
}

/// The print tallies of a file.
#[derive(Debug, Default, Deserialize)]
struct PrintsPayload {
    /// How many prints completed.
    #[serde(default)]
    success: Option<i64>,
    /// How many did not.
    #[serde(default)]
    failure: Option<i64>,
    /// The most recent print.
    #[serde(default)]
    last: Option<LastPrintPayload>,
}

/// The most recent print of a file.
#[derive(Debug, Default, Deserialize)]
struct LastPrintPayload {
    /// When it ended, in seconds since the epoch, fractional.
    #[serde(default)]
    date: Option<f64>,
    /// How it ended.
    #[serde(default)]
    success: Option<Outcome>,
}

/// The links of a file.
#[derive(Debug, Default, Deserialize)]
struct RefsPayload {
    /// Where its bytes are downloaded from.
    #[serde(default)]
    download: Option<DownloadLink>,
}

/// The download link `OctoPrint` answered, read as what this port can do
/// with it.
///
/// `OctoPrint` spells the link absolute, with whatever host the request reached
/// it by, so its scheme and authority are dropped and the path alone is kept:
/// the key goes to the instance the configuration names and nowhere else. A
/// link is read when it arrives rather than when it is followed, so nothing
/// downstream holds a link it has not been told whether it can follow — and an
/// unfollowable one is carried, with its text, rather than refused, because it
/// is a fault in the download alone and not in the facts read beside it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub(crate) enum DownloadLink {
    /// A path to send to the configured instance.
    Followable(DownloadPath),
    /// A link whose path would change the request it lands in, as answered.
    Unfollowable(String),
}

/// The path of a followable download link: absolute, and free of anything
/// that would end the request line it is written into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DownloadPath(String);

impl DownloadPath {
    /// The path, to write into a request line.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for DownloadLink {
    fn from(link: String) -> Self {
        let path = match link.split_once("://") {
            Some((_, rest)) => rest.find('/').map_or("/", |start| &rest[start..]),
            None => link.as_str(),
        };
        if path.starts_with('/')
            && !path
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b' ')
        {
            Self::Followable(DownloadPath(path.to_owned()))
        } else {
            Self::Unfollowable(link)
        }
    }
}

impl FilePayload {
    /// The link `OctoPrint` says this file's bytes are downloaded from.
    pub(crate) fn download(&self) -> Option<&DownloadLink> {
        self.refs.as_ref()?.download.as_ref()
    }

    /// The facts this entry denotes, or why they are not facts about a file.
    ///
    /// A size or a tally below zero is no count of anything, so an entry
    /// carrying one is refused rather than passed on as though it were.
    pub(crate) fn facts(self) -> Result<FileFacts, String> {
        if self.kind == EntryKind::Folder {
            return Err(format!("{} is a folder rather than a file", self.name));
        }
        let prints = self.prints.unwrap_or_default();
        let successes = prints.success.unwrap_or(0);
        let failures = prints.failure.unwrap_or(0);
        if self.size.is_some_and(|size| size < 0) || successes < 0 || failures < 0 {
            return Err(format!(
                "{} carries a negative size or tally: size {:?}, {successes} succeeded, \
                 {failures} failed",
                self.name, self.size
            ));
        }
        Ok(FileFacts {
            file_name: self.name.as_str().to_owned(),
            origin: self.origin.as_str().to_owned(),
            size_bytes: self.size,
            filament: self
                .gcode_analysis
                .and_then(|analysis| analysis.filament)
                .map(|filament| filament_of(&filament)),
            successes,
            failures,
            // A last print missing its date or its outcome, or dated where no
            // timestamp is, says nothing a last print is made of.
            last_print: prints.last.and_then(|last| {
                let at = Timestamp::from_unix_seconds(epoch_seconds(last.date?)?).ok()?;
                Some(FilePrint {
                    at,
                    success: last.success? == Outcome::Completed,
                })
            }),
        })
    }
}

impl ListingPayload {
    /// Every file of the listing that carries a print tally, folders flattened.
    ///
    /// A folder carries a tally of its own — `OctoPrint` sums its children's —
    /// and is not a file, so it is never one of these.
    pub(crate) fn printed(self) -> Result<Vec<FileFacts>, String> {
        let mut printed = Vec::new();
        let mut pending = self.files;
        while let Some(mut entry) = pending.pop() {
            if entry.kind == EntryKind::Folder {
                pending.append(&mut entry.children);
            } else if entry.prints.is_some() {
                printed.push(entry.facts()?);
            }
        }
        printed.sort_by(|a, b| (&a.origin, &a.file_name).cmp(&(&b.origin, &b.file_name)));
        Ok(printed)
    }
}

/// The whole second a date `OctoPrint` reports in fractional epoch seconds
/// falls in, when it names one.
///
/// [`whole_seconds`] answers zero for a value no `i64` holds, which here would
/// be a print dated 1970 rather than a date that is not one.
fn epoch_seconds(date: f64) -> Option<i64> {
    /// Further from the epoch than any `i64` count of seconds a date spells.
    const LIMIT: f64 = 1e15;
    (date.is_finite() && date.abs() < LIMIT).then(|| whole_seconds(date))
}

/// The filament every tool `OctoPrint` reports a length for, in tool order.
///
/// A tool whose length is null, or is no length of anything — below zero, or
/// not a number — is omitted, and so is a key naming no tool, one whose number
/// is not a whole number from zero. A volume below zero is no volume.
fn filament_of(filament: &FilamentPayload) -> Vec<ToolFilament> {
    let mut tools: Vec<ToolFilament> = filament
        .iter()
        .filter_map(|(name, value)| {
            let tool = i64::from(name.strip_prefix(TOOL_PREFIX)?.parse::<u32>().ok()?);
            let payload: ToolFilamentPayload = serde_json::from_value(value.clone()).ok()?;
            let measured = |value: &f64| value.is_finite() && *value >= 0.0;
            Some(ToolFilament {
                tool,
                length_mm: payload.length.filter(measured)?,
                volume_cm3: payload.volume.filter(measured),
            })
        })
        .collect();
    tools.sort_by_key(|tool| tool.tool);
    tools
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
    /// Whether the instance has started the job and not yet begun to send it.
    ///
    /// `OctoPrint` is `Starting` while it drains the commands queued ahead of
    /// the job, and its move to `Printing` overwrites a pause asked meanwhile.
    pub(crate) fn is_starting(&self) -> bool {
        self.state.as_deref() == Some(STARTING)
    }

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
            error: self.error.as_deref().and_then(|text| {
                error_source_of(text).map(|source| JobError {
                    text: text.to_owned(),
                    source,
                })
            }),
            // A position before the file's first byte is no position at all.
            file_position_bytes: progress.filepos.filter(|position| *position >= 0),
            filament: job.filament.as_ref().map(filament_of).unwrap_or_default(),
        }
    }
}

/// The state `OctoPrint`'s flags denote.
///
/// The order is the whole of the mapping: an error is an error whatever else is
/// set, a closed connection is offline, and a printer that is cancelling still
/// has `printing` set, so that is read before it.
///
/// **A printer that is `pausing` is still printing**, and that is a decision
/// rather than an oversight. `OctoPrint` sets `pausing` from the moment it
/// accepts a pause until the machine actually stops, which on a printer part-way
/// through a long move is many seconds later; this vocabulary has no arm for the
/// transition, and answering `Paused` while the head is still moving is exactly
/// the confident lie about a machine that this layer exists not to tell. So
/// `Paused` here means stopped, and nothing else does. `resuming` needs no arm
/// of its own for the same reason: `printing` is already set through it.
fn connection_of(state: &StatePayload) -> PrinterState {
    let flags = &state.flags;
    if flag(flags, "error") {
        PrinterState::Error
    } else if flag(flags, "closedOrError") {
        PrinterState::Offline
    } else if flag(flags, "cancelling") {
        PrinterState::Cancelling
    } else if flag(flags, "paused") {
        PrinterState::Paused
    } else if flag(flags, "printing") || flag(flags, "pausing") {
        PrinterState::Printing
    } else if flag(flags, "operational") {
        PrinterState::Operational
    } else {
        PrinterState::Unknown(state.text.clone().unwrap_or_default())
    }
}

/// Whose error one job's error text is: the printer's own message, or the
/// name of an exception the host raised — an identifier, dotted or not, with
/// something before its closing `Exception` or `Error` and followed by its
/// colon, such as
/// `SerialTimeoutException: `. Nothing for no text at all.
///
/// `OctoPrint` reports a stop that went offline with the same state whether
/// the printer's firmware stopped it or the host lost the line, and this is
/// the one trace in a read of which it was.
#[must_use]
pub fn error_source_of(error: &str) -> Option<ErrorSource> {
    let text = error.trim();
    if text.is_empty() {
        return None;
    }
    Some(if is_exception_name(text) {
        ErrorSource::Host
    } else {
        ErrorSource::Printer
    })
}

/// Whether text begins with an exception name and its colon.
fn is_exception_name(text: &str) -> bool {
    let Some((name, _)) = text.split_once(':') else {
        return false;
    };
    let starts = name
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
    starts
        && name
            .chars()
            .all(|each| each.is_ascii_alphanumeric() || each == '_' || each == '.')
        && ["Exception", "Error"].iter().any(|suffix| {
            name.strip_suffix(suffix)
                .is_some_and(|before| !before.is_empty())
        })
}

/// What `GET /api/connection` answers, as far as this adapter reads it.
#[derive(Debug, Deserialize)]
pub(crate) struct ConnectionPayload {
    /// The connection as it is now.
    current: ConnectionCurrent,
}

/// The connection as it is now.
#[derive(Debug, Deserialize)]
struct ConnectionCurrent {
    /// `OctoPrint`'s own word for the printer's state.
    #[serde(default)]
    state: Option<String>,
}

impl ConnectionPayload {
    /// A snapshot of a printer this connection says is offline or in error,
    /// carrying nothing but that state; nothing for any other state.
    pub(crate) fn disconnected(&self, observed_at: Timestamp) -> Option<PrinterSnapshot> {
        let connection = job_state_of(self.current.state.as_deref());
        matches!(connection, PrinterState::Offline | PrinterState::Error).then(|| PrinterSnapshot {
            connection,
            tools: Vec::new(),
            bed: None,
            chamber: None,
            feedrate_factor: None,
            flowrate_factor: None,
            fan_percent: None,
            observed_at,
        })
    }
}

/// `OctoPrint`'s word for a job it has started and not yet begun to send.
const STARTING: &str = "Starting";

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
        "Printing" | STARTING | "Resuming" | "Printing from SD" | "Sending file to SD" => {
            PrinterState::Printing
        }
        "Paused" => PrinterState::Paused,
        "Pausing" => PrinterState::Printing,
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
