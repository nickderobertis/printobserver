//! The push socket and the one command: what the printer pushes reaches the
//! port as the push vocabulary, and a printer that is not operational is read
//! as offline rather than as a conflict.
//!
//! Driven over a real socket against the scripted `OctoPrint` this crate's
//! `standin` feature compiles, which upgrades a real websocket and frames what
//! it pushes as `OctoPrint` does. The same reads against a real `OctoPrint`
//! are the server's integration tier's.

use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use printobserver_octoprint::push::pushes_of;
use printobserver_octoprint::standin::{JobScript, STANDIN_API_KEY, ScriptedOctoPrint};
use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{
    BoxStream, PrinterCommand, PrinterError, PrinterPort as _, PrinterPush, PrinterState,
};
use printobserver_types::serde_json::json;

use crate::block_on::block_on;

/// How long a journey waits for something the stand-in pushed.
const PATIENCE: Duration = Duration::from_secs(5);

/// The adapter, configured for one stand-in.
fn adapter(standin: &ScriptedOctoPrint) -> OctoPrintPrinter {
    OctoPrintPrinter::new(
        OctoPrintConfig::new(&standin.base_url(), STANDIN_API_KEY).expect("a valid configuration"),
    )
}

/// The next push, or `None` once the stream ended; panics when nothing arrives
/// in time.
fn next(stream: &mut BoxStream<'static, PrinterPush>) -> Option<PrinterPush> {
    let mut context = Context::from_waker(Waker::noop());
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Poll::Ready(item) = Pin::as_mut(stream).poll_next(&mut context) {
            return item;
        }
        assert!(
            Instant::now() < deadline,
            "nothing arrived on the push stream"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The next push that is not a terminal line.
fn next_event(stream: &mut BoxStream<'static, PrinterPush>) -> Option<PrinterPush> {
    loop {
        match next(stream) {
            Some(PrinterPush::Log { .. }) => {}
            other => return other,
        }
    }
}

/// Wait until the stand-in holds one open socket.
fn connected(standin: &ScriptedOctoPrint) {
    let deadline = Instant::now() + PATIENCE;
    while standin.open_sockets() == 0 {
        assert!(Instant::now() < deadline, "the push socket never opened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A reset and a printer's own error reach the port as what they are, and the
/// stream ends when the socket drops.
#[test]
fn a_reset_and_an_error_are_pushed_and_a_dropped_socket_ends_the_stream() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    let mut stream = block_on(printer.push_events()).expect("the push socket opens");
    connected(&standin);

    standin.pushes_event("PrinterReset", &json!({}));
    assert_eq!(next_event(&mut stream), Some(PrinterPush::Reset));
    standin.pushes_event(
        "Error",
        &json!({ "error": "Thermal Runaway", "reason": "firmware" }),
    );
    assert_eq!(
        next_event(&mut stream),
        Some(PrinterPush::Error {
            reason: "firmware".to_owned()
        })
    );
    standin.pushes_logs(&["Recv: ok"]);
    assert_eq!(
        next(&mut stream),
        Some(PrinterPush::Log {
            line: "Recv: ok".to_owned()
        })
    );

    standin.drops_sockets();
    assert_eq!(next_event(&mut stream), None);
}

/// An upgrade the instance refuses is an error opening the stream, and once it
/// is admitted again the socket opens.
#[test]
fn a_refused_upgrade_is_an_error_and_a_later_open_reconnects() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    standin.refuses_sockets(true);

    let refused = block_on(printer.push_events()).err();
    assert!(
        matches!(&refused, Some(PrinterError::Unreachable { detail }) if detail.contains("503")),
        "{refused:?}"
    );

    standin.refuses_sockets(false);
    let mut stream = block_on(printer.push_events()).expect("the push socket opens again");
    connected(&standin);
    standin.pushes_event("PrinterReset", &json!({}));
    assert_eq!(next_event(&mut stream), Some(PrinterPush::Reset));
}

/// `ReportPosition` is sent as `M114`, and its reply arrives on the push
/// socket as a terminal line naming the position.
#[test]
fn report_position_is_sent_as_m114_and_answered_on_the_push_socket() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    standin.moves_head_to([50.0, 60.0, 20.0]);
    let mut stream = block_on(printer.push_events()).expect("the push socket opens");
    connected(&standin);

    block_on(printer.send_command(PrinterCommand::ReportPosition)).expect("the command is sent");

    assert_eq!(standin.commands(), vec![("M114".to_owned(), false)]);
    let reply = loop {
        match next(&mut stream) {
            Some(PrinterPush::Log { line }) if line.starts_with("Recv: X:") => break line,
            Some(_) => {}
            None => panic!("the stream ended before the reply"),
        }
    };
    assert!(reply.contains("X:50.00 Y:60.00 Z:20.00"), "{reply}");
}

/// A printer that is not operational is refused by the printer read, and is
/// answered as offline from the connection read rather than as a conflict.
#[test]
fn a_printer_that_is_not_operational_reads_as_offline() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    standin.reports(
        "Offline after error",
        JobScript {
            state: "Offline after error".to_owned(),
            error: Some("SerialTimeoutException: ".to_owned()),
            ..JobScript::idle()
        },
    );

    let snapshot = block_on(printer.snapshot()).expect("an offline printer is a snapshot");
    assert_eq!(snapshot.connection, PrinterState::Offline);
    assert!(snapshot.tools.is_empty());
    let job = block_on(printer.job()).expect("the job reads");
    assert_eq!(job.state, PrinterState::Offline);
    assert_eq!(job.error.as_deref(), Some("SerialTimeoutException: "));
}

/// The history `OctoPrint` sends on connect is the past, and is read as
/// nothing; an event of another type is nothing either.
#[test]
fn the_history_and_other_events_are_read_as_nothing() {
    assert_eq!(
        pushes_of(r#"{"history": {"logs": ["Recv: start"]}}"#),
        Vec::new()
    );
    assert_eq!(
        pushes_of(r#"{"event": {"type": "ZChange", "payload": {}}}"#),
        Vec::new()
    );
    assert_eq!(pushes_of("not json"), Vec::new());
}
