//! Reading what the instance knows of a job's filament and of the files it
//! holds, from bodies a real `OctoPrint` 1.11.8 answered.
//!
//! Every body is a file under `tests/fixtures/octoprint-1.11.8/`, recorded from
//! the instance `just octoprint-up` provisions; that directory's `README.md`
//! says which request answered each, and names the one that is derived. The
//! same reads against a running instance are the integration tier's.

use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{FileFacts, PrinterError, PrinterPort as _, ToolFilament};
use printobserver_types::{FileName, Timestamp};

use crate::block_on::block_on;
use crate::stub::{Reply, Stub};

/// One recorded body.
fn recorded(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("octoprint-1.11.8")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A printer built against a server answering one recorded body to everything.
fn answering(name: &str) -> (Stub, OctoPrintPrinter) {
    let stub = Stub::always(Reply::Body(200, recorded(name)));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    (stub, OctoPrintPrinter::new(config))
}

/// One tool's filament, as the contract carries it.
fn tool(tool: i64, length_mm: f64, volume_cm3: Option<f64>) -> ToolFilament {
    ToolFilament {
        tool,
        length_mm,
        volume_cm3,
    }
}

/// A file name.
fn file(name: &str) -> FileName {
    FileName::new(name).expect("a file name")
}

/// A running job carries how far into the file it is and its one tool's
/// filament.
#[test]
fn a_running_job_carries_its_file_position_and_its_filament() {
    let (_stub, printer) = answering("job-running.json");

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.file_position_bytes, Some(959));
    assert_eq!(job.filament, vec![tool(0, 300.0, Some(0.0))]);
}

/// A job fed by two tools carries both, in tool order.
#[test]
fn a_two_tool_job_carries_each_tool_s_filament_in_tool_order() {
    let (_stub, printer) = answering("job-two-tools-selected.json");

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(
        job.filament,
        vec![tool(0, 10.0, Some(0.0)), tool(1, 7.0, Some(0.0))]
    );
    assert_eq!(
        job.file_position_bytes, None,
        "a job selected and not started has read nothing"
    );
}

/// A tool whose length is null is omitted, and a null volume is absent.
#[test]
fn a_tool_with_a_null_length_is_omitted_and_a_null_volume_is_absent() {
    let (_stub, printer) = answering("job-null-lengths.json");

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.filament, vec![tool(1, 7.0, None)]);
}

/// A cancelled job's progress is cleared by `OctoPrint`, and the snapshot
/// reports no position rather than inventing one.
#[test]
fn a_cancelled_job_reports_no_position() {
    let (_stub, printer) = answering("job-cancelled.json");

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.file_position_bytes, None);
    assert_eq!(job.completion, None);
    assert_eq!(
        job.filament,
        vec![tool(0, 300.0, Some(0.0))],
        "the analysis outlives the cancel"
    );
}

/// A completed job keeps its position at the file's size.
#[test]
fn a_completed_job_reports_a_position_equal_to_its_size() {
    let (_stub, printer) = answering("job-completed.json");

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.size_bytes, Some(184));
    assert_eq!(job.file_position_bytes, job.size_bytes);
}

/// A file the analysis queue has not reached carries no filament, and one never
/// printed carries zero tallies and no last print.
#[test]
fn a_file_before_its_analysis_carries_no_filament() {
    let (stub, printer) = answering("file-before-analysis.json");

    let facts = block_on(printer.file_facts(file("two-tools.gcode"))).expect("the facts");

    assert_eq!(
        facts,
        FileFacts {
            file_name: "two-tools.gcode".to_owned(),
            origin: "local".to_owned(),
            size_bytes: Some(184),
            filament: None,
            successes: 0,
            failures: 0,
            last_print: None,
        }
    );
    let request = stub.only_request();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/files/local/two-tools.gcode");
}

/// An analysed file carries each tool's filament.
#[test]
fn an_analysed_file_carries_each_tool_s_filament() {
    let (_stub, printer) = answering("file-analysed-never-printed.json");

    let facts = block_on(printer.file_facts(file("two-tools.gcode"))).expect("the facts");

    assert_eq!(
        facts.filament,
        Some(vec![tool(0, 10.0, Some(0.0)), tool(1, 7.0, Some(0.0))])
    );
    assert_eq!((facts.successes, facts.failures), (0, 0));
    assert_eq!(facts.last_print, None);
}

