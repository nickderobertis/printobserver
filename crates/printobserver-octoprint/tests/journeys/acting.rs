//! What each action puts on the wire, and the conversions it puts there.
//!
//! The pins that matter — a factor in `OctoPrint`'s own units, a fan duty in the
//! firmware's — are made against a real `OctoPrint` by the integration tier.
//! What this journey adds is the arms that tier cannot reach: a machine with no
//! fan, a multiplier that denotes no whole percentage, a file name that has to
//! be escaped into its path segment.

use printobserver_octoprint::{
    COMMAND_SET, FAN_PWM_PARAMETER, FAN_SET_COMMAND, FanSupport, OctoPrintConfig, OctoPrintPrinter,
    fan_pwm_of_percent, fraction_of_completion, percent_of_multiplier,
};
use printobserver_printer_api::{PrinterError, PrinterPort as _};
use printobserver_types::{Adjustable, FileName};
use serde_json::json;

use crate::block_on::block_on;
use crate::numbers::exactly;
use crate::received::Recorded;
use crate::stub::{Reply, Stub};

/// A printer against a server accepting every action the way `OctoPrint` does.
fn accepting() -> (Stub, OctoPrintPrinter) {
    let stub = Stub::always(Reply::Empty(204));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    (stub, OctoPrintPrinter::new(config))
}

/// Pausing asks the job endpoint to pause.
#[test]
fn pausing_asks_the_job_endpoint_for_the_pause_action() {
    let (stub, printer) = accepting();

    block_on(printer.pause()).expect("the pause is accepted");

    let request = stub.only_request();
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/api/job")
    );
    assert_eq!(
        request.json(),
        json!({"command": "pause", "action": "pause"})
    );
}

/// Resuming asks the same endpoint for the other action.
#[test]
fn resuming_asks_the_job_endpoint_for_the_resume_action() {
    let (stub, printer) = accepting();

    block_on(printer.resume()).expect("the resume is accepted");

    assert_eq!(
        stub.only_request().json(),
        json!({"command": "pause", "action": "resume"})
    );
}

/// Cancelling asks the job endpoint to cancel.
#[test]
fn cancelling_asks_the_job_endpoint_to_cancel() {
    let (stub, printer) = accepting();

    block_on(printer.cancel()).expect("the cancel is accepted");

    assert_eq!(stub.only_request().json(), json!({"command": "cancel"}));
}

/// Starting selects the named file, then starts what is selected.
#[test]
fn starting_selects_the_named_file_and_then_starts_it() {
    let (stub, printer) = accepting();

    block_on(printer.start(FileName::new("hold.gcode").expect("a file name")))
        .expect("the start is accepted");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].path, "/api/files/local/hold.gcode");
    assert_eq!(
        requests[0].json(),
        json!({"command": "select", "print": false})
    );
    assert_eq!(requests[1].path, "/api/job");
    assert_eq!(requests[1].json(), json!({"command": "start"}));
}

/// A legal file name that is not a legal path segment is escaped into one.
#[test]
fn a_file_name_needing_escaping_is_escaped_into_its_segment() {
    let (stub, printer) = accepting();

    block_on(printer.start(FileName::new("a b?c#d&e.gcode").expect("a file name")))
        .expect("the start is accepted");

    assert_eq!(
        stub.requests()[0].path,
        "/api/files/local/a%20b%3Fc%23d%26e.gcode",
        "every byte outside the unreserved set is escaped"
    );
}

/// A start whose selection is refused does not go on to start anything.
#[test]
fn a_start_whose_selection_is_refused_starts_nothing() {
    let stub = Stub::always(Reply::Body(404, "Unknown file".to_owned()));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    let printer = OctoPrintPrinter::new(config);

    let error = block_on(printer.start(FileName::new("gone.gcode").expect("a file name")))
        .expect_err("a refusal");

    assert!(
        matches!(error, PrinterError::Refused { status: 404, .. }),
        "{error:?}"
    );
    assert_eq!(stub.requests().len(), 1, "nothing was started");
}

/// The feedrate goes to the print head as a whole-number percentage.
#[test]
fn the_feedrate_goes_to_the_print_head_as_a_whole_percentage() {
    let (stub, printer) = accepting();

    block_on(printer.set_feedrate_factor(1.5)).expect("the feedrate is accepted");

    let request = stub.only_request();
    assert_eq!(request.path, "/api/printer/printhead");
    assert_eq!(
        request.json(),
        json!({"command": "feedrate", "factor": 150})
    );
    assert!(
        !request.body.contains("150.0"),
        "OctoPrint reads a fractional factor as a multiplier: {}",
        request.body
    );
}

/// The flowrate goes to the tool endpoint as a whole-number percentage.
#[test]
fn the_flowrate_goes_to_the_tool_endpoint_as_a_whole_percentage() {
    let (stub, printer) = accepting();

    block_on(printer.set_flowrate_factor(0.9)).expect("the flowrate is accepted");

    let request = stub.only_request();
    assert_eq!(request.path, "/api/printer/tool");
    assert_eq!(request.json(), json!({"command": "flowrate", "factor": 90}));
}

