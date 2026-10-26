//! A machine these journeys stand in for, and the state it holds.
//!
//! The layer under test here is the server: its API, the policy it puts every
//! action through, its record and its ingress. The printer is the external
//! system on the far side of a port, and this is what stands in for one — it
//! holds the state a printer holds, moves that state when an action reaches it,
//! and remembers what it was asked, so a journey can assert an operation's own
//! effect rather than its status code.
//!
//! It holds the files a printer holds too — each with the analysis `OctoPrint`
//! makes of it and the tally of how often it was printed — and answers how far
//! a file's own moves fed each tool up to a byte of it, so a journey can tell
//! the story the filament ledger is charged from: a file printed while nothing
//! was watching, a tally reset by a re-upload, a print cancelled part-way.
//!
//! It is not a substitute for the real thing. What proves the same operations
//! against a real `OctoPrint` is this crate's integration tier, which runs
//! against the instance `just octoprint-up` starts; the two tiers walk the same
//! declared operation list.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// How long a job read waits for the others it was told to meet before going
/// on alone.
const PATIENCE: Duration = Duration::from_millis(400);

use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_printer_api::{
    BoxFuture, FileFacts, FilePrint, HeaterSnapshot, JobSnapshot, PrinterError, PrinterPort,
    PrinterSnapshot, ToolFilament,
};
use printobserver_types::{FileName, Reported, Timestamp};

/// One thing the machine was asked to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// Pause the print.
    Pause,
    /// Resume the print.
    Resume,
    /// Cancel the print.
    Cancel,
    /// Start a print of a named file.
    Start(FileName),
    /// Set the feedrate factor.
    Feedrate(f64),
    /// Set the flowrate factor.
    Flowrate(f64),
    /// Set one tool's target temperature.
    ToolTarget(i64, f64),
    /// Set the bed's target temperature.
    BedTarget(f64),
    /// Set the fan percentage.
    Fan(f64),
}

/// What the machine holds, and what it has been asked.
#[derive(Debug)]
struct Held {
    /// The printer's own state.
    snapshot: PrinterSnapshot,
    /// The job it reports it is running.
    job: JobSnapshot,
    /// Everything it has been asked, in order.
    calls: Vec<Call>,
    /// Whether it answers no read at all, as a machine switched off does.
    unreadable: bool,
    /// The files it holds, with their analysis and their print tallies.
    files: Vec<FileFacts>,
}

/// A printer these journeys drive the server against.
#[derive(Debug)]
pub struct RecordingPrinter {
    /// What it holds.
    held: Mutex<Held>,
    /// How many job reads are to wait for one another, and how many have come.
    meeting: Mutex<(usize, usize)>,
    /// Signalled whenever a job read arrives.
    arrived: Condvar,
    /// How long a start takes to answer, after which the machine reports the
    /// job it names running, as `OctoPrint` does by the time its start answers;
    /// absent, a start answers at once and the job is the journey's to run.
    starting: Mutex<Option<Duration>>,
}

/// One feedrate factor, flagged against the range the contracts declare for it.
fn feedrate(value: f64) -> Reported<f64> {
    Reported::new(value, printobserver_printer_api::FEEDRATE_FACTOR_RANGE)
}

