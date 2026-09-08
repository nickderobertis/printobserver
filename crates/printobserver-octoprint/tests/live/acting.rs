//! Every action the printer port declares, against a real printer.
//!
//! The walk is over the port's own action methods, and each is confirmed at an
//! observable that action actually has. Six of them — start, pause, resume,
//! cancel, tool target and bed target — are confirmed by a snapshot read back
//! afterwards. Three of them are not, and that narrowing is deliberate:
//! `OctoPrint` reports neither an applied feedrate factor nor an applied
//! flowrate factor nor a fan setting, and its setting endpoints answer with no
//! content, so there is no read-back to have. Those three are confirmed instead
//! by what the instance received, through a proxy that records every request and
//! then forwards it to the real instance — which then rules on it, so a request
//! `OctoPrint` would have refused cannot pass here.
//!
//! The expected side of the two factor pins is a literal written into this file
//! rather than something derived from the conversion under test. That is the
//! whole point of pinning them here: a conversion wrong in the same way in both
//! directions round-trips perfectly through this port while commanding the wrong
//! physical factor, and only a number written down independently catches it.

use std::time::Duration;

use printobserver_octoprint::{COMMAND_SET, FAN_PWM_PARAMETER};
use printobserver_printer_api::{PrinterError, PrinterPort};
use printobserver_types::{Adjustable, FileName, PrinterState};

use crate::block_on::block_on;
use crate::env::Scripted;
use crate::proxy::Proxy;
use crate::received::Recorded;
use crate::wait;

/// The file the scripted environment uploads and this walk starts.
const HOLD_FILE: &str = "hold.gcode";

/// How long an action has to reach a machine that dwells ten seconds at a time.
const REACHED: Duration = Duration::from_secs(90);

/// The hold print is running, whatever state this environment was left in.
///
/// `just octoprint-up` starts one and answers only once it is running, and the
/// tier asserts that it did — but the tier also cancels and restarts prints, so
/// a second run against one environment finds it where the first left it. This
/// puts it back, through the port's own `start`, which the walk below proves
/// separately.
///
/// # Panics
///
/// Panics when the print cannot be started.
pub fn ensure_the_hold_print_is_running(instance: &Scripted) {
    let printer = instance.printer();
    if block_on(printer.job()).expect("a job snapshot").state == PrinterState::Printing {
        return;
    }
    block_on(printer.start(FileName::new(HOLD_FILE).expect("a file name")))
        .expect("the hold print starts");
    wait_for_job(
        instance,
        &PrinterState::Printing,
        "the hold print to be running",
    );
}

/// Every action method the printer port declares, against a real printer.
///
/// # Panics
///
/// Panics when an action is not confirmed at its own observable.
pub fn walk(instance: &Scripted) {
    let proxy = Proxy::in_front_of(&instance.url);
    let printer = instance.printer_at(&proxy.base_url());

    // Whatever the environment started, so that this walk starts its own.
    block_on(printer.cancel()).expect("the running print is cancelled");
    wait_for_job(
        instance,
        &PrinterState::Operational,
        "the print to be cancelled",
    );

    // start — confirmed by the job snapshot naming the file that is running.
    block_on(printer.start(FileName::new(HOLD_FILE).expect("a file name")))
        .expect("the print starts");
    wait::until("the started file to be printing", REACHED, || {
        let job = block_on(printer.job()).map_err(|error| error.to_string())?;
        if job.state == PrinterState::Printing && job.file_name.as_deref() == Some(HOLD_FILE) {
            Ok(())
        } else {
            Err(format!("{:?} printing {:?}", job.state, job.file_name))
        }
    });

    // pause — confirmed by the printer snapshot.
    block_on(printer.pause()).expect("the print pauses");
    wait_for_printer(&printer, &PrinterState::Paused, "the printer to be paused");

    // resume — confirmed by the printer snapshot.
    block_on(printer.resume()).expect("the print resumes");
    wait_for_printer(
        &printer,
        &PrinterState::Printing,
        "the printer to be printing",
    );

    pin_the_feedrate(&proxy, &printer);
    pin_the_flowrate(&proxy, &printer);
    confirm_the_tool_target(instance, &printer);
    confirm_the_bed_target(instance, &printer);
    pin_the_fan(&proxy, &printer);
    refuse_the_fan_where_there_is_none(instance);

    // cancel — confirmed by the job snapshot.
    block_on(printer.cancel()).expect("the print cancels");
    wait_for_job(
        instance,
        &PrinterState::Operational,
        "the print to be cancelled",
    );
}

/// The feedrate factor, in `OctoPrint`'s own units, at two distinct multipliers.
fn pin_the_feedrate(proxy: &Proxy, printer: &impl PrinterPort) {
    for (multiplier, expected) in [(1.5_f64, 150_i64), (0.8, 80)] {
        proxy.forget();
        block_on(printer.set_feedrate_factor(multiplier)).expect("the feedrate is accepted");
        let request = only(proxy, "the feedrate");
        assert_eq!(request.path, "/api/printer/printhead");
        assert_eq!(
            request.json()["factor"],
            serde_json::json!(expected),
            "a multiplier of {multiplier} is {expected} percent in OctoPrint's own units"
        );
    }
}

