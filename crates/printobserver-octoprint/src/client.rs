//! The printer port, spoken to one `OctoPrint` instance.
//!
//! This is the only place in this repository that names an `OctoPrint` path, and
//! a check refuses one named anywhere else. Every path is a constant of this
//! module and every body is built from a bounded value here: no method of the
//! port takes text, so nothing a caller supplies reaches the instance except the
//! one file name [`PrinterPort::start`], [`PrinterPort::file_facts`] and
//! [`PrinterPort::extruded_to`] take, which is percent-encoded into a single
//! path segment.
//!
//! # Reading, and acting
//!
//! Reading is `GET /api/printer` and `GET /api/job`, and for the files the
//! instance holds `GET /api/files/local/<name>` and
//! `GET /api/files?recursive=true`. Acting is `POST` to
//! `/api/job`, `/api/printer/printhead`, `/api/printer/tool`,
//! `/api/printer/bed` and — for the fan alone — `/api/printer/command`. Every
//! acting endpoint answers `204 No Content` on success, so what happened is read
//! from the status rather than from a body.
//!
//! `start` is the one action that is two requests. The port takes the name of
//! the file to print and `POST /api/job` starts whatever is *selected*, so this
//! selects the named file first and then starts it. Selecting and printing in
//! one request would collapse them, but it would also make a start that failed
//! indistinguishable from a selection that did.
//!
//! # Reading a file's own G-code
//!
//! [`PrinterPort::extruded_to`] reads the file's facts and then downloads it
//! from the link those facts name, through the same authenticated request as
//! every other read — but only the **path** of that link, sent to the
//! configured instance: the key is never sent to a host the link names. The
//! download is streamed through [`crate::gcode`] and the connection closed the
//! moment the position is reached, so the file is never held whole and the
//! rest of it need never arrive.
//!
//! # What each status means
//!
//! `401` and `403` are the key: `OctoPrint` answers both to a request whose
//! `X-Api-Key` it does not know. `409` is a state conflict, which is what
//! `OctoPrint` answers a pause with no print running, a start with one already
//! running, and a read of a printer that is not operational. Every other
//! non-success status is a refusal carrying `OctoPrint`'s own words, so the
//! layer above can tell "the machine will not" from "the machine is not there".

use core::fmt::Write as _;
use core::ops::ControlFlow;

use printobserver_printer_api::{Adjustable, FileFacts, ToolFilament};
use printobserver_printer_api::{
    BoxFuture, JobSnapshot, PrinterError, PrinterPort, PrinterSnapshot,
};
use printobserver_types::{FileName, Timestamp};
use serde_json::json;

use crate::config::{FanSupport, OctoPrintConfig};
use crate::convert::{
    FAN_PWM_PARAMETER, FAN_SET_COMMAND, fan_pwm_of_percent, percent_of_multiplier,
};
use crate::gcode::ExtrudedTo;
use crate::http::{self, Method, Response, Sink, Transport};
use crate::wire::{FilePayload, JobPayload, ListingPayload, PrinterPayload, TOOL_PREFIX};

/// Reading the printer.
const PRINTER_PATH: &str = "/api/printer?history=false";
/// Reading the job.
const JOB_PATH: &str = "/api/job";
/// Acting on the job: starting, pausing, resuming and cancelling.
const JOB_COMMAND_PATH: &str = "/api/job";
/// Acting on the print head, which is where the feedrate factor lives.
const PRINTHEAD_PATH: &str = "/api/printer/printhead";
/// Acting on a tool: its target temperature, and the flowrate factor.
const TOOL_PATH: &str = "/api/printer/tool";
/// Acting on the bed.
const BED_PATH: &str = "/api/printer/bed";
/// Sending one of this crate's own commands, which is how the fan is reached.
const COMMAND_PATH: &str = "/api/printer/command";
/// Selecting, and reading, a file of the instance's own storage.
const LOCAL_FILE_PATH: &str = "/api/files/local/";
/// Listing every file of every origin, folders and all.
const FILES_PATH: &str = "/api/files?recursive=true";

/// How much of an instance's own words an error carries.
///
/// `OctoPrint` answers a refusal with one line, but a proxy in front of it can
/// answer a whole page, and an error nobody can read is one nobody acts on.
const DETAIL_LIMIT: usize = 512;

/// The `OctoPrint` adapter: the printer port, spoken to one instance.
#[derive(Debug, Clone)]
pub struct OctoPrintPrinter {
    /// Where the instance is, and what authenticates against it.
    config: OctoPrintConfig,
}

impl OctoPrintPrinter {
    /// The adapter for the instance this configuration names.
    #[must_use]
    pub const fn new(config: OctoPrintConfig) -> Self {
        Self { config }
    }

    /// The configuration this adapter was built from.
    #[must_use]
    pub const fn config(&self) -> &OctoPrintConfig {
        &self.config
    }