impl RecordingPrinter {
    /// A printer part-way through a print, at the values a print starts at.
    #[must_use]
    pub fn printing() -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::new(Held {
                snapshot: PrinterSnapshot {
                    connection: PrinterState::Printing,
                    tools: vec![HeaterSnapshot {
                        actual_c: Some(Reported::new(
                            215.0,
                            printobserver_printer_api::HEATER_ACTUAL_C_RANGE,
                        )),
                        target_c: Some(Reported::new(
                            215.0,
                            printobserver_printer_api::HEATER_TARGET_C_RANGE,
                        )),
                        offset_c: None,
                    }],
                    bed: Some(HeaterSnapshot {
                        actual_c: Some(Reported::new(
                            60.0,
                            printobserver_printer_api::HEATER_ACTUAL_C_RANGE,
                        )),
                        target_c: Some(Reported::new(
                            60.0,
                            printobserver_printer_api::HEATER_TARGET_C_RANGE,
                        )),
                        offset_c: None,
                    }),
                    chamber: None,
                    feedrate_factor: Some(feedrate(1.0)),
                    flowrate_factor: Some(Reported::new(
                        1.0,
                        printobserver_printer_api::FLOWRATE_FACTOR_RANGE,
                    )),
                    fan_percent: Some(Reported::new(
                        40.0,
                        printobserver_printer_api::FAN_PERCENT_RANGE,
                    )),
                    observed_at: Timestamp::now(),
                },
                job: JobSnapshot {
                    file_name: Some("benchy.gcode".to_owned()),
                    file_origin: Some("local".to_owned()),
                    size_bytes: Some(4096),
                    estimated_print_time_s: Some(3600),
                    completion: Some(Reported::new(
                        0.25,
                        printobserver_printer_api::COMPLETION_RANGE,
                    )),
                    print_time_s: Some(900),
                    print_time_left_s: Some(2700),
                    state: PrinterState::Printing,
                    error: None,
                    error_report: None,
                    file_position_bytes: None,
                    filament: Vec::new(),
                },
                calls: Vec::new(),
                unreadable: false,
                files: Vec::new(),
            }),
            meeting: Mutex::new((0, 0)),
            arrived: Condvar::new(),
            starting: Mutex::new(None),
        })
    }

    /// Make this many job reads wait for one another before any answers.
    ///
    /// Two callers nothing keeps apart are made to overlap at the read, and two
    /// that something does keep apart each wait a moment alone and go on.
    pub fn job_reads_meet(&self, callers: usize) {
        *self.meeting.lock().expect("the meeting is not poisoned") = (callers, 0);
    }

    /// Arrive at the job read's meeting, and wait for the others when one is set.
    fn meet(&self) {
        let mut counts = self.meeting.lock().expect("the meeting is not poisoned");
        if counts.0 == 0 {
            return;
        }
        counts.1 += 1;
        self.arrived.notify_all();
        let _ = self
            .arrived
            .wait_timeout_while(counts, PATIENCE, |(wanted, came)| *came < *wanted)
            .expect("the meeting is not poisoned");
    }

    /// Take `delay` to answer every start, and run the job it names once it has.
    ///
    /// What a journey about a read made while a start is in flight needs: the
    /// machine is idle until the start answers and running the job after.
    pub fn starts_jobs_after(&self, delay: Duration) {
        *self.starting.lock().expect("the start is not poisoned") = Some(delay);
    }

    /// A switched-off machine: while set, both its snapshot and its job are
    /// refused as unreachable, which is how a listing is shown to answer anyway.
    pub fn unreadable(&self, unreadable: bool) {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .unreadable = unreadable;
    }

    /// The refusal a machine nothing can read answers, when it is one.
    fn unread(&self) -> Option<PrinterError> {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .unreadable
            .then(|| PrinterError::Unreachable {
                detail: "the machine is switched off".to_owned(),
            })
    }

    /// Hold one file, `size_bytes` long, whose analysis says each tool feeds
    /// `filament` — or, given none, that has no analysis yet — replacing any
    /// file of that name, tally and all, as a re-upload does.
    pub fn holds_file(&self, name: &str, size_bytes: i64, filament: Option<Vec<ToolFilament>>) {
        let mut held = self.held.lock().expect("the machine is not poisoned");
        held.files.retain(|file| file.file_name != name);
        held.files.push(FileFacts {
            file_name: name.to_owned(),
            origin: "local".to_owned(),
            size_bytes: Some(size_bytes),
            filament,
            successes: 0,
            failures: 0,
            last_print: None,
        });
    }

    /// Count one finished print of a held file in its tally, at `at`.
    ///
    /// # Panics
    ///
    /// Panics when it holds no such file.
    pub fn counts_print(&self, name: &str, success: bool, at: Timestamp) {
        let mut held = self.held.lock().expect("the machine is not poisoned");
        let file = held
            .files
            .iter_mut()
            .find(|file| file.file_name == name)
            .unwrap_or_else(|| panic!("the machine holds no `{name}`"));
        if success {
            file.successes += 1;
        } else {
            file.failures += 1;
        }
        file.last_print = Some(FilePrint { at, success });
    }

    /// Report the job at this byte of its file, as a print part-way through
    /// it does.
    pub fn at_byte(&self, position_bytes: i64) {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .job
            .file_position_bytes = Some(position_bytes);
    }

    /// Report the job with no position in its file, as a job read taken while
    /// the printer is busy with a filament change can.
    pub fn reports_no_position(&self) {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .job
            .file_position_bytes = None;
    }

    /// Everything it has been asked, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<Call> {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .calls
            .clone()
    }

    /// Forget what it has been asked, so a journey's next step starts clean.
    pub fn forget(&self) {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .calls
            .clear();
    }

    /// The state it holds now.
    #[must_use]
    pub fn snapshot(&self) -> PrinterSnapshot {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .snapshot
            .clone()
    }

    /// Put it in one state, which is what a journey about a state conflict needs.
    pub fn in_state(&self, state: PrinterState) {
        let mut held = self.held.lock().expect("the machine is not poisoned");
        held.snapshot.connection = state.clone();
        held.job.state = state;
    }

    /// Report `state` as the printer's own while its job goes on reporting
    /// what it did, as a firmware that faulted under a host still reporting
    /// the job does.
    pub fn reports_connection(&self, state: PrinterState) {
        self.held
            .lock()
            .expect("the machine is not poisoned")
            .snapshot
            .connection = state;
    }

    /// Run `file` in `state`, reporting `print_time_s` as its running time.
    ///
    /// The running time is what tells one job of a file from another, so this
    /// is how a journey stands a later job, a pause or a resume up.
    pub fn runs_job(&self, file: &str, state: PrinterState, print_time_s: Option<i64>) {
        let mut held = self.held.lock().expect("the machine is not poisoned");
        held.snapshot.connection = state.clone();
        held.job.file_name = Some(file.to_owned());
        held.job.state = state;
        held.job.print_time_s = print_time_s;
    }

    /// Make it report a value JSON denotes no spelling of.
    ///
    /// A disconnected thermistor and a division by a zero-valued reading both
    /// produce one, and the contracts refuse to emit one rather than writing
    /// `null` for it — so this is what a machine having that kind of bad day
    /// looks like from here.
    pub fn reporting_nothing_denotable(&self) {
        let mut held = self.held.lock().expect("the machine is not poisoned");
        held.snapshot.feedrate_factor = Some(feedrate(f64::NAN));
    }

    /// What one adjustable reads as now, when the machine reports it.
    #[must_use]
    pub fn value_of(&self, adjustable: Adjustable) -> Option<f64> {
        let snapshot = self.snapshot();
        let reported = match adjustable {
            Adjustable::Feedrate => snapshot.feedrate_factor,
            Adjustable::Flowrate => snapshot.flowrate_factor,
            Adjustable::Fan => snapshot.fan_percent,
            Adjustable::BedTarget => snapshot.bed.as_ref().and_then(|heater| heater.target_c),
            Adjustable::ToolTarget { tool } => usize::try_from(tool)
                .ok()
                .and_then(|index| snapshot.tools.get(index))
                .and_then(|heater| heater.target_c),
        };
        reported.map(|value| value.value())
    }

    /// Record one call, and move what it changes.
    fn took(&self, call: Call) {
        let mut held = self.held.lock().expect("the machine is not poisoned");
        match &call {
            Call::Pause => held.snapshot.connection = PrinterState::Paused,
            Call::Resume => held.snapshot.connection = PrinterState::Printing,
            Call::Cancel => held.snapshot.connection = PrinterState::Cancelling,
            Call::Start(name) => {
                held.snapshot.connection = PrinterState::Printing;
                held.job.file_name = Some(name.as_str().to_owned());
            }
            Call::Feedrate(factor) => {
                held.snapshot.feedrate_factor = Some(Reported::new(
                    *factor,
                    printobserver_printer_api::FEEDRATE_FACTOR_RANGE,
                ));
            }
            Call::Flowrate(factor) => {
                held.snapshot.flowrate_factor = Some(Reported::new(
                    *factor,
                    printobserver_printer_api::FLOWRATE_FACTOR_RANGE,
                ));
            }
            Call::ToolTarget(tool, target) => {
                if let Some(heater) = usize::try_from(*tool)
                    .ok()
                    .and_then(|index| held.snapshot.tools.get_mut(index))
                {
                    heater.target_c = Some(Reported::new(
                        *target,
                        printobserver_printer_api::HEATER_TARGET_C_RANGE,
                    ));
                }
            }
            Call::BedTarget(target) => {
                if let Some(bed) = held.snapshot.bed.as_mut() {
                    bed.target_c = Some(Reported::new(
                        *target,
                        printobserver_printer_api::HEATER_TARGET_C_RANGE,
                    ));
                }
            }
            Call::Fan(percent) => {
                held.snapshot.fan_percent = Some(Reported::new(
                    *percent,
                    printobserver_printer_api::FAN_PERCENT_RANGE,
                ));
            }
        }
        held.calls.push(call);
    }
}

