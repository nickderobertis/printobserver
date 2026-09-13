//! This port's shapes, its assessment vocabulary, its session and its two
//! event kinds carry the stated fields and emit their schemas.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes every schema under `schemas/printobserver-supervisor-api/`, and
//! without it refuses a tree whose checked-in schema no longer matches what
//! the types generate.
//!
//! One of the files it reconciles is the generated assessment schema, which is
//! the artifact the agent's answer is constrained by — and one journey in
//! `printobserver-oneharness` **writes** to it on disk, drives an answer that
//! was accepted before, and puts it back. The two suites run at the same
//! time, so every test here that reads the checked-in tree takes the lock
//! [`schema_lock`] describes.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_supervisor_api::{
    AgentAssessment, Confidence, SessionPhase, SupervisionSession, SupervisionSessionClosedPayload,
    SupervisionSessionOpenedPayload, TurnOutcome, TurnRequest,
};
use printobserver_types::contract::{Sample as _, TypeContract, schema_of};
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventPayload, WireField, event_schema_of, wire_fields,
};
use schema_files::reconcile;

/// The lock the checked-in schema tree is read and written under.
///
/// `printobserver-oneharness`'s assessment-schema journey changes the
/// checked-in `AgentAssessment.json` on disk and puts it back, which is what
/// proves the port reads that artifact at run time rather than validating
/// against a copy of its bytes; this suite reads that same file, and
/// `nx run-many` drives the two at the same time. Both sides take this lock,
/// so neither ever sees the other's half-done tree.
///
/// The lock is the operating system's own, so the kernel releases it when the
/// handle goes — a test that panics, or is killed, leaves nothing behind. The
/// file sits under `target`, which is per-worktree and ignored, so two
/// checkouts on one machine never block each other.
///
/// `repo-policy.toml`'s `supervisor.schema_lock` is where the name comes from,
/// and `just check-repo` holds every holder it declares to that one name: two
/// suites that locked two different files would be back to no lock at all.
fn schema_lock() -> std::fs::File {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target");
    std::fs::create_dir_all(&directory).expect("the target directory is writable");
    let file = std::fs::File::create(directory.join("printobserver-schemas.lock"))
        .expect("the schema lock file is creatable");
    file.lock().expect("the schema lock is takeable");
    file
}

/// The assessment vocabulary this port declares, each with its canonical values.
fn assessment_vocabulary() -> Vec<TypeContract> {
    vec![
        TypeContract::of::<AgentAssessment>("AgentAssessment"),
        TypeContract::of::<Confidence>("Confidence"),
    ]
}

