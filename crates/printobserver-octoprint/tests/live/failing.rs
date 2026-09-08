//! Every way a real `OctoPrint` refuses, and what each one becomes.
//!
//! The walk is over the variants the error vocabulary declares. Five of them are
//! produced against the real instance, each by a fixture that produces that one
//! and no other, and each is asserted to carry its own variant and its own
//! payload — so the layer above can tell "the machine will not" from "the
//! machine is not there" from "this key is wrong".
//!
//! One of them is recorded rather than produced, and the record says why:
//! `PrinterError::Malformed` is what this port answers a body it cannot read,
//! and a healthy `OctoPrint` answers JSON to every read this port makes. There
//! is nothing to ask it that produces one. It is produced against a server that
//! answers something else, in the deterministic tier's `failing` journey, which
//! drives the same client over the same socket.

use printobserver_printer_api::{PrinterError, PrinterPort as _};
use printobserver_types::{Adjustable, FileName};

use crate::block_on::block_on;
use crate::env::Scripted;

/// The file the scripted environment uploads, which the instance does have.
const HOLD_FILE: &str = "hold.gcode";

/// A file the instance does not have, and will not be given.
const ABSENT_FILE: &str = "no-such-file-printobserver.gcode";

/// Every variant, against a real instance.
///
/// # Panics
///
/// Panics when a fixture does not produce the variant it exists to produce.
pub fn walk(instance: &Scripted) {
    unauthorized(instance);
    state_conflict(instance);
    refused(instance);
    unreachable(instance);
    unsupported(instance);
    recorded_as_unproducible_against_this_target();
}

/// A key the instance does not know is the key, not a refusal.
fn unauthorized(instance: &Scripted) {
    let error = block_on(instance.printer_with_a_wrong_key().job()).expect_err("a refusal");

    match error {
        PrinterError::Unauthorized { detail } => {
            assert!(
                !detail.is_empty(),
                "the instance said nothing about the key"
            );
        }
        other => panic!("expected the key to be refused, found {other:?}"),
    }
}

/// Selecting a file while a print is running is a state conflict.
fn state_conflict(instance: &Scripted) {
    let error = block_on(
        instance
            .printer()
            .start(FileName::new(HOLD_FILE).expect("a file name")),
    )
    .expect_err("a refusal");

    match error {
        PrinterError::StateConflict { detail } => {
            assert!(
                !detail.is_empty(),
                "the instance said nothing about the state"
            );
        }
        other => panic!("expected a state conflict, found {other:?}"),
    }
}

/// A file the printer does not have is refused, in the instance's own words.
///
/// This is what a refusal is against this target, and finding that out is part
/// of what driving the real thing is for: `OctoPrint` 1.11 takes a feedrate
/// factor of any magnitude and a target for a tool it does not have, answering
/// `204` to both, so neither of those is a refusal here however plainly wrong it
/// looks. Asking it to print something it does not hold is.
fn refused(instance: &Scripted) {
    let error = block_on(
        instance
            .printer()
            .start(FileName::new(ABSENT_FILE).expect("a file name")),
    )
    .expect_err("a refusal");

    match error {
        PrinterError::Refused { status, detail } => {
            assert_eq!(status, 404, "the instance refused with {status}: {detail}");
            assert!(!detail.is_empty(), "the instance said nothing");
        }
        other => panic!("expected a refusal, found {other:?}"),
    }
}

/// A printer nothing answers for is unreachable.
fn unreachable(instance: &Scripted) {
    let closed = closed_address();
    let printer = instance.printer_at(&closed);

    let error = block_on(printer.job()).expect_err("a refusal");

    match error {
        PrinterError::Unreachable { detail } => {
            assert!(detail.contains(&closed), "{detail} does not name {closed}");
        }
        other => panic!("expected an unreachable printer, found {other:?}"),
    }
}

/// A machine with no fan cannot express one.
fn unsupported(instance: &Scripted) {
    let error =
        block_on(instance.printer_with_no_fan().set_fan_percent(40.0)).expect_err("a refusal");

    assert_eq!(
        error,
        PrinterError::Unsupported {
            adjustable: Adjustable::Fan,
        }
    );
}

/// The one variant this target cannot produce, and the reason it cannot.
fn recorded_as_unproducible_against_this_target() {
    let recorded = [(
        PrinterError::Malformed {
            detail: String::new(),
        },
        "a healthy OctoPrint answers JSON to every read this port makes, so there is \
         nothing to ask it that produces an unreadable body; it is produced against a \
         server that answers something else, over the same client and the same socket, \
         in the deterministic tier's `journeys/failing.rs`",
    )];
    for (variant, reason) in recorded {
        assert!(
            !reason.is_empty(),
            "{variant:?} is recorded as unproducible with no reason"
        );
    }
}

/// A loopback address nothing is listening on.
fn closed_address() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    drop(listener);
    format!("http://{address}")
}