    /// Send one request and rule on what came back.
    fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Vec<u8>, PrinterError> {
        let encoded = body.map(|value| value.to_string());
        let outcome = http::send(
            self.config.endpoint(),
            self.config.api_key(),
            method,
            &self.config.endpoint().path(path),
            encoded.as_deref().map(str::as_bytes),
            self.config.timeout(),
        );
        match outcome {
            Ok(response) => ruled(method, path, response),
            Err(transport) => {
                let error = match transport {
                    Transport::Unreachable(detail) => PrinterError::Unreachable { detail },
                    Transport::Malformed(detail) => PrinterError::Malformed { detail },
                };
                log::warn!(
                    "octoprint {} {path} did not answer: {error}",
                    method.as_str()
                );
                Err(error)
            }
        }
    }

    /// Read the printer, as this adapter's own snapshot.
    fn read_printer(&self) -> Result<PrinterSnapshot, PrinterError> {
        let body = self.request(Method::Get, PRINTER_PATH, None)?;
        let payload: PrinterPayload = decode(&body, PRINTER_PATH)?;
        Ok(payload.snapshot(Timestamp::now()))
    }

    /// Read the job, as this adapter's own snapshot.
    fn read_job(&self) -> Result<JobSnapshot, PrinterError> {
        let body = self.request(Method::Get, JOB_PATH, None)?;
        let payload: JobPayload = decode(&body, JOB_PATH)?;
        Ok(payload.snapshot())
    }

    /// Read one file of the instance's own storage, as `OctoPrint` answers it.
    fn read_file(&self, file_name: &FileName) -> Result<FilePayload, PrinterError> {
        let path = local_file_path(file_name);
        let body = self.request(Method::Get, &path, None)?;
        decode(&body, &path)
    }

    /// Read every file of every origin that carries a print tally.
    fn read_printed_files(&self) -> Result<Vec<FileFacts>, PrinterError> {
        let body = self.request(Method::Get, FILES_PATH, None)?;
        let listing: ListingPayload = decode(&body, FILES_PATH)?;
        listing
            .printed()
            .map_err(|detail| malformed(format!("{FILES_PATH} answered {detail}")))
    }

    /// Read the net filament one file's G-code feeds each tool, up to a byte.
    fn read_extruded_to(
        &self,
        file_name: &FileName,
        position_bytes: i64,
    ) -> Result<Vec<ToolFilament>, PrinterError> {
        let file = self.read_file(file_name)?;
        let link = file.download().map(ToOwned::to_owned);
        // The record is held to what `file_facts` holds it to before its link
        // is followed: a folder, or a record no file could carry, is no file
        // to download.
        file.facts().map_err(|detail| {
            malformed(format!("{} answered {detail}", local_file_path(file_name)))
        })?;
        let link = link.ok_or_else(|| {
            malformed(format!(
                "{} answered no download link for {file_name}",
                local_file_path(file_name)
            ))
        })?;
        let path = download_path(&link)?;
        let mut reader = Reader(ExtrudedTo::new(position_bytes));
        let outcome = http::stream(
            self.config.endpoint(),
            self.config.api_key(),
            &path,
            self.config.timeout(),
            &mut reader,
        );
        match outcome {
            Ok(None) => Ok(reader.0.fed()),
            Ok(Some(response)) => ruled(Method::Get, &path, response).map(|_| reader.0.fed()),
            Err(transport) => {
                let error = match transport {
                    Transport::Unreachable(detail) => PrinterError::Unreachable { detail },
                    Transport::Malformed(detail) => PrinterError::Malformed { detail },
                };
                log::warn!("octoprint GET {path} did not answer: {error}");
                Err(error)
            }
        }
    }

    /// Select the named file, then start it.
    fn start_file(&self, file_name: &FileName) -> Result<(), PrinterError> {
        let path = local_file_path(file_name);
        self.request(
            Method::Post,
            &path,
            Some(json!({ "command": "select", "print": false })),
        )?;
        self.request(
            Method::Post,
            JOB_COMMAND_PATH,
            Some(json!({ "command": "start" })),
        )
        .map(|_| ())
    }

    /// Ask the job endpoint for one of the four things it does.
    fn job_command(&self, body: serde_json::Value) -> Result<(), PrinterError> {
        self.request(Method::Post, JOB_COMMAND_PATH, Some(body))
            .map(|_| ())
    }

    /// Send the fan command set's one member, carrying a duty and nothing else.
    fn command_fan(&self, percent: f64) -> Result<(), PrinterError> {
        if self.config.fan() == FanSupport::Absent {
            let error = PrinterError::Unsupported {
                adjustable: Adjustable::Fan,
            };
            log::warn!("octoprint will not command the fan: {error}");
            return Err(error);
        }
        let mut parameters = serde_json::Map::new();
        parameters.insert(
            FAN_PWM_PARAMETER.to_owned(),
            json!(fan_pwm_of_percent(percent)),
        );
        self.request(
            Method::Post,
            COMMAND_PATH,
            Some(json!({ "command": FAN_SET_COMMAND, "parameters": parameters })),
        )
        .map(|_| ())
    }
}

/// The G-code reader, as the sink a download is streamed into.
struct Reader(ExtrudedTo);

impl Sink for Reader {
    fn take(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        self.0.take(bytes)
    }

    fn end(&mut self) {
        self.0.end();
    }
}

