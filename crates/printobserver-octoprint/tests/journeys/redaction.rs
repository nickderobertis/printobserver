//! The API key appears in nothing this crate can render.
//!
//! The claim is over a bounded, enumerated universe rather than over one induced
//! failure, because a key leaks through whichever rendering nobody happened to
//! look at. The universe is three things and it is the whole of what this crate
//! can render:
//!
//! 1. every log record captured while each method the printer port declares is
//!    driven once against an instance that succeeds, once against one that
//!    fails, and once against one that cannot be reached at all — three, rather
//!    than the two a failure needs, because an unreachable instance is the one
//!    whose error this crate composes from the endpoint it was configured with;
//! 2. the displayed and the debug rendering of a value of every variant
//!    `PrinterError` declares, each constructed and rendered in turn;
//! 3. the displayed and the debug rendering of the configuration value the key
//!    was read into, and of the key itself.
//!
//! `journeys/coverage.rs` is what keeps the first two from falling behind the
//! surface they are about: it fails when the port gains a method this file does
//! not drive, or the error vocabulary a variant this file does not name.

use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter, REDACTED};
use printobserver_printer_api::{PrinterError, PrinterPort as _};
use printobserver_types::{Adjustable, FileName};

use crate::block_on::block_on;
use crate::received::Recorded;
use crate::records::{capture_everything, taken};
use crate::stub::{Reply, Stub, closed_address};

/// The key this journey configures, distinctive enough that its appearance
/// anywhere is unambiguous.
const DISTINCTIVE_KEY: &str = "octoprint-api-key-Zq7Wm3xR4tVn8sPdKfHb";

/// What an instance that is working answers each path with.
fn succeeding(request: &Recorded) -> Reply {
    if request.method == "GET" && request.path.starts_with("/api/printer?") {
        Reply::Body(
            200,
            r#"{"state": {"text": "Printing", "flags": {"operational": true, "printing": true}},
                "temperature": {"tool0": {"actual": 210.0, "target": 210.0}}}"#
                .to_owned(),
        )
    } else if request.method == "GET" && request.path == "/api/job" {
        Reply::Body(
            200,
            r#"{"job": {"file": {"name": "hold.gcode"}}, "progress": {"completion": 5.0},
                "state": "Printing"}"#
                .to_owned(),
        )
    } else {
        Reply::Empty(204)
    }
}

/// Drive every method the printer port declares, once each.
fn drive_every_method(printer: &OctoPrintPrinter) {
    drop(block_on(printer.snapshot()));
    drop(block_on(printer.job()));
    drop(block_on(
        printer.start(FileName::new("hold.gcode").expect("a file name")),
    ));
    drop(block_on(printer.pause()));
    drop(block_on(printer.resume()));
    drop(block_on(printer.cancel()));
    drop(block_on(printer.set_feedrate_factor(1.2)));
    drop(block_on(printer.set_flowrate_factor(0.95)));
    drop(block_on(printer.set_tool_target_c(0, 210.0)));
    drop(block_on(printer.set_bed_target_c(60.0)));
    drop(block_on(printer.set_fan_percent(75.0)));
}

/// A value of every variant `PrinterError` declares, constructed in turn.
fn every_error_variant() -> Vec<PrinterError> {
    vec![
        PrinterError::Unreachable {
            detail: "connecting to http://printer.local:5000: refused".to_owned(),
        },
        PrinterError::Unauthorized {
            detail: "Invalid API key".to_owned(),
        },
        PrinterError::Refused {
            status: 400,
            detail: "Invalid tool".to_owned(),
        },
        PrinterError::StateConflict {
            detail: "Printer is not operational or currently printing".to_owned(),
        },
        PrinterError::Unsupported {
            adjustable: Adjustable::Fan,
        },
        PrinterError::Malformed {
            detail: "/api/job answered something this port could not read".to_owned(),
        },
    ]
}

/// The key is in no record, no error rendering and no configuration rendering.
#[test]
fn the_api_key_appears_in_nothing_this_crate_can_render() {
    capture_everything();
    drop(taken());

    let working = Stub::start(succeeding);
    let unwell = Stub::always(Reply::Body(500, "the instance is unwell".to_owned()));
    let against = |stub: &Stub| {
        OctoPrintPrinter::new(
            OctoPrintConfig::new(&stub.base_url(), DISTINCTIVE_KEY).expect("a configuration"),
        )
    };

    drive_every_method(&against(&working));
    drive_every_method(&against(&unwell));
    drive_every_method(&OctoPrintPrinter::new(
        OctoPrintConfig::new(&closed_address(), DISTINCTIVE_KEY).expect("a configuration"),
    ));

    let records = taken();
    assert!(
        records.len() >= 11,
        "driving every method logged only {} records: {records:?}",
        records.len()
    );
    for record in &records {
        assert!(
            !record.contains(DISTINCTIVE_KEY),
            "a log record carries the API key: {record}"
        );
    }

    for error in every_error_variant() {
        assert!(
            !format!("{error}").contains(DISTINCTIVE_KEY),
            "an error's display carries the API key: {error}"
        );
        assert!(
            !format!("{error:?}").contains(DISTINCTIVE_KEY),
            "an error's debug carries the API key: {error:?}"
        );
    }

    let config = OctoPrintConfig::new("http://printer.local:5000", DISTINCTIVE_KEY)
        .expect("a configuration");
    for rendering in [format!("{config}"), format!("{config:?}")] {
        assert!(
            !rendering.contains(DISTINCTIVE_KEY),
            "a configuration rendering carries the API key: {rendering}"
        );
        assert!(
            rendering.contains(REDACTED),
            "a configuration rendering does not say the key is there: {rendering}"
        );
    }
    for rendering in [
        format!("{}", config.api_key()),
        format!("{:?}", config.api_key()),
    ] {
        assert!(
            !rendering.contains(DISTINCTIVE_KEY),
            "a key rendering carries the key: {rendering}"
        );
        assert!(rendering.contains(REDACTED), "{rendering}");
    }
}

/// The key does reach the instance, in the one header that authenticates it.
///
/// Without this, a crate that never sent the key at all would satisfy every
/// assertion above.
#[test]
fn the_api_key_reaches_the_instance_in_its_own_header() {
    let stub = Stub::always(Reply::Empty(204));
    let printer = OctoPrintPrinter::new(
        OctoPrintConfig::new(&stub.base_url(), DISTINCTIVE_KEY).expect("a configuration"),
    );

    block_on(printer.pause()).expect("the pause is accepted");

    let request = stub.only_request();
    assert_eq!(request.header("x-api-key"), Some(DISTINCTIVE_KEY));
    assert!(
        !request.path.contains(DISTINCTIVE_KEY),
        "the key is in the path, where a proxy log would keep it: {}",
        request.path
    );
    assert!(!request.body.contains(DISTINCTIVE_KEY), "{}", request.body);
}
