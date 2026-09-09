//! A machine these journeys stand in for, and the state it holds.
//!
//! The layer under test here is the server: its API, the policy it puts every
//! action through, its record and its ingress. The printer is the external
//! system on the far side of a port, and this is what stands in for one — it
//! holds the state a printer holds, moves that state when an action reaches it,
//! and remembers what it was asked, so a journey can assert an operation's own
//! effect rather than its status code.
//!
//! It is not a substitute for the real thing. What proves the same operations
//! against a real `OctoPrint` is this crate's integration tier, which runs
//! against the instance `just octoprint-up` starts; the two tiers walk the same
//! declared operation list.

use std::sync::{Arc, Mutex};

use printobserver_printer_api::{BoxFuture, PrinterError, PrinterPort};
use printobserver_types::{
    Adjustable, FileName, HeaterSnapshot, JobSnapshot, PrinterSnapshot, PrinterState, Reported,
    Timestamp,
};

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
}

/// A printer these journeys drive the server against.
#[derive(Debug)]
pub struct RecordingPrinter {
    /// What it holds.
    held: Mutex<Held>,
}

/// One feedrate factor, flagged against the range the contracts declare for it.
fn feedrate(value: f64) -> Reported<f64> {
    Reported::new(value, printobserver_types::FEEDRATE_FACTOR_RANGE)
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
                            printobserver_types::HEATER_ACTUAL_C_RANGE,
                        )),
                        target_c: Some(Reported::new(
                            215.0,
                            printobserver_types::HEATER_TARGET_C_RANGE,
                        )),
                        offset_c: None,
                    }],
                    bed: Some(HeaterSnapshot {
                        actual_c: Some(Reported::new(
                            60.0,
                            printobserver_types::HEATER_ACTUAL_C_RANGE,
                        )),
                        target_c: Some(Reported::new(
                            60.0,
                            printobserver_types::HEATER_TARGET_C_RANGE,
                        )),
                        offset_c: None,
                    }),
                    chamber: None,
                    feedrate_factor: Some(feedrate(1.0)),
                    flowrate_factor: Some(Reported::new(
                        1.0,
                        printobserver_types::FLOWRATE_FACTOR_RANGE,
                    )),
                    fan_percent: Some(Reported::new(40.0, printobserver_types::FAN_PERCENT_RANGE)),
                    observed_at: Timestamp::now(),
                },
                job: JobSnapshot {
                    file_name: Some("benchy.gcode".to_owned()),
                    file_origin: Some("local".to_owned()),
                    size_bytes: Some(4096),
                    estimated_print_time_s: Some(3600),
                    completion: Some(Reported::new(0.25, printobserver_types::COMPLETION_RANGE)),
                    print_time_s: Some(900),
                    print_time_left_s: Some(2700),
                    state: PrinterState::Printing,
                    error: None,
                },
                calls: Vec::new(),
            }),
        })
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
                    printobserver_types::FEEDRATE_FACTOR_RANGE,
                ));
            }
            Call::Flowrate(factor) => {
                held.snapshot.flowrate_factor = Some(Reported::new(
                    *factor,
                    printobserver_types::FLOWRATE_FACTOR_RANGE,
                ));
            }
            Call::ToolTarget(tool, target) => {
                if let Some(heater) = usize::try_from(*tool)
                    .ok()
                    .and_then(|index| held.snapshot.tools.get_mut(index))
                {
                    heater.target_c = Some(Reported::new(
                        *target,
                        printobserver_types::HEATER_TARGET_C_RANGE,
                    ));
                }
            }
            Call::BedTarget(target) => {
                if let Some(bed) = held.snapshot.bed.as_mut() {
                    bed.target_c = Some(Reported::new(
                        *target,
                        printobserver_types::HEATER_TARGET_C_RANGE,
                    ));
                }
            }
            Call::Fan(percent) => {
                held.snapshot.fan_percent = Some(Reported::new(
                    *percent,
                    printobserver_types::FAN_PERCENT_RANGE,
                ));
            }
        }
        held.calls.push(call);
    }
}

impl PrinterPort for RecordingPrinter {
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>> {
        let taken = Self::snapshot(self);
        Box::pin(async move { Ok(taken) })
    }

    fn job(&self) -> BoxFuture<'_, Result<JobSnapshot, PrinterError>> {
        let job = self
            .held
            .lock()
            .expect("the machine is not poisoned")
            .job
            .clone();
        Box::pin(async move { Ok(job) })
    }

    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.took(Call::Start(file_name));
        Box::pin(async move { Ok(()) })
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