/// The session vocabulary this port declares, each with its canonical values.
fn session_vocabulary() -> Vec<TypeContract> {
    vec![
        TypeContract::of::<SessionPhase>("SessionPhase"),
        TypeContract::of::<SupervisionSession>("SupervisionSession"),
    ]
}

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
fn generated() -> Vec<(String, Value)> {
    let mut entries = vec![
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
    ];
    entries.extend(
        assessment_vocabulary()
            .into_iter()
            .chain(session_vocabulary())
            .map(|entry| (format!("{}.json", entry.name), entry.schema())),
    );
    entries
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let _lock = schema_lock();
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

/// The assessment vocabulary carries exactly the fields the contract states,
/// under the names the schemas carry.
#[test]
fn the_assessment_vocabulary_carries_exactly_the_stated_fields() {
    let expected: [(&str, Vec<WireField>); 2] = [
        (
            "AgentAssessment",
            vec![
                field("confidence", "Confidence", true),
                field("did", "string", true),
                field("escalating", "boolean", true),
                field("should_continue", "boolean", true),
                field("summary", "string", true),
                field("why", "string", true),
            ],
        ),
        ("Confidence", vec![]),
    ];
    let declared = assessment_vocabulary();
    assert_eq!(declared.len(), expected.len());
    for (entry, (name, fields)) in declared.iter().zip(expected) {
        assert_eq!(entry.name, name);
        let schema = entry.schema();
        assert_eq!(
            wire_fields(&schema),
            fields,
            "{name} does not carry the stated fields"
        );
        assert_eq!(schema.get("title").and_then(Value::as_str), Some(name));
    }
}

/// The confidence vocabulary is the closed set of three, in its lowercase
/// spellings, and its sample is one of them.
#[test]
fn confidence_is_a_closed_set_of_three() {
    let schema = schema_of::<Confidence>();
    let spellings: Vec<&str> = schema["oneOf"]
        .as_array()
        .expect("a closed set of arms")
        .iter()
        .filter_map(|arm| arm["const"].as_str())
        .collect();
    assert_eq!(spellings, ["low", "medium", "high"]);
    let sample = printobserver_types::serde_json::to_value(Confidence::sample_full())
        .expect("a fieldless enum serializes");
    assert!(spellings.contains(&sample.as_str().expect("a spelling")));
}

/// Every canonical value of the assessment vocabulary round-trips unchanged,
/// and a value of no declared type is refused: an assessment carries every
/// field or is not one.
#[test]
fn the_assessment_vocabulary_round_trips_and_refuses_what_it_does_not_declare() {
    for entry in assessment_vocabulary() {
        for value in entry.samples() {
            let round = entry
                .round_trip(value.clone())
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
            assert_eq!(round, value, "{} does not survive a round trip", entry.name);
        }
        assert!(
            entry
                .round_trip(json!({ "printobserver": "no type declares this field" }))
                .is_err(),
            "{} accepted a value of no declared type",
            entry.name
        );
    }
    let mut without_why = TypeContract::of::<AgentAssessment>("AgentAssessment").full();
    without_why
        .as_object_mut()
        .expect("an object")
        .remove("why");
    assert!(
        TypeContract::of::<AgentAssessment>("AgentAssessment")
            .round_trip(without_why)
            .is_err(),
        "an assessment missing a required field was accepted"
    );
}

/// The session vocabulary carries exactly the fields the contract states.
#[test]
fn the_session_vocabulary_carries_exactly_the_stated_fields() {
    let expected: [(&str, Vec<WireField>); 2] = [
        ("SessionPhase", vec![]),
        (
            "SupervisionSession",
            vec![
                field("close_reason", "string", false),
                field("closed_at", "Timestamp", false),
                field("created_at", "Timestamp", true),
                field("harness_identity", "string", true),
                field("last_turn_at", "Timestamp", true),
                field("print_id", "PrintId", true),
                field("session_name", "string", true),
            ],
        ),
    ];
    let declared = session_vocabulary();
    assert_eq!(declared.len(), expected.len());
    for (entry, (name, fields)) in declared.iter().zip(expected) {
        assert_eq!(entry.name, name);
        assert_eq!(wire_fields(&entry.schema()), fields, "{name}");
        assert_eq!(
            entry.schema().get("title").and_then(Value::as_str),
            Some(name),
            "{name} generates a schema titled otherwise"
        );
    }
}

/// The session round-trips its canonical values, omits an optional it does
/// not carry rather than writing `null`, and holds the wire rules every
/// declared type is held to: its instants are RFC 3339 at a zero offset —
/// normalized from an offset on parse, refused when not RFC 3339 at all —
/// and its print identifier is refused in any spelling this system does not
/// mint.
#[test]
fn the_session_round_trips_and_holds_the_wire_rules() {
    for entry in session_vocabulary() {
        for value in entry.samples() {
            let round = entry
                .round_trip(value.clone())
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
            assert_eq!(round, value, "{} does not survive a round trip", entry.name);
        }
        assert!(
            entry
                .round_trip(json!({ "printobserver": "no type declares this field" }))
                .is_err(),
            "{} accepted a value of no declared type",
            entry.name
        );
    }

    let session = TypeContract::of::<SupervisionSession>("SupervisionSession");
    let minimal = session.minimal();
    let object = minimal.as_object().expect("a session is an object");
    for name in ["closed_at", "close_reason"] {
        assert!(
            !object.contains_key(name),
            "the minimal session carries {name} as {:?}",
            object.get(name)
        );
    }

    let full = session.full();
    for name in ["created_at", "last_turn_at", "closed_at"] {
        let text = full[name].as_str().expect("an instant is a string");
        assert!(text.ends_with('Z'), "{name} is {text}");
        let mut offset = full.clone();
        offset[name] = json!("2026-03-01T13:00:00+01:00");
        let round = session
            .round_trip(offset)
            .unwrap_or_else(|error| panic!("{name} refused an offset: {error}"));
        assert_eq!(round[name].as_str(), Some("2026-03-01T12:00:00Z"));
        for refused in ["2026-03-01T12:00:00", "the first of March"] {
            let mut value = full.clone();
            value[name] = json!(refused);
            assert!(
                session.round_trip(value).is_err(),
                "{name} accepted {refused}"
            );
        }
    }
    for refused in [
        "0191f0a0-0000-4000-8000-000000000001",
        "0191F0A0-0000-7000-8000-000000000001",
        "{0191f0a0-0000-7000-8000-000000000001}",
        "not-a-uuid",
    ] {
        let mut value = full.clone();
        value["print_id"] = json!(refused);
        assert!(
            session.round_trip(value).is_err(),
            "print_id accepted {refused}"
        );
    }
}
