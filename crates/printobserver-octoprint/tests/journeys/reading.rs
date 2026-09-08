//! Reading a printer and a job: every field filled from what was reported, and
//! left absent from what was not.
//!
//! The instance here is a real HTTP server answering real bodies, so what is
//! under test is the whole path from socket to snapshot. The same walk against a
//! real `OctoPrint` is the integration tier's; this one is what makes the
//! mapping's own arms — a state this vocabulary has no arm for, a heater a
//! printer does not have, a body that is not JSON — reachable on demand.

use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{PrinterError, PrinterPort as _};
use printobserver_types::PrinterState;

use crate::block_on::block_on;
use crate::numbers::exactly;
use crate::stub::{Reply, Stub};

/// A printer answering exactly what `OctoPrint` answers mid-print.
const PRINTING: &str = r#"{
  "state": {
    "text": "Printing",
    "flags": {
      "operational": true, "paused": false, "printing": true, "pausing": false,
      "cancelling": false, "sdReady": false, "error": false, "ready": false,
      "closedOrError": false
    }
  },
  "temperature": {
    "tool0": {"actual": 209.9, "target": 210.0, "offset": 0},
    "bed": {"actual": 59.4, "target": 60.0, "offset": 0}
  }
}"#;

/// A job answering exactly what `OctoPrint` answers mid-print.
const RUNNING_JOB: &str = r#"{
  "job": {
    "file": {"name": "hold.gcode", "origin": "local", "size": 812, "date": 1756000000},
    "estimatedPrintTime": 400.4,
    "filament": null,
    "user": "printobserver"
  },
  "progress": {
    "completion": 12.5, "filepos": 101, "printTime": 50,
    "printTimeLeft": 350, "printTimeLeftOrigin": "linear"
  },
  "state": "Printing"
}"#;

/// A printer built against a server answering one body to everything.
fn against(body: &'static str) -> (Stub, OctoPrintPrinter) {
    let stub = Stub::always(Reply::Body(200, body.to_owned()));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    (stub, OctoPrintPrinter::new(config))
}

/// Every field the printer reported is filled, and the three it never reports
/// are absent.
#[test]
fn a_printer_snapshot_carries_what_the_printer_reported() {
    let (stub, printer) = against(PRINTING);

    let snapshot = block_on(printer.snapshot()).expect("a snapshot");

    assert_eq!(snapshot.connection, PrinterState::Printing);
    assert_eq!(snapshot.tools.len(), 1);
    let tool = &snapshot.tools[0];
    exactly(
        "the tool's temperature",
        tool.actual_c.expect("the tool's temperature").value(),
        209.9,
    );
    exactly(
        "the tool's target",
        tool.target_c.expect("the tool's target").value(),
        210.0,
    );
    exactly(
        "the tool's offset",
        tool.offset_c.expect("the tool's offset").value(),
        0.0,
    );
    let bed = snapshot.bed.as_ref().expect("the bed");
    exactly(
        "the bed's temperature",
        bed.actual_c.expect("the bed's temperature").value(),
        59.4,
    );
    assert_eq!(snapshot.chamber, None, "this printer reports no chamber");
    assert_eq!(snapshot.feedrate_factor, None);
    assert_eq!(snapshot.flowrate_factor, None);
    assert_eq!(snapshot.fan_percent, None);

    let request = stub.only_request();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/printer?history=false");
    assert_eq!(request.header("x-api-key"), Some("test-key"));
}

/// Every field the job reported is filled, completion converted to a fraction.
#[test]
fn a_job_snapshot_carries_what_the_job_reported() {
    let (stub, printer) = against(RUNNING_JOB);

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.file_name.as_deref(), Some("hold.gcode"));
    assert_eq!(job.file_origin.as_deref(), Some("local"));
    assert_eq!(job.size_bytes, Some(812));
    assert_eq!(job.estimated_print_time_s, Some(400));
    exactly(
        "OctoPrint's 12.5 percent as the contract's fraction",
        job.completion.expect("the completion").value(),
        0.125,
    );
    assert_eq!(job.print_time_s, Some(50));
    assert_eq!(job.print_time_left_s, Some(350));
    assert_eq!(job.state, PrinterState::Printing);
    assert_eq!(job.error, None, "this job reports no error");

    assert_eq!(stub.only_request().path, "/api/job");
}

