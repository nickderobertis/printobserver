//! This port's two shapes carry the stated fields and emit their schemas.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_supervisor_api::{TurnOutcome, TurnRequest};
use printobserver_types::contract::schema_of;
use printobserver_types::{WireField, wire_fields};
use schema_files::reconcile;

/// The field the contract states, as a name, what it is, and whether it is
/// required.
fn field(name: &str, descriptor: &str, required: bool) -> WireField {
    WireField {
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
        required,
    }
}

/// The schemas this crate declares, by the file name each is written under.
fn generated() -> Vec<(String, printobserver_types::serde_json::Value)> {
    vec![
        ("TurnRequest.json".to_owned(), schema_of::<TurnRequest>()),
        ("TurnOutcome.json".to_owned(), schema_of::<TurnOutcome>()),
    ]
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let findings = reconcile("printobserver-supervisor-api", &generated());
    assert!(
        findings.is_empty(),
        "the checked-in schemas have drifted:\n{}",
        findings.join("\n")
    );
}

/// `TurnRequest` carries exactly the fields the contract states.
#[test]
fn turn_request_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("context_command", "string", true),
        field("event", "EventRecord", true),
        field("image_path", "string", false),
        field("print_id", "PrintId", true),
    ];
    assert_eq!(wire_fields(&schema_of::<TurnRequest>()), expected);
}

/// `TurnOutcome` carries exactly the fields the contract states.
#[test]
fn turn_outcome_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("assessment", "AgentAssessment", true),
        field("phase", "SessionPhase", true),
        field("session", "SupervisionSession", true),
    ];
    assert_eq!(wire_fields(&schema_of::<TurnOutcome>()), expected);
}