/// The path one file of the instance's own storage is read and selected at.
fn local_file_path(file_name: &FileName) -> String {
    format!("{LOCAL_FILE_PATH}{}", encode_segment(file_name.as_str()))
}

/// The path of the download link `OctoPrint` answered, to send to the
/// configured instance.
///
/// `OctoPrint` spells the link absolute, with whatever host the request reached
/// it by, so its scheme and authority are dropped and the path alone is kept:
/// the key goes to the instance the configuration names and nowhere else. A
/// path that would change the request it lands in is refused rather than sent.
fn download_path(link: &str) -> Result<String, PrinterError> {
    let path = match link.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("/", |start| &rest[start..]),
        None => link,
    };
    if !path.starts_with('/')
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return Err(malformed(format!(
            "{link:?} is no download link this port can follow"
        )));
    }
    Ok(path.to_owned())
}

/// A malformed answer, logged.
fn malformed(detail: String) -> PrinterError {
    let error = PrinterError::Malformed { detail };
    log::warn!("octoprint {error}");
    error
}

/// What one status means, in the vocabulary the layer above acts on.
fn ruled(method: Method, path: &str, response: Response) -> Result<Vec<u8>, PrinterError> {
    let status = response.status;
    log::debug!("octoprint {} {path} answered {status}", method.as_str());
    if (200..300).contains(&status) {
        return Ok(response.body);
    }
    let detail = detail_of(&response);
    let error = match status {
        401 | 403 => PrinterError::Unauthorized { detail },
        409 => PrinterError::StateConflict { detail },
        _ => PrinterError::Refused { status, detail },
    };
    log::warn!("octoprint {} {path} refused: {error}", method.as_str());
    Err(error)
}

/// One `OctoPrint` body, as the payload this adapter reads it into.
fn decode<T: serde::de::DeserializeOwned>(body: &[u8], path: &str) -> Result<T, PrinterError> {
    serde_json::from_slice(body).map_err(|error| {
        malformed(format!(
            "{path} answered something this port could not read: {error}"
        ))
    })
}

/// What the instance said, bounded so an error stays legible.
fn detail_of(response: &Response) -> String {
    let text = response.text();
    match text.char_indices().nth(DETAIL_LIMIT) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text,
    }
}

/// One path segment, with everything outside the unreserved set escaped.
///
/// `FileName` already refuses a separator and a NUL, so this is not what keeps a
/// name inside its segment; it is what keeps a name that is legal as a name from
/// changing the request it lands in.
fn encode_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("a String never fails to be written to");
        }
    }
    encoded
}

impl PrinterPort for OctoPrintPrinter {
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>> {
        Box::pin(async move { self.read_printer() })
    }

    fn job(&self) -> BoxFuture<'_, Result<JobSnapshot, PrinterError>> {
        Box::pin(async move { self.read_job() })
    }

    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move { self.start_file(&file_name) })
    }

    fn pause(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move { self.job_command(json!({ "command": "pause", "action": "pause" })) })
    }

    fn resume(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move { self.job_command(json!({ "command": "pause", "action": "resume" })) })
    }

    fn cancel(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move { self.job_command(json!({ "command": "cancel" })) })
    }

    fn set_feedrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move {
            self.request(
                Method::Post,
                PRINTHEAD_PATH,
                Some(json!({
                    "command": "feedrate",
                    "factor": percent_of_multiplier(factor),
                })),
            )
            .map(|_| ())
        })
    }

    fn set_flowrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move {
            self.request(
                Method::Post,
                TOOL_PATH,
                Some(json!({
                    "command": "flowrate",
                    "factor": percent_of_multiplier(factor),
                })),
            )
            .map(|_| ())
        })
    }

    fn set_tool_target_c(
        &self,
        tool: i64,
        target_c: f64,
    ) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move {
            let mut targets = serde_json::Map::new();
            targets.insert(format!("{TOOL_PREFIX}{tool}"), json!(target_c));
            self.request(
                Method::Post,
                TOOL_PATH,
                Some(json!({ "command": "target", "targets": targets })),
            )
            .map(|_| ())
        })
    }

    fn set_bed_target_c(&self, target_c: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move {
            self.request(
                Method::Post,
                BED_PATH,
                Some(json!({ "command": "target", "target": target_c })),
            )
            .map(|_| ())
        })
    }

    fn set_fan_percent(&self, percent: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async move { self.command_fan(percent) })
    }

    fn file_facts(&self, file_name: FileName) -> BoxFuture<'_, Result<FileFacts, PrinterError>> {
        Box::pin(async move {
            self.read_file(&file_name)?.facts().map_err(|detail| {
                malformed(format!("{} answered {detail}", local_file_path(&file_name)))
            })
        })
    }

    fn printed_files(&self) -> BoxFuture<'_, Result<Vec<FileFacts>, PrinterError>> {
        Box::pin(async move { self.read_printed_files() })
    }

    fn extruded_to(
        &self,
        file_name: FileName,
        position_bytes: i64,
    ) -> BoxFuture<'_, Result<Vec<ToolFilament>, PrinterError>> {
        Box::pin(async move { self.read_extruded_to(&file_name, position_bytes) })
    }
}
