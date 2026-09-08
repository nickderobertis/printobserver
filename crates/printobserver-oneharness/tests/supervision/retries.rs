//! `OneHarness` performs the retrying, and an answer that still fails after it
//! is a failed turn the loop carries on past.
//!
//! Three properties over two scripted responders. That retries are live: a
//! responder answering invalidly once and then validly must produce a turn that
//! succeeds, which a port that disabled or bypassed them cannot do. That the
//! exhausted path is the right one: a responder answering invalidly every time
//! must produce a recorded failure the next turn runs past. And that the
//! retrying is `OneHarness`'s own: in each fixture this crate issues exactly one
//! run request for the turn, and every responder invocation happens under it —
//! which a port that re-invoked `OneHarness` itself cannot satisfy, because each
//! of its attempts would be a run request of its own.
//!
//! No fixture reads a retry budget off the run request, and none asserts a
//! number of invocations: what is owed is that the outcome after exhaustion is
//! correct and that whatever retrying happens happens inside one request.

use std::sync::Arc;

use printobserver_supervisor_api::{SupervisorError, SupervisorPort};
use printobserver_types::{
    Confidence, EventPayload, MalformedExternalEventPayload, PrintId, serde_json,
};

use crate::support::{
    Fixture, HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, in_turn, port, responder, turn,
};

/// An event to hang a turn off.
fn payload() -> EventPayload {
    EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
        detail: "the body was not JSON".to_owned(),
    })
}

/// An answer that is well-formed JSON and violates the schema.
fn invalid() -> String {
    serde_json::json!({ "summary": "the first layer is down" }).to_string()
}

/// Every process created under this turn was the responder this repository
/// ships, and they all ran under the one request the port issued.
fn one_request_and_only_the_responder(watch: &Watch) {
    assert_eq!(
        watch.requests().len(),
        1,
        "the port issued more than one run request for one turn, so the retrying \
         was its own rather than OneHarness's"
    );
    let programs = watch.programs();
    assert!(!programs.is_empty(), "nothing ran under the request");
    for program in programs {
        assert_eq!(
            program,
            responder(),
            "a process other than the responder ran"
        );
    }
}

/// An answer that fails once and conforms next time is a turn that succeeds,
/// under one run request.
#[test]
fn a_retry_inside_one_request_recovers_the_turn() {
    let fixture = Fixture::new("retries-recovered");
    let counter = fixture.path("attempts");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            in_turn(
                "SID-RETRY",
                &[&invalid(), &assessment("the print is fine", "medium")],
                &counter,
            ),
        ),
        &watch,
    );

    let print_id = PrintId::new();
    let outcome = block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn recovered from the first invalid answer");
    assert_eq!(outcome.assessment.summary, "the print is fine");
    assert_eq!(outcome.assessment.confidence, Confidence::Medium);

    one_request_and_only_the_responder(&watch);
    let turns = supervisor
        .recorded_turns(&print_id)
        .expect("the ledger is readable");
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].failure, None,
        "a recovered turn was recorded failed"
    );
}

/// An answer that never conforms is a failed turn, written down, that the next
/// turn of the same print runs past.
#[test]
fn an_answer_that_never_conforms_is_a_failed_turn_the_loop_runs_past() {
    let fixture = Fixture::new("retries-exhausted");
    let schema = generated_assessment_schema();
    let print_id = PrintId::new();

    let watch = Arc::new(Watch::default());
    let failing = port(
        config(
            &fixture,
            HARNESS,
            &schema,
            always("SID-EXHAUST", &invalid()),
        ),
        &watch,
    );
    let refused = block_on(failing.run_turn(turn(print_id, event(print_id, payload()), None)));
    assert!(
        matches!(refused, Err(SupervisorError::InvalidAnswer { .. })),
        "an answer that never conformed was accepted: {refused:?}"
    );

    let turns = failing
        .recorded_turns(&print_id)
        .expect("the ledger is readable");
    assert_eq!(turns.len(), 1);
    let failure = turns[0]
        .failure
        .as_deref()
        .expect("the failed turn was not recorded as one");
    assert!(
        !failure.is_empty(),
        "the failed turn was recorded with no reason"
    );
    one_request_and_only_the_responder(&watch);

    // The loop is still running: the next turn of the same print drives through.
    let answering_watch = Arc::new(Watch::default());
    let answering = port(
        config(
            &fixture,
            HARNESS,
            &schema,
            always("SID-EXHAUST", &assessment("the print recovered", "low")),
        ),
        &answering_watch,
    );
    let outcome = block_on(answering.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn after a failed one did not run");
    assert_eq!(outcome.assessment.summary, "the print recovered");

    let turns = answering
        .recorded_turns(&print_id)
        .expect("the ledger is readable");
    assert_eq!(turns.len(), 2);
    assert!(turns[0].failure.is_some());
    assert_eq!(turns[1].failure, None);
}
