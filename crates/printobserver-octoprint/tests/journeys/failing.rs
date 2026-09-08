//! What each way of failing becomes, so the layer above can act on it.
//!
//! The same distinctions against a real `OctoPrint` are the integration tier's.
//! What this journey adds is the far-side behaviour a healthy `OctoPrint` will
//! not produce on demand: a body that is not a response, a frame that promises
//! more than it delivers, an answer longer than an error should carry.

use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{PrinterError, PrinterPort as _};
use printobserver_types::PrinterState;

use crate::block_on::block_on;
use crate::stub::{Reply, Stub, closed_address};

/// A printer against a server answering one reply to everything.
fn answering(reply: Reply) -> (Stub, OctoPrintPrinter) {
    let stub = Stub::always(reply);
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    (stub, OctoPrintPrinter::new(config))
}

/// The error one reply produces.
fn error_from(reply: Reply) -> PrinterError {
    let (_stub, printer) = answering(reply);
    block_on(printer.pause()).expect_err("a refusal")
}

/// Both statuses `OctoPrint` answers an unknown key with are the key.
#[test]
fn a_rejected_key_is_the_key_rather_than_a_refusal() {
    for status in [401_u16, 403] {
        assert_eq!(
            error_from(Reply::Body(status, "Invalid API key".to_owned())),
            PrinterError::Unauthorized {
                detail: "Invalid API key".to_owned(),
            },
            "the status {status}"
        );
    }
}

/// A conflict is a state conflict rather than a refusal, because the layer
/// above retries one and not the other.
#[test]
fn a_conflict_is_a_state_conflict() {
    assert_eq!(
        error_from(Reply::Body(
            409,
            "Printer is not operational or currently printing".to_owned()
        )),
        PrinterError::StateConflict {
            detail: "Printer is not operational or currently printing".to_owned(),
        }
    );
}

/// Every other refusal carries the instance's own status and words.
#[test]
fn every_other_refusal_carries_the_status_and_the_words() {
    for status in [400_u16, 404, 500, 502] {
        assert_eq!(
            error_from(Reply::Body(status, "no".to_owned())),
            PrinterError::Refused {
                status,
                detail: "no".to_owned(),
            }
        );
    }
}

/// A printer nothing is listening for is unreachable, not refused.
#[test]
fn a_printer_nothing_answers_for_is_unreachable() {
    let config = OctoPrintConfig::new(&closed_address(), "test-key").expect("a configuration");
    let printer = OctoPrintPrinter::new(config);

    let error = block_on(printer.pause()).expect_err("a refusal");

    assert!(
        matches!(error, PrinterError::Unreachable { .. }),
        "{error:?}"
    );
}

/// A host that resolves to nothing is unreachable too.
#[test]
fn a_host_that_resolves_to_nothing_is_unreachable() {
    let config =
        OctoPrintConfig::new("http://printer.invalid:5000", "test-key").expect("a configuration");
    let printer = OctoPrintPrinter::new(config);

    let error = block_on(printer.job()).expect_err("a refusal");

    assert!(
        matches!(error, PrinterError::Unreachable { .. }),
        "{error:?}"
    );
}

/// Something that is not a response at all is malformed.
#[test]
fn something_that_is_not_a_response_is_malformed() {
    assert_eq!(
        error_from(Reply::Raw(b"not a response".to_vec())),
        PrinterError::Malformed {
            detail: "the response carries no header block".to_owned(),
        }
    );
}

/// A first line that is not a status line is malformed.
#[test]
fn a_first_line_that_is_no_status_line_is_malformed() {
    let error = error_from(Reply::Raw(b"OK\r\n\r\n".to_vec()));

    assert!(matches!(error, PrinterError::Malformed { .. }), "{error:?}");
}

/// A frame that promises more than it delivers is an answer that stopped part
/// way, which is the connection rather than the content.
#[test]
fn a_frame_that_promises_more_than_it_delivers_is_unreachable() {
    let error = error_from(Reply::Raw(
        b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n{}".to_vec(),
    ));

    assert!(
        matches!(error, PrinterError::Unreachable { .. }),
        "{error:?}"
    );
}

/// A chunked answer is read as the body it frames.
#[test]
fn a_chunked_answer_is_read_as_the_body_it_frames() {
    let body = r#"{"job": {"file": {"name": "a.gcode"}}, "state": "Operational"}"#;
    let mut raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for piece in [&body[..9], &body[9..30], &body[30..]] {
        raw.extend_from_slice(format!("{:x}\r\n{piece}\r\n", piece.len()).as_bytes());
    }
    raw.extend_from_slice(b"0\r\n\r\n");
    let stub = Stub::always(Reply::Raw(raw));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    let printer = OctoPrintPrinter::new(config);

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.file_name.as_deref(), Some("a.gcode"));
}

/// A chunk whose size is not a size is malformed.
#[test]
fn a_chunk_whose_size_is_no_size_is_malformed() {
    let error = error_from(Reply::Raw(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n{}\r\n".to_vec(),
    ));

    assert!(matches!(error, PrinterError::Malformed { .. }), "{error:?}");
}

/// A chunked answer that stops part way through a chunk is unreachable.
#[test]
fn a_chunked_answer_that_stops_part_way_is_unreachable() {
    let error = error_from(Reply::Raw(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nff\r\n{}\r\n".to_vec(),
    ));

    assert!(
        matches!(error, PrinterError::Unreachable { .. }),
        "{error:?}"
    );
}

/// A chunked answer with no size line at all is malformed.
#[test]
fn a_chunked_answer_with_no_size_line_is_malformed() {
    let error = error_from(Reply::Raw(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
    ));

    assert!(matches!(error, PrinterError::Malformed { .. }), "{error:?}");
}

/// An answer longer than an error should carry is cut, so the error stays
/// readable rather than becoming a page.
#[test]
fn an_answer_longer_than_an_error_should_carry_is_cut() {
    let error = error_from(Reply::Body(500, "x".repeat(4000)));

    match error {
        PrinterError::Refused { detail, .. } => {
            assert!(detail.len() < 4000, "the detail was not cut");
            assert!(detail.ends_with('…'), "the detail does not say it was cut");
        }
        other => panic!("expected a refusal, found {other:?}"),
    }
}

/// An answer framed by nothing but the close is read to the close.
#[test]
fn an_answer_framed_by_the_close_alone_is_read_to_the_close() {
    let stub = Stub::always(Reply::Raw(
        b"HTTP/1.1 200 OK\r\nServer: something\r\n\r\n{\"state\": \"Operational\"}".to_vec(),
    ));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    let printer = OctoPrintPrinter::new(config);

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(
        job.state,
        PrinterState::Operational,
        "the body after the header block is the body"
    );
}

/// A header line that is no header is skipped rather than failing the read.
#[test]
fn a_header_line_that_is_no_header_is_skipped() {
    let body = r#"{"state": "Operational"}"#;
    let raw = format!(
        "HTTP/1.1 200 OK\r\nnonsense\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let stub = Stub::always(Reply::Raw(raw.into_bytes()));
    let config = OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration");
    let printer = OctoPrintPrinter::new(config);

    let job = block_on(printer.job()).expect("a job");

    assert_eq!(job.state, PrinterState::Operational);
}
