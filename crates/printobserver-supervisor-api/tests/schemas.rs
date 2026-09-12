//! This port's two shapes and two event kinds carry the stated fields and emit
//! their schemas.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_supervisor_api::{
    SupervisionSessionClosedPayload, SupervisionSessionOpenedPayload, TurnOutcome, TurnRequest,
};
use printobserver_types::contract::{Sample as _, schema_of};
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventPayload, WireField, event_schema_of, wire_fields,
};
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
        (
            "SupervisionSessionOpenedPayload.json".to_owned(),
            event_schema_of::<SupervisionSessionOpenedPayload>(),
        ),
        (
            "SupervisionSessionClosedPayload.json".to_owned(),
            event_schema_of::<SupervisionSessionClosedPayload>(),
        ),
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

/// Each kind this crate declares is written under its own name, carries the
/// stated fields, and reads back under that name and no other.
#[test]
fn each_kind_is_written_under_its_own_name_and_reads_back_under_it() {
    assert_eq!(
        SupervisionSessionOpenedPayload::KIND,
        "supervision_session_opened"
    );
    assert_eq!(
        SupervisionSessionClosedPayload::KIND,
        "supervision_session_closed"
    );
    assert_eq!(
        wire_fields(&schema_of::<SupervisionSessionOpenedPayload>()),
        vec![
            field("harness_identity", "string", true),
            field("session_name", "string", true),
        ]
    );
    assert_eq!(
        wire_fields(&schema_of::<SupervisionSessionClosedPayload>()),
        vec![
            field("close_reason", "string", true),
            field("session_name", "string", true),
        ]
    );
    assert_eq!(
        event_schema_of::<SupervisionSessionOpenedPayload>()[EVENT_KIND_MARKER],
        "supervision_session_opened"
    );

    let opened = SupervisionSessionOpenedPayload::sample_full();
    let body = EventBody::of(&opened).expect("a payload renders");
    assert_eq!(body.kind.as_str(), "supervision_session_opened");
    assert_eq!(
        body.read::<SupervisionSessionOpenedPayload>()
            .expect("under its own kind")
            .expect("of its own type"),
        opened
    );
    assert!(body.read::<SupervisionSessionClosedPayload>().is_none());
}