/// A job with no file loaded reports almost nothing, and almost nothing is what
/// the snapshot carries.
#[test]
fn a_job_with_nothing_loaded_carries_nothing_it_did_not_report() {
    let (_stub, printer) = against(
        r#"{"job": {"file": {"name": null, "origin": null, "size": null},
             "estimatedPrintTime": null},
            "progress": {"completion": null, "printTime": null, "printTimeLeft": null},
            "state": "Operational"}"#,
    );

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.file_name, None);
    assert_eq!(job.file_origin, None);
    assert_eq!(job.size_bytes, None);
    assert_eq!(job.estimated_print_time_s, None);
    assert_eq!(job.completion, None);
    assert_eq!(job.print_time_s, None);
    assert_eq!(job.print_time_left_s, None);
    assert_eq!(job.state, PrinterState::Operational);
    assert_eq!(job.error, None);
}

/// A job carrying an error carries it through in the source's own words.
#[test]
fn a_job_reporting_an_error_carries_the_source_s_own_words() {
    let (_stub, printer) = against(
        r#"{"job": {}, "progress": {}, "state": "Error",
            "error": "Too many consecutive timeouts"}"#,
    );

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.state, PrinterState::Error);
    assert_eq!(job.error.as_deref(), Some("Too many consecutive timeouts"));
}