/// A printed file carries its tallies and its last print, to the second.
#[test]
fn a_printed_file_carries_its_tallies_and_its_last_print() {
    let (_stub, printer) = answering("file-printed.json");

    let facts = block_on(printer.file_facts(file("extrusion.gcode"))).expect("the facts");

    assert_eq!((facts.successes, facts.failures), (0, 1));
    let last = facts.last_print.expect("a last print");
    assert!(!last.success, "the one print was cancelled");
    assert_eq!(
        last.at,
        Timestamp::from_unix_seconds(1_791_683_203).expect("a timestamp")
    );
    assert_eq!(facts.filament, Some(vec![tool(0, 300.0, Some(0.0))]));
}

/// A file the instance does not hold is refused exactly as `start` refuses
/// one: a refusal carrying `OctoPrint`'s status and its own words.
#[test]
fn an_unknown_file_is_refused_in_the_shape_start_uses() {
    let stub = Stub::always(Reply::Body(404, recorded("file-missing.json")));
    let printer = OctoPrintPrinter::new(
        OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration"),
    );

    let read = block_on(printer.file_facts(file("missing.gcode"))).expect_err("a refusal");
    let started = block_on(printer.start(file("missing.gcode"))).expect_err("a refusal");
    let charged = block_on(printer.extruded_to(file("missing.gcode"), 10)).expect_err("a refusal");

    match &read {
        PrinterError::Refused { status, detail } => {
            assert_eq!(*status, 404);
            assert!(detail.contains("was not found"), "{detail}");
        }
        other => panic!("expected a refusal, found {other:?}"),
    }
    assert_eq!(read, started);
    assert_eq!(read, charged);
}

/// The listing is read recursively, folders flattened, and every file carrying
/// a tally is answered — and no folder, though a folder carries one too.
#[test]
fn every_printed_file_of_a_recursive_listing_is_answered() {
    let (stub, printer) = answering("files-recursive.json");

    let answered = block_on(printer.printed_files()).expect("the printed files");

    let names: Vec<(&str, i64, i64)> = answered
        .iter()
        .map(|facts| (facts.file_name.as_str(), facts.successes, facts.failures))
        .collect();
    assert_eq!(
        names,
        vec![
            ("extrusion.gcode", 0, 1),
            ("hold.gcode", 0, 1),
            ("two-tools.gcode", 1, 0),
        ],
        "the folder `fixtures` and the never-printed file inside it are not answered"
    );
    let completed = &answered[2];
    assert!(completed.last_print.as_ref().expect("a last print").success);
    assert_eq!(
        answered[1].filament,
        Some(Vec::new()),
        "an analysis that found no filament is an analysis, not an absent one"
    );
    assert_eq!(stub.only_request().path, "/api/files?recursive=true");
}

/// A file nested in a folder is found by the flattening.
#[test]
fn a_printed_file_inside_a_folder_is_answered() {
    let stub = Stub::always(Reply::Body(
        200,
        r#"{"files": [{"name": "parts", "origin": "local", "type": "folder",
             "prints": {"success": 2, "failure": 0},
             "children": [{"name": "deep", "origin": "local", "type": "folder",
               "children": [{"name": "bracket.gcode", "origin": "local",
                 "type": "machinecode", "size": 10,
                 "prints": {"success": 2, "failure": 0,
                   "last": {"date": 1700000000.0, "success": true}}}]}]},
           {"name": "on-card.gcode", "origin": "sdcard", "type": "machinecode"}]}"#
            .to_owned(),
    ));
    let printer = OctoPrintPrinter::new(
        OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration"),
    );

    let answered = block_on(printer.printed_files()).expect("the printed files");

    assert_eq!(answered.len(), 1, "{answered:?}");
    assert_eq!(answered[0].file_name, "bracket.gcode");
    assert_eq!(answered[0].successes, 2);
    assert_eq!(
        answered[0].filament, None,
        "an SD-card style entry has no analysis"
    );
}

/// A listing this port cannot read is malformed, naming the path.
#[test]
fn a_listing_this_port_cannot_read_is_malformed() {
    let stub = Stub::always(Reply::Body(200, r#"{"files": "nothing"}"#.to_owned()));
    let printer = OctoPrintPrinter::new(
        OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration"),
    );

    match block_on(printer.printed_files()).expect_err("a refusal") {
        PrinterError::Malformed { detail } => assert!(detail.contains("/api/files"), "{detail}"),
        other => panic!("expected a malformed answer, found {other:?}"),
    }
}
