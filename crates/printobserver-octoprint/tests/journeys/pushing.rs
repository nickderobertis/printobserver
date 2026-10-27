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
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint, minted};
use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{
    BoxStream, PrinterCommand, PrinterError, PrinterErrorReason, PrinterPort as _, PrinterPush,
    PrinterState,
};
use printobserver_types::serde_json::json;

use crate::block_on::block_on;

/// How long a journey waits for something the stand-in pushed.
const PATIENCE: Duration = Duration::from_secs(5);

/// The adapter, configured for one stand-in.
fn adapter(standin: &ScriptedOctoPrint) -> OctoPrintPrinter {
    OctoPrintPrinter::new(
        OctoPrintConfig::new(&standin.base_url(), standin.api_key())
            .expect("a valid configuration"),
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
            reason: PrinterErrorReason::Firmware
        })
    );
    standin.pushes_event(
        "Error",
        &json!({ "error": "Should resend", "reason": "resend" }),
    );
    assert_eq!(
        next_event(&mut stream),
        Some(PrinterPush::Error {
            reason: PrinterErrorReason::Other("resend".to_owned())
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
    assert_eq!(
        job.error,
        Some(printobserver_printer_api::JobError {
            text: "SerialTimeoutException: ".to_owned(),
            source: printobserver_printer_api::ErrorSource::Host,
        })
    );
}

/// A job's error carries the source's own text and whose it is, and a job
/// whose source reports none, or only blank text, carries none; and the
/// shape round-trips on the wire as an object of the two.
#[test]
fn a_jobs_error_carries_its_text_and_whose_it_is() {
    use printobserver_printer_api::{ErrorSource, JobError};
    for (reported, expected) in [
        (
            Some("SerialTimeoutException: "),
            Some(("SerialTimeoutException: ", ErrorSource::Host)),
        ),
        (
            Some("Thermal Runaway, system stopped!"),
            Some(("Thermal Runaway, system stopped!", ErrorSource::Printer)),
        ),
        (Some("   "), None),
        (None, None),
    ] {
        let standin = ScriptedOctoPrint::start();
        let printer = adapter(&standin);
        standin.reports(
            "Closed",
            JobScript {
                state: "Offline after error".to_owned(),
                error: reported.map(str::to_owned),
                ..JobScript::idle()
            },
        );
        let job = block_on(printer.job()).expect("the job reads");
        let expected = expected.map(|(text, source)| JobError {
            text: text.to_owned(),
            source,
        });
        assert_eq!(job.error, expected, "{reported:?}");
        let wire = printobserver_types::serde_json::to_value(&job).expect("the job renders");
        match &job.error {
            Some(error) => assert_eq!(
                wire["error"],
                json!({ "text": error.text, "source": printobserver_types::serde_json::to_value(error.source).expect("a source renders") })
            ),
            None => assert!(wire.get("error").is_none(), "{wire}"),
        }
        let read: printobserver_printer_api::JobSnapshot =
            printobserver_types::serde_json::from_value(wire).expect("the job reads back");
        assert_eq!(read, job);
    }
}

/// A job's error is the host's when it is an exception's name and colon, and
/// the printer's own message otherwise.
#[test]
fn a_jobs_error_is_the_hosts_when_it_names_an_exception() {
    use printobserver_octoprint::error_source_of;
    use printobserver_printer_api::ErrorSource;
    for host in [
        "SerialTimeoutException: ",
        "serial.SerialException: device reports readiness to read but returned no data",
        "OSError: [Errno 5] Input/output error",
    ] {
        assert_eq!(error_source_of(host), Some(ErrorSource::Host), "{host}");
    }
    for printer in [
        "Thermal Runaway, system stopped! Heater_ID: bed - Printer halted. kill() called!",
        "Should resend line 2 but no sufficient history is available, can't resend",
        "Error: Printer halted",
        "MINTEMP triggered: system stopped",
    ] {
        assert_eq!(
            error_source_of(printer),
            Some(ErrorSource::Printer),
            "{printer}"
        );
    }
    assert_eq!(error_source_of("  "), None);
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

/// One server frame, unmasked, with the given first byte and payload.
fn frame(first: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![first];
    let length = u8::try_from(payload.len()).expect("a short payload");
    assert!(length < 126, "these frames are short");
    bytes.push(length);
    bytes.extend_from_slice(payload);
    bytes
}

/// A login the instance refuses is an error opening the stream, in the port's
/// own vocabulary: the key refused, or the refusal in the instance's words.
#[test]
fn a_refused_login_is_an_error_opening_the_stream() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);

    standin.answers_login_with(403);
    assert!(matches!(
        block_on(printer.push_events()).err(),
        Some(PrinterError::Unauthorized { .. })
    ));
    standin.answers_login_with(500);
    assert!(matches!(
        block_on(printer.push_events()).err(),
        Some(PrinterError::Refused { status: 500, .. })
    ));
    assert_eq!(standin.open_sockets(), 0);
}

/// An upgrade answered with an accept that matches no key is refused.
#[test]
fn an_upgrade_whose_accept_matches_no_key_is_refused() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    standin.answers_a_wrong_accept(true);

    let refused = block_on(printer.push_events()).err();
    assert!(
        matches!(&refused, Some(PrinterError::Unreachable { detail }) if detail.contains("Sec-WebSocket-Accept")),
        "{refused:?}"
    );
}