/// A tool's target goes to the tool endpoint keyed by the printer's own number.
#[test]
fn a_tool_target_is_keyed_by_the_printer_s_own_tool_number() {
    let (stub, printer) = accepting();

    block_on(printer.set_tool_target_c(1, 215.5)).expect("the target is accepted");

    let request = stub.only_request();
    assert_eq!(request.path, "/api/printer/tool");
    assert_eq!(
        request.json(),
        json!({"command": "target", "targets": {"tool1": 215.5}})
    );
}

/// The bed's target goes to the bed endpoint.
#[test]
fn a_bed_target_goes_to_the_bed_endpoint() {
    let (stub, printer) = accepting();

    block_on(printer.set_bed_target_c(60.0)).expect("the target is accepted");

    let request = stub.only_request();
    assert_eq!(request.path, "/api/printer/bed");
    assert_eq!(request.json(), json!({"command": "target", "target": 60.0}));
}

/// The fan is one of this crate's own commands, carrying a duty and no text.
#[test]
fn the_fan_is_one_of_this_crate_s_own_commands_carrying_a_duty() {
    let (stub, printer) = accepting();

    block_on(printer.set_fan_percent(50.0)).expect("the fan is accepted");
    block_on(printer.set_fan_percent(100.0)).expect("the fan is accepted");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.path, "/api/printer/command");
        let sent = request.json();
        let command = sent["command"].as_str().expect("a command");
        assert!(
            COMMAND_SET.contains(&command),
            "{command:?} is not one of {COMMAND_SET:?}"
        );
    }
    assert_eq!(
        requests[0].json()["parameters"][FAN_PWM_PARAMETER],
        json!(128),
        "half of full fan rounds half away from zero, so 127.5 is 128"
    );
    assert_eq!(
        requests[1].json()["parameters"][FAN_PWM_PARAMETER],
        json!(255)
    );
    assert_eq!(
        non_numeric(&requests[0]),
        non_numeric(&requests[1]),
        "the two fan requests differ in their number and in nothing else"
    );
}

/// A machine whose operator says it has no fan is asked for nothing.
#[test]
fn a_machine_with_no_fan_answers_unsupported_and_is_asked_for_nothing() {
    let stub = Stub::always(Reply::Empty(204));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key")
        .expect("a configuration")
        .with_fan(FanSupport::Absent);
    let printer = OctoPrintPrinter::new(config);

    let error = block_on(printer.set_fan_percent(50.0)).expect_err("a refusal");

    assert_eq!(
        error,
        PrinterError::Unsupported {
            adjustable: Adjustable::Fan,
        }
    );
    assert!(
        stub.requests().is_empty(),
        "a machine with no fan was sent something: {:?}",
        stub.requests()
    );
}

/// The completion conversion is a division by a hundred, in that direction.
#[test]
fn the_completion_conversion_divides_the_reported_percentage() {
    exactly(
        "no progress as a fraction",
        fraction_of_completion(0.0),
        0.0,
    );
    exactly(
        "12.5 percent as a fraction",
        fraction_of_completion(12.5),
        0.125,
    );
    exactly(
        "a finished print as a fraction",
        fraction_of_completion(100.0),
        1.0,
    );
}

/// The factor conversion multiplies by a hundred, in that direction, and rounds
/// half away from zero.
#[test]
fn the_factor_conversion_answers_a_whole_percentage() {
    assert_eq!(percent_of_multiplier(1.0), 100);
    assert_eq!(percent_of_multiplier(1.5), 150);
    assert_eq!(
        percent_of_multiplier(0.755),
        76,
        "75.5 rounds away from zero"
    );
    assert_eq!(
        percent_of_multiplier(f64::NAN),
        0,
        "a multiplier that is no number denotes no percentage OctoPrint accepts"
    );
    assert_eq!(percent_of_multiplier(f64::INFINITY), 0);
    assert_eq!(
        percent_of_multiplier(1e30),
        0,
        "a magnitude no integer holds is refused rather than saturated"
    );
}

/// The fan conversion scales a percentage onto the duty range and clamps to it.
#[test]
fn the_fan_conversion_scales_onto_the_duty_range() {
    assert_eq!(fan_pwm_of_percent(0.0), 0);
    assert_eq!(fan_pwm_of_percent(50.0), 128);
    assert_eq!(fan_pwm_of_percent(100.0), 255);
    assert_eq!(fan_pwm_of_percent(40.0), 102);
    assert_eq!(fan_pwm_of_percent(1000.0), 255, "clamped to full fan");
    assert_eq!(fan_pwm_of_percent(-50.0), 0, "clamped to no fan");
    assert_eq!(fan_pwm_of_percent(f64::NAN), 0);
}

/// The fan command names its own parameter, so the number is never in the text.
#[test]
fn the_fan_command_carries_its_number_in_a_named_parameter() {
    assert!(
        FAN_SET_COMMAND.contains(&format!("%({FAN_PWM_PARAMETER})s")),
        "{FAN_SET_COMMAND}"
    );
    assert_eq!(COMMAND_SET, [FAN_SET_COMMAND]);
}

/// One request with every digit removed, which is everything a caller must not
/// be able to change.
fn non_numeric(request: &Recorded) -> String {
    request
        .body
        .chars()
        .filter(|character| !character.is_ascii_digit())
        .collect()
}