/// The flowrate factor, in `OctoPrint`'s own units, at two distinct multipliers.
fn pin_the_flowrate(proxy: &Proxy, printer: &impl PrinterPort) {
    for (multiplier, expected) in [(1.1_f64, 110_i64), (0.9, 90)] {
        proxy.forget();
        block_on(printer.set_flowrate_factor(multiplier)).expect("the flowrate is accepted");
        let request = only(proxy, "the flowrate");
        assert_eq!(request.path, "/api/printer/tool");
        assert_eq!(
            request.json()["factor"],
            serde_json::json!(expected),
            "a multiplier of {multiplier} is {expected} percent in OctoPrint's own units"
        );
    }
}

/// The tool's target, confirmed by the printer snapshot reporting it.
fn confirm_the_tool_target(instance: &Scripted, printer: &impl PrinterPort) {
    block_on(printer.set_tool_target_c(0, 205.0)).expect("the tool target is accepted");
    wait::until(
        "the tool to report the target it was given",
        REACHED,
        || {
            let snapshot = block_on(instance.printer().snapshot()).map_err(|e| e.to_string())?;
            let target = snapshot
                .tools
                .first()
                .and_then(|tool| tool.target_c)
                .map(|reported| reported.value());
            if target == Some(205.0) {
                Ok(())
            } else {
                Err(format!("a target of {target:?}"))
            }
        },
    );
    block_on(printer.set_tool_target_c(0, 0.0)).expect("the tool is turned back off");
}

/// The bed's target, confirmed by the printer snapshot reporting it.
fn confirm_the_bed_target(instance: &Scripted, printer: &impl PrinterPort) {
    block_on(printer.set_bed_target_c(55.0)).expect("the bed target is accepted");
    wait::until("the bed to report the target it was given", REACHED, || {
        let snapshot = block_on(instance.printer().snapshot()).map_err(|e| e.to_string())?;
        let target = snapshot
            .bed
            .and_then(|bed| bed.target_c)
            .map(|reported| reported.value());
        if target == Some(55.0) {
            Ok(())
        } else {
            Err(format!("a target of {target:?}"))
        }
    });
    block_on(printer.set_bed_target_c(0.0)).expect("the bed is turned back off");
}

/// The fan, at two distinct percentages, as a command from this crate's own set.
fn pin_the_fan(proxy: &Proxy, printer: &impl PrinterPort) {
    let mut sent = Vec::new();
    for (percent, expected) in [(50.0_f64, 128_i64), (100.0, 255)] {
        proxy.forget();
        block_on(printer.set_fan_percent(percent)).expect("the fan is accepted");
        let request = only(proxy, "the fan");
        assert_eq!(request.path, "/api/printer/command");
        let body = request.json();
        let command = body["command"].as_str().expect("a command");
        assert!(
            COMMAND_SET.contains(&command),
            "{command:?} is not one of the commands this crate declares: {COMMAND_SET:?}"
        );
        assert_eq!(
            body["parameters"][FAN_PWM_PARAMETER],
            serde_json::json!(expected),
            "{percent} percent of full fan is a duty of {expected}"
        );
        sent.push(request.body.clone());
    }
    let without_digits =
        |body: &String| -> String { body.chars().filter(|c| !c.is_ascii_digit()).collect() };
    assert_eq!(
        without_digits(&sent[0]),
        without_digits(&sent[1]),
        "the two fan requests differ in something other than their number"
    );
}

/// A machine whose operator says it has no fan answers unsupported.
fn refuse_the_fan_where_there_is_none(instance: &Scripted) {
    let printer = instance.printer_with_no_fan();

    let error = block_on(printer.set_fan_percent(50.0)).expect_err("a refusal");

    assert_eq!(
        error,
        PrinterError::Unsupported {
            adjustable: Adjustable::Fan,
        },
        "a machine with no fan answers unsupported, naming the adjustable"
    );
}

/// Wait for the printer snapshot to report one state.
fn wait_for_printer(printer: &impl PrinterPort, wanted: &PrinterState, describing: &str) {
    wait::until(describing, REACHED, || {
        let found = block_on(printer.snapshot())
            .map_err(|error| error.to_string())?
            .connection;
        if found == *wanted {
            Ok(())
        } else {
            Err(format!("{found:?}"))
        }
    });
}

/// Wait for the job snapshot to report one state.
fn wait_for_job(instance: &Scripted, wanted: &PrinterState, describing: &str) {
    let printer = instance.printer();
    wait::until(describing, REACHED, || {
        let found = block_on(printer.job())
            .map_err(|error| error.to_string())?
            .state;
        if found == *wanted {
            Ok(())
        } else {
            Err(format!("{found:?}"))
        }
    });
}

/// The one request the instance received since the record was cleared.
fn only(proxy: &Proxy, describing: &str) -> Recorded {
    let requests = proxy.requests();
    assert_eq!(
        requests.len(),
        1,
        "{describing} sent {} requests: {requests:?}",
        requests.len()
    );
    let request = requests.into_iter().next().expect("the one request");
    assert_eq!(
        request.header("content-type"),
        Some("application/json"),
        "{describing} did not reach the instance as JSON"
    );
    request
}