/// A message split across a text frame and its continuation, with a ping
/// between them, is read whole, and the ping is answered with a pong.
#[test]
fn a_fragmented_message_is_read_whole_and_a_ping_between_its_fragments_is_answered() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    let mut stream = block_on(printer.push_events()).expect("the push socket opens");
    connected(&standin);

    let message = r#"{"event": {"type": "PrinterReset", "payload": {}}}"#.as_bytes();
    let (head, tail) = message.split_at(10);
    let mut bytes = frame(0x01, head);
    bytes.extend(frame(0x89, b"are you there"));
    bytes.extend(frame(0x80, tail));
    standin.pushes_raw(&bytes);

    assert_eq!(next_event(&mut stream), Some(PrinterPush::Reset));
    let deadline = Instant::now() + PATIENCE;
    while !standin.client_opcodes().contains(&0xA) {
        assert!(Instant::now() < deadline, "the ping was never answered");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Every frame RFC 6455 refuses a server's sending ends the stream rather
/// than being read: a masked frame, a reserved bit, a length in more bytes
/// than it needs, a frame past the limit, a binary frame, a continuation of
/// nothing, a message that is not UTF-8, a fragmented control frame, and a
/// close — well-formed or not.
#[test]
fn every_frame_a_server_may_not_send_ends_the_stream() {
    let mut nonminimal = vec![0x81, 126, 0, 5];
    nonminimal.extend_from_slice(b"hello");
    let mut past_the_limit = vec![0x81, 127];
    past_the_limit
        .extend_from_slice(&(printobserver_octoprint::push::FRAME_LIMIT + 1).to_be_bytes());
    let refused: Vec<(&str, Vec<u8>)> = vec![
        (
            "a masked frame",
            vec![0x81, 0x80 | 2, 0, 0, 0, 0, b'h', b'i'],
        ),
        ("a reserved bit", frame(0xC1, b"hi")),
        ("a non-minimal length", nonminimal),
        ("a frame past the limit", past_the_limit),
        ("a binary frame", frame(0x82, b"hi")),
        ("a continuation of nothing", frame(0x80, b"hi")),
        ("a message that is not UTF-8", frame(0x81, &[0xFF, 0xFE])),
        ("a fragmented ping", frame(0x09, b"hi")),
        (
            "a close with a status and a reason",
            frame(0x88, b"\x03\xe8going away"),
        ),
        ("a close with a one-byte payload", frame(0x88, b"\x03")),
        (
            "a close whose reason is not UTF-8",
            frame(0x88, &[0x03, 0xE8, 0xFF]),
        ),
        ("a close with nothing", frame(0x88, b"")),
    ];
    for (what, bytes) in refused {
        let standin = ScriptedOctoPrint::start();
        let printer = adapter(&standin);
        let mut stream = block_on(printer.push_events()).expect("the push socket opens");
        connected(&standin);

        standin.pushes_raw(&bytes);

        assert_eq!(
            next_event(&mut stream),
            None,
            "{what} did not end the stream"
        );
    }
}

/// A terminal line is a position only when it is a received reply naming X, Y
/// and Z as finite numbers.
#[test]
fn a_position_is_read_only_from_a_received_reply_naming_three_finite_axes() {
    use printobserver_octoprint::push::position_of;
    assert_eq!(
        position_of("Recv: X:50.00 Y:60.00 Z:20.00 E:0.00 Count X:0 Y:0 Z:0"),
        Some([50.0, 60.0, 20.0])
    );
    assert_eq!(
        position_of("Recv: ok X:0 Y:0 Z:0 E:0 Count: A:0 B:0 C:0"),
        Some([0.0, 0.0, 0.0])
    );
    for refused in [
        "Send: M114",
        "Recv: X:50.00 Y:60.00 E:0.00",
        "Recv: X:NaN Y:1 Z:1",
        "Recv: X:inf Y:1 Z:1",
        "Recv: T:210.0 /210.0 B:60.0 /60.0",
    ] {
        assert_eq!(position_of(refused), None, "{refused}");
    }
}

/// A login answered with no usable session, or an upgrade answered without
/// naming a websocket upgrade, is an error opening the stream.
#[test]
fn a_login_with_no_session_or_an_upgrade_naming_none_is_refused() {
    for body in [
        "not json",
        r#"{"name": "", "session": "a-session"}"#,
        r#"{"name": "standin", "session": ""}"#,
    ] {
        let standin = ScriptedOctoPrint::start();
        let printer = adapter(&standin);
        standin.answers_login_body(body);
        let refused = block_on(printer.push_events()).err();
        assert!(
            matches!(&refused, Some(PrinterError::Malformed { detail }) if detail.contains("/api/login")),
            "{body}: {refused:?}"
        );
    }
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    standin.answers_a_bare_upgrade(true);
    let refused = block_on(printer.push_events()).err();
    assert!(
        matches!(&refused, Some(PrinterError::Unreachable { detail }) if detail.contains("no websocket upgrade")),
        "{refused:?}"
    );
}

/// One server text frame, unmasked, whose length is written in the two-byte
/// (`126`) or the eight-byte (`127`) extended form.
fn extended_frame(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x81, marker];
    let length = u64::try_from(payload.len()).expect("a length");
    if marker == 126 {
        let short = u16::try_from(length).expect("a two-byte length");
        bytes.extend_from_slice(&short.to_be_bytes());
    } else {
        bytes.extend_from_slice(&length.to_be_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

/// A push message padded with trailing whitespace to `size` bytes.
fn padded(message: &str, size: usize) -> Vec<u8> {
    let mut bytes = message.as_bytes().to_vec();
    bytes.resize(size, b' ');
    bytes
}

/// A message too long for a one-byte length is read whole in either extended
/// encoding: two bytes for one under 64 KiB, eight for one past it.
#[test]
fn messages_in_both_extended_lengths_are_read_whole() {
    let reset = r#"{"event": {"type": "PrinterReset", "payload": {}}}"#;
    let error = r#"{"event": {"type": "Error", "payload": {"error": "Thermal Runaway", "reason": "firmware"}}}"#;
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    let mut stream = block_on(printer.push_events()).expect("the push socket opens");
    connected(&standin);

    let mut bytes = extended_frame(126, &padded(reset, 300));
    bytes.extend(extended_frame(127, &padded(error, 70_000)));
    standin.pushes_raw(&bytes);

    assert_eq!(next_event(&mut stream), Some(PrinterPush::Reset));
    assert_eq!(
        next_event(&mut stream),
        Some(PrinterPush::Error {
            reason: PrinterErrorReason::Firmware
        })
    );
}

/// A pong the server sends between a message's fragments is passed over: the
/// message is read whole, the pong is not answered, and the message after it
/// is read too.
#[test]
fn a_pong_between_a_messages_fragments_is_passed_over() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    let mut stream = block_on(printer.push_events()).expect("the push socket opens");
    connected(&standin);

    let message = r#"{"event": {"type": "PrinterReset", "payload": {}}}"#.as_bytes();
    let (head, tail) = message.split_at(10);
    let mut bytes = frame(0x01, head);
    bytes.extend(frame(0x8A, b"unsolicited"));
    bytes.extend(frame(0x80, tail));
    bytes.extend(frame(0x81, message));
    standin.pushes_raw(&bytes);

    assert_eq!(next_event(&mut stream), Some(PrinterPush::Reset));
    assert_eq!(next_event(&mut stream), Some(PrinterPush::Reset));
    assert!(
        !standin.client_opcodes().contains(&0xA),
        "a pong was answered with a pong"
    );
}

/// An upgrade answer whose head never ends is refused once it is longer than
/// any upgrade answer, rather than read without bound.
#[test]
fn an_upgrade_answer_that_never_ends_is_refused() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    standin.answers_an_endless_upgrade(true);

    let refused = block_on(printer.push_events()).err();
    assert!(
        matches!(&refused, Some(PrinterError::Unreachable { detail }) if detail.contains("never ended")),
        "{refused:?}"
    );
    assert_eq!(standin.open_sockets(), 0);
}

/// A length in eight bytes that two would hold, or one with its top bit set,
/// a message whose fragments together pass the limit, and a message begun
/// inside an unfinished one each end the stream.
#[test]
fn eight_byte_lengths_aggregate_limits_and_interleaved_messages_end_the_stream() {
    let limit = printobserver_octoprint::push::FRAME_LIMIT;
    let mut nonminimal = vec![0x81, 127];
    nonminimal.extend_from_slice(&5_u64.to_be_bytes());
    nonminimal.extend_from_slice(b"hello");
    let mut top_bit = vec![0x81, 127];
    top_bit.extend_from_slice(&(1_u64 << 63 | 0x0001_1170).to_be_bytes());
    let half = usize::try_from(limit / 2 + 1).expect("half the limit fits");
    let mut aggregate = Vec::new();
    for first in [0x01_u8, 0x80] {
        aggregate.push(first);
        aggregate.push(127);
        aggregate.extend_from_slice(&u64::try_from(half).expect("a length").to_be_bytes());
        aggregate.extend(std::iter::repeat_n(b'a', half));
    }
    let mut interleaved = frame(0x01, b"{\"current\"");
    interleaved.extend(frame(0x81, b"{}"));
    for (what, bytes) in [
        ("a non-minimal eight-byte length", nonminimal),
        ("a length with its top bit set", top_bit),
        ("fragments past the limit together", aggregate),
        ("a message begun inside another", interleaved),
    ] {
        let standin = ScriptedOctoPrint::start();
        let printer = adapter(&standin);
        let mut stream = block_on(printer.push_events()).expect("the push socket opens");
        connected(&standin);

        standin.pushes_raw(&bytes);

        assert_eq!(
            next_event(&mut stream),
            None,
            "{what} did not end the stream"
        );
    }
}

/// A printer read refused as a conflict stays the conflict it was when the
/// connection read fails, answers nothing readable, or says the printer is
/// operational: only a connection saying it is offline or in error is a
/// snapshot of that state.
#[test]
fn a_conflict_the_connection_does_not_explain_stays_a_conflict() {
    use crate::stub::{Reply, Stub};
    for connection in [
        Reply::Empty(500),
        Reply::Body(200, "not json".to_owned()),
        Reply::Body(200, r#"{"current": {"state": "Operational"}}"#.to_owned()),
    ] {
        let stub = Stub::start(move |request| {
            if request.path.starts_with("/api/printer") {
                Reply::Body(409, r#"{"error":"Printer is not operational"}"#.to_owned())
            } else {
                connection.clone()
            }
        });
        let printer = OctoPrintPrinter::new(
            OctoPrintConfig::new(&stub.base_url(), minted("octoprint-key"))
                .expect("a valid configuration"),
        );
        let read = block_on(printer.snapshot());
        assert!(
            matches!(read, Err(PrinterError::StateConflict { .. })),
            "{read:?}"
        );
    }
}

/// A position read whose push socket ends before any position arrived is the
/// printer unreachable, said as that, rather than a wait out to its timeout:
/// the socket that would carry the answer is gone.
#[test]
fn a_position_read_whose_socket_ends_is_unreachable_at_once() {
    let standin = ScriptedOctoPrint::start();
    let printer = adapter(&standin);
    standin.answers_position(false);

    let reading = std::thread::spawn(move || block_on(printer.read_position()));
    let deadline = Instant::now() + PATIENCE;
    while standin.open_sockets() == 0 {
        assert!(
            Instant::now() < deadline,
            "the position read opened no socket"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let dropped = Instant::now();
    standin.drops_sockets();
    let read = reading.join().expect("the read ran to its end");

    assert!(
        matches!(&read, Err(PrinterError::Unreachable { detail })
            if detail.contains("ended before the position was reported")),
        "{read:?}"
    );
    // Well inside the five seconds a read waits for a position that never
    // comes, which is the other way it could have ended.
    assert!(
        dropped.elapsed() < Duration::from_secs(4),
        "the read waited {:?} after its socket ended",
        dropped.elapsed()
    );
}