/// Every state word this mapping has an arm for, and one it does not.
#[test]
fn every_job_state_word_maps_to_the_arm_it_names() {
    let expected = [
        ("Operational", PrinterState::Operational),
        ("Printing", PrinterState::Printing),
        ("Starting", PrinterState::Printing),
        ("Resuming", PrinterState::Printing),
        ("Printing from SD", PrinterState::Printing),
        ("Sending file to SD", PrinterState::Printing),
        ("Finishing", PrinterState::Operational),
        ("Ready", PrinterState::Operational),
        ("Paused", PrinterState::Paused),
        ("Pausing", PrinterState::Paused),
        ("Cancelling", PrinterState::Cancelling),
        ("Error", PrinterState::Error),
        ("Offline", PrinterState::Offline),
        ("Offline after error", PrinterState::Offline),
        ("Closed", PrinterState::Offline),
        (
            "Transferring file to SD",
            PrinterState::Unknown("Transferring file to SD".to_owned()),
        ),
    ];
    for (word, state) in expected {
        let (_stub, printer) = {
            let stub = Stub::always(Reply::Body(
                200,
                format!(r#"{{"job": {{}}, "progress": {{}}, "state": "{word}"}}"#),
            ));
            let config =
                OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
            (stub, OctoPrintPrinter::new(config))
        };
        assert_eq!(
            block_on(printer.job()).expect("a job").state,
            state,
            "the job state word {word:?}"
        );
    }
}

/// A job answering no state at all is unknown rather than guessed at.
#[test]
fn a_job_reporting_no_state_is_unknown() {
    let (_stub, printer) = against(r#"{"job": {}, "progress": {}}"#);

    assert_eq!(
        block_on(printer.job()).expect("a job").state,
        PrinterState::Unknown(String::new())
    );
}

/// Every flag combination this mapping reads, in the order it reads them.
#[test]
fn every_printer_flag_combination_maps_to_the_arm_it_names() {
    let expected = [
        (r#"{"error": true, "printing": true}"#, PrinterState::Error),
        (
            r#"{"closedOrError": true, "operational": true}"#,
            PrinterState::Offline,
        ),
        (
            r#"{"cancelling": true, "printing": true, "operational": true}"#,
            PrinterState::Cancelling,
        ),
        (
            r#"{"paused": true, "operational": true}"#,
            PrinterState::Paused,
        ),
        (
            r#"{"pausing": true, "printing": true, "operational": true}"#,
            PrinterState::Paused,
        ),
        (
            r#"{"printing": true, "operational": true}"#,
            PrinterState::Printing,
        ),
        (r#"{"operational": true}"#, PrinterState::Operational),
    ];
    for (flags, state) in expected {
        let stub = Stub::always(Reply::Body(
            200,
            format!(r#"{{"state": {{"text": "x", "flags": {flags}}}, "temperature": {{}}}}"#),
        ));
        let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
        let printer = OctoPrintPrinter::new(config);
        assert_eq!(
            block_on(printer.snapshot()).expect("a snapshot").connection,
            state,
            "the flags {flags}"
        );
    }
}

/// A printer whose flags say nothing this vocabulary names carries its own word.
#[test]
fn a_printer_in_a_state_this_vocabulary_does_not_name_carries_its_own_word() {
    let (_stub, printer) =
        against(r#"{"state": {"text": "Detecting baudrate", "flags": {}}, "temperature": {}}"#);

    assert_eq!(
        block_on(printer.snapshot()).expect("a snapshot").connection,
        PrinterState::Unknown("Detecting baudrate".to_owned())
    );
}

/// A tool number the printer skipped leaves a heater reporting nothing in its
/// place, so the index stays the printer's own tool number.
#[test]
fn a_gap_in_the_tool_numbering_keeps_the_index_the_tool_number() {
    let (_stub, printer) = against(
        r#"{"state": {"text": "Operational", "flags": {"operational": true}},
            "temperature": {"tool2": {"actual": 30.0}, "bed": {"actual": 20.0},
                            "history": [], "toolX": {"actual": 1.0}}}"#,
    );

    let snapshot = block_on(printer.snapshot()).expect("a snapshot");

    assert_eq!(snapshot.tools.len(), 3, "tools 0, 1 and 2");
    assert_eq!(snapshot.tools[0].actual_c, None);
    assert_eq!(snapshot.tools[1].actual_c, None);
    exactly(
        "the heater that stayed at its own tool number",
        snapshot.tools[2].actual_c.expect("tool 2").value(),
        30.0,
    );
}

/// A printer reporting no temperatures at all reports no tools.
#[test]
fn a_printer_reporting_no_temperatures_reports_no_tools() {
    let (_stub, printer) = against(r#"{"state": {"text": "Offline", "flags": {}}}"#);

    let snapshot = block_on(printer.snapshot()).expect("a snapshot");

    assert!(snapshot.tools.is_empty(), "{:?}", snapshot.tools);
    assert_eq!(snapshot.bed, None);
}

/// A reading outside the range this vocabulary declares is carried as reported
/// and flagged, rather than dropped.
#[test]
fn an_implausible_reading_is_carried_and_flagged() {
    let (_stub, printer) = against(
        r#"{"state": {"text": "Operational", "flags": {"operational": true}},
            "temperature": {"tool0": {"actual": 4000.0, "target": 210.0}}}"#,
    );

    let reading = block_on(printer.snapshot()).expect("a snapshot").tools[0]
        .actual_c
        .expect("the tool's temperature");

    exactly(
        "the implausible reading, carried as reported",
        reading.value(),
        4000.0,
    );
    assert!(reading.out_of_range(), "4000 C is not a plausible reading");
}

/// A body that is not what this port can read is malformed, naming the path.
#[test]
fn a_body_this_port_cannot_read_is_malformed() {
    let (_stub, printer) = against("this is not JSON");

    let error = block_on(printer.snapshot()).expect_err("a refusal");

    match error {
        PrinterError::Malformed { detail } => {
            assert!(detail.contains("/api/printer"), "{detail}");
        }
        other => panic!("expected a malformed answer, found {other:?}"),
    }
}
