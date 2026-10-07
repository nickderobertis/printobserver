//! The documents that state how a running job is matched to a print, held to
//! the constants that decide it.
//!
//! The README and the skill's references tell an operator and an agent how far
//! a job's running time may fall short and still be the same job, and what a
//! print a later job replaced is ended with. Both are this crate's
//! ([`JOB_IDENTITY_TOLERANCE_S`], [`REPLACED_REASON`]), so a document restating
//! either is read against them here rather than trusted to be kept in step.
//!
//! Every statement of the tolerance is written `<N> seconds (the identity
//! tolerance)`, and that phrase is what this reads: a statement carrying any
//! other number of seconds is refused, and so is a document that no longer
//! carries the phrase at all.

use std::path::PathBuf;

use printobserver_core::listing::{JOB_IDENTITY_TOLERANCE_S, REPLACED_REASON};

/// What names the tolerance in a document, after the number of seconds.
const PHRASE: &str = " seconds (the identity tolerance)";

/// Every document that states the tolerance and the replaced reason.
const DOCUMENTS: [&str; 3] = [
    "README.md",
    "skills/printobserver/reference/api-and-clients.md",
    "skills/printobserver/reference/common-operations.md",
];

/// One document, with every run of whitespace — a line break included — read
/// as one space, so a phrase wrapped across lines reads as it is written.
fn document(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let text = std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("{path} could not be read: {error}"));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The number of seconds each statement of the tolerance in `text` carries.
fn stated_tolerances(text: &str) -> Vec<String> {
    let mut stated = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(PHRASE) {
        let before = &rest[..at];
        let number = before.rsplit(' ').next().unwrap_or_default().to_owned();
        stated.push(number);
        rest = &rest[at + PHRASE.len()..];
    }
    stated
}

#[test]
fn every_document_states_the_tolerance_the_matching_uses() {
    let expected = JOB_IDENTITY_TOLERANCE_S.to_string();
    for path in DOCUMENTS {
        let stated = stated_tolerances(&document(path));
        assert!(
            !stated.is_empty(),
            "{path} no longer states the identity tolerance as `<N>{PHRASE}`"
        );
        for number in stated {
            assert_eq!(
                number, expected,
                "{path} states the identity tolerance as {number} seconds, and the \
                 matching uses JOB_IDENTITY_TOLERANCE_S = {expected}"
            );
        }
    }
}

#[test]
fn every_document_names_the_reason_a_replaced_print_ends_with() {
    for path in DOCUMENTS {
        assert!(
            document(path).contains(REPLACED_REASON),
            "{path} does not name `{REPLACED_REASON}`, the reason a print a later job \
             of its file replaced is ended with"
        );
    }
}

#[test]
fn a_statement_of_another_tolerance_is_read_as_that_number() {
    let stated = stated_tolerances(
        "less than 90 seconds (the identity tolerance) and no more than 120 seconds \
         (the identity tolerance) before",
    );

    assert_eq!(stated, ["90", "120"]);
}