impl PrinterPort for RecordingPrinter {
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>> {
        let taken = self.unread().map_or_else(|| Ok(Self::snapshot(self)), Err);
        Box::pin(async move { taken })
    }

    fn job(&self) -> BoxFuture<'_, Result<JobSnapshot, PrinterError>> {
        self.meet();
        let job = self.unread().map_or_else(
            || {
                Ok(self
                    .held
                    .lock()
                    .expect("the machine is not poisoned")
                    .job
                    .clone())
            },
            Err,
        );
        Box::pin(async move { job })
    }

    fn file_facts(&self, file_name: FileName) -> BoxFuture<'_, Result<FileFacts, PrinterError>> {
        let found = self.unread().map_or_else(
            || {
                self.held
                    .lock()
                    .expect("the machine is not poisoned")
                    .files
                    .iter()
                    .find(|file| file.file_name == file_name.as_str())
                    .cloned()
                    .ok_or_else(|| PrinterError::Refused {
                        status: 404,
                        detail: format!("there is no file {file_name}"),
                    })
            },
            Err,
        );
        Box::pin(async move { found })
    }

    fn printed_files(&self) -> BoxFuture<'_, Result<Vec<FileFacts>, PrinterError>> {
        let files = self.unread().map_or_else(
            || {
                Ok(self
                    .held
                    .lock()
                    .expect("the machine is not poisoned")
                    .files
                    .clone())
            },
            Err,
        );
        Box::pin(async move { files })
    }

    /// The file's analysed feed, in proportion to how far into its bytes the
    /// position is: a file whose moves are spread evenly through it.
    fn extruded_to(
        &self,
        file_name: FileName,
        position_bytes: i64,
    ) -> BoxFuture<'_, Result<Vec<ToolFilament>, PrinterError>> {
        let fed = self.unread().map_or_else(
            || {
                let held = self.held.lock().expect("the machine is not poisoned");
                let file = held
                    .files
                    .iter()
                    .find(|file| file.file_name == file_name.as_str())
                    .ok_or_else(|| PrinterError::Refused {
                        status: 404,
                        detail: format!("there is no file {file_name}"),
                    })?;
                // A test file is far shorter than a `u32` counts, so the
                // conversions lose nothing.
                let size = file.size_bytes.unwrap_or(1).max(1);
                let bytes = |count: i64| f64::from(u32::try_from(count).unwrap_or(u32::MAX));
                let fraction = bytes(position_bytes.clamp(0, size)) / bytes(size);
                Ok(file
                    .filament
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|tool| ToolFilament {
                        length_mm: tool.length_mm * fraction,
                        volume_cm3: None,
                        ..tool
                    })
                    .collect())
            },
            Err,
        );
        Box::pin(async move { fed })
    }

    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Start(file_name.clone()));
        let delay = *self.starting.lock().expect("the start is not poisoned");
        Box::pin(async move {
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
                self.runs_job(file_name.as_str(), PrinterState::Printing, Some(0));
            }
            Ok(())
        })
    }

    fn pause(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Pause);
        Box::pin(async move { Ok(()) })
    }

    fn resume(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Resume);
        Box::pin(async move { Ok(()) })
    }

    fn cancel(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Cancel);
        Box::pin(async move { Ok(()) })
    }

    fn set_feedrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Feedrate(factor));
        Box::pin(async move { Ok(()) })
    }

    fn set_flowrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Flowrate(factor));
        Box::pin(async move { Ok(()) })
    }

    fn set_tool_target_c(
        &self,
        tool: i64,
        target_c: f64,
    ) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::ToolTarget(tool, target_c));
        Box::pin(async move { Ok(()) })
    }

    fn set_bed_target_c(&self, target_c: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::BedTarget(target_c));
        Box::pin(async move { Ok(()) })
    }

    fn set_fan_percent(&self, percent: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Fan(percent));
        Box::pin(async move { Ok(()) })
    }
}
