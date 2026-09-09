//! A configuration that cannot be valid, and the run request a valid one makes.
//!
//! Every field a caller could get wrong carries a type whose only constructor
//! refuses the wrong value, so the journeys here drive those constructors and
//! then drive a real turn: what the port hands `OneHarness` is what the
//! constrained values said, rather than strings this port re-checked on the way
//! past.

use std::sync::Arc;

use std::fs;

use printobserver_oneharness::{
    AssessmentSchema, ConfigError, EnvAssignment, HarnessIdentity, ModelName, TurnTimeout,
};
use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{EventPayload, MalformedExternalEventPayload, PrintId};

use crate::support::{
    Fixture, HARNESS, Watch, always, assessment, assignment, block_on, config, event,
    generated_assessment_schema, port, schema_read_lock, turn,
};

/// An event to hang a turn off.
fn payload() -> EventPayload {
    EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
        detail: "the body was not JSON".to_owned(),
    })
}

/// A harness identity is something `OneHarness` could select.
#[test]
fn an_empty_harness_identity_is_not_an_identity() {
    for empty in ["", "   ", "\n\t"] {
        assert_eq!(
            HarnessIdentity::new(empty),
            Err(ConfigError::HarnessIdentityEmpty),
            "`{empty:?}` was accepted as a harness identity"
        );
    }
    let named = HarnessIdentity::new("  claude-code  ").expect("a named identity");
    assert_eq!(named.as_str(), "claude-code");
    assert_eq!(named.to_string(), "claude-code");
}

/// A turn timeout bounds a turn, so zero seconds is not one.
#[test]
fn a_turn_timeout_of_zero_seconds_is_not_a_bound() {
    assert_eq!(TurnTimeout::new(0), Err(ConfigError::TurnTimeoutZero));
    assert_eq!(
        TurnTimeout::new(45).expect("a bound").seconds(),
        45,
        "a bound did not answer the seconds it was made from"
    );
    assert_eq!(TurnTimeout::DEFAULT.to_string(), "300s");
}

/// An environment assignment is a name and a value, or it is nothing.
#[test]
fn an_assignment_with_no_key_equals_value_shape_is_refused() {
    for malformed in ["MOCK_STDOUT", "", "=value", "1MOCK=value", "a b=value"] {
        let refused = EnvAssignment::new(malformed)
            .expect_err(&format!("`{malformed}` was accepted as an assignment"));
        assert!(
            matches!(refused, ConfigError::EnvAssignmentMalformed { .. }),
            "`{malformed}` was refused as something else: {refused:?}"
        );
        assert!(
            refused.to_string().contains(malformed),
            "the refusal does not name what was offered: {refused}"
        );
    }

    // A value carrying its own `=` is ordinary — the responder is scripted with
    // JSON — and all of it stays in the value.
    let json = EnvAssignment::new(r#"MOCK_STDOUT={"a":"b=c"}"#).expect("an assignment");
    assert_eq!(json.name(), "MOCK_STDOUT");
    assert_eq!(json.value(), r#"{"a":"b=c"}"#);
    assert_eq!(json.to_string(), r#"MOCK_STDOUT={"a":"b=c"}"#);
}

/// A model nobody pinned is the absence of a name rather than an empty one.
#[test]
fn a_model_named_as_nothing_is_not_a_pin() {
    for empty in ["", "  "] {
        assert_eq!(
            ModelName::new(empty),
            Err(ConfigError::ModelNameEmpty),
            "`{empty:?}` was accepted as a model"
        );
    }
    assert_eq!(
        ModelName::new(" claude-opus-5 ")
            .expect("a pinned model")
            .as_str(),
        "claude-opus-5"
    );
}

/// A file that constrains no answer is refused where it is named, rather than
/// hours later as every answer being turned away.
#[test]
fn a_schema_that_constrains_no_answer_is_refused_where_it_is_named() {
    let fixture = Fixture::new("configuration-schema");

    let absent = fixture.path("no-such-schema.json");
    let not_json = fixture.path("not-json.json");
    fs::write(&not_json, "this is not a schema").expect("a scratch file");
    let not_a_document = fixture.path("an-array.json");
    fs::write(&not_a_document, r#"["summary", "confidence"]"#).expect("a scratch file");
    // An object, and still no schema: nothing could be judged against it.
    let not_a_schema = fixture.path("not-a-schema.json");
    fs::write(&not_a_schema, r#"{"type": 17}"#).expect("a scratch file");

    // Each of the three is refused naming the file. What is said about the
    // absent one and the unparsable one is the operating system's own text and
    // serde's own text; only the last is this crate's to promise.
    for path in [&absent, &not_json, &not_a_document, &not_a_schema] {
        let refused = AssessmentSchema::at(path)
            .expect_err(&format!("{} was accepted as a schema", path.display()));
        let said = refused.to_string();
        assert!(
            said.contains(&path.display().to_string()),
            "the refusal does not name the file: {said}"
        );
        let (_, why) = said
            .split_once("does not constrain an answer: ")
            .unwrap_or_else(|| panic!("the refusal does not say why: {said}"));
        assert!(!why.is_empty(), "the refusal says why with nothing: {said}");
    }
    assert!(
        AssessmentSchema::at(&not_a_document)
            .expect_err("an array was accepted as a schema")
            .to_string()
            .contains("not a schema document"),
        "a JSON array was refused as something other than what it is"
    );

    // The generated artifact is a schema document, and reading it says so.
    let named =
        AssessmentSchema::at(generated_assessment_schema()).expect("the generated artifact");
    assert_eq!(named.path(), generated_assessment_schema());
}

/// What the constrained values say is what reaches `OneHarness`.
#[test]
fn the_run_request_carries_what_the_constrained_configuration_says() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("configuration-request");
    let watch = Arc::new(Watch::default());
    let mut configured = config(
        &fixture,
        HARNESS,
        &generated_assessment_schema(),
        always("SID-CONFIG", &assessment("the print is fine", "high")),
    );
    configured.turn_timeout = TurnTimeout::new(97).expect("a bound");
    configured.model = Some(ModelName::new("a-pinned-model").expect("a pinned model"));
    configured.harness_env.push(assignment("MOCK_EXTRA=beside"));
    let supervisor = port(configured, &watch);

    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn runs");

    let request = watch
        .requests()
        .into_iter()
        .next()
        .expect("the port built a run request");
    assert_eq!(request.harness, vec![HARNESS.to_owned()]);
    assert_eq!(request.timeout, Some(97));
    assert_eq!(request.model, vec!["a-pinned-model".to_owned()]);
    assert!(
        request.env.iter().any(|line| line == "MOCK_EXTRA=beside"),
        "the assignment did not reach the run request as KEY=VALUE: {:?}",
        request.env
    );
}
