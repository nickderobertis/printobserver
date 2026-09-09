//! A configuration that cannot be valid, and the run request a valid one makes.
//!
//! Every field a caller could get wrong carries a type whose only constructor
//! refuses the wrong value, so the journeys here drive those constructors and
//! then drive a real turn: what the port hands `OneHarness` is what the
//! constrained values said, rather than strings this port re-checked on the way
//! past.

use std::sync::Arc;

use printobserver_oneharness::{ConfigError, EnvAssignment, HarnessIdentity, TurnTimeout};
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
    assert!(
        request.env.iter().any(|line| line == "MOCK_EXTRA=beside"),
        "the assignment did not reach the run request as KEY=VALUE: {:?}",
        request.env
    );
}
