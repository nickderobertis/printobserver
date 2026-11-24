//! An answer that does not conform to the schema is asked for again, and one
//! that still fails after it is a failed turn the loop carries on past.
//!
//! The asking again is `OneHarness`'s own on every harness: the port hands it
//! the schema and issues exactly one run request for the turn, and every
//! responder invocation happens under it. The port asks again for no schema
//! itself.

use std::sync::Arc;

use printobserver_supervisor_api::Confidence;
use printobserver_supervisor_api::{SupervisorError, SupervisorPort};
use printobserver_types::{EventBody, PrintId, serde_json};

use crate::support::{
    Fixture, HARNESS, OTHER_HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, in_turn, port, responder, schema_read_lock, turn, unreadable,
};

/// An event to hang a turn off.
fn payload() -> EventBody {
    unreadable("the body was not JSON")
}

/// An answer that is well-formed JSON and violates the schema.
fn invalid() -> String {
    serde_json::json!({ "summary": "the first layer is down" }).to_string()
}

/// How many times `OneHarness` asks again for an answer that does not conform
/// before it gives up: its own default, which a turn leaves as it is.
const ENGINE_RETRIES: usize = 2;

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

/// On either harness, an answer that fails once and conforms next time is a
/// turn that succeeds, under one run request.
#[test]
fn a_retry_inside_one_request_recovers_the_turn() {
    for harness in [HARNESS, OTHER_HARNESS] {
        // Every answer this journey drives is judged by the checked-in
        // assessment schema, so it is held still while the journey reads it.
        let schemas = schema_read_lock();
        let fixture = Fixture::new(&format!("retries-recovered-{harness}"));
        let counter = fixture.path("attempts");
        let watch = Arc::new(Watch::default());
        let supervisor = port(
            config(
                &schemas,
                &fixture,
                harness,
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
        let outcome = block_on(supervisor.run_turn(
            turn(print_id, event(print_id, payload()), None),
            crate::support::access(),
        ))
        .expect("the turn recovered from the first invalid answer");
        assert_eq!(outcome.assessment.summary, "the print is fine", "{harness}");
        assert_eq!(outcome.assessment.confidence, Confidence::Medium);

        one_request_and_only_the_responder(&watch);
        assert_eq!(
            watch.programs().len(),
            2,
            "{harness} was not asked again once"
        );
        let turns = supervisor
            .recorded_turns(&print_id)
            .expect("the ledger is readable");
        assert_eq!(turns.len(), 1);
        assert_eq!(
            turns[0].failure, None,
            "a recovered turn was recorded failed"
        );
    }
}

/// On Claude Code, an answer that never conforms is asked for again by
/// `OneHarness` [`ENGINE_RETRIES`] times under the turn's one request, and is
/// then a failed turn, written down, that the next turn of the same print runs
/// past.
#[test]
fn an_answer_that_never_conforms_is_a_failed_turn_the_loop_runs_past() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let schemas = schema_read_lock();
    let fixture = Fixture::new("retries-exhausted");
    let schema = generated_assessment_schema();
    let print_id = PrintId::new();

    let watch = Arc::new(Watch::default());
    let failing = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &schema,
            always("SID-EXHAUST", &invalid()),
        ),
        &watch,
    );
    let refused = block_on(failing.run_turn(
        turn(print_id, event(print_id, payload()), None),
        crate::support::access(),
    ));
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
    assert_eq!(
        watch.programs().len(),
        1 + ENGINE_RETRIES,
        "the answer was not asked for again exactly as often as the engine asks"
    );

    // The loop is still running: the next turn of the same print drives through.
    let answering_watch = Arc::new(Watch::default());
    let answering = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &schema,
            always("SID-EXHAUST", &assessment("the print recovered", "low")),
        ),
        &answering_watch,
    );
    let outcome = block_on(answering.run_turn(
        turn(print_id, event(print_id, payload()), None),
        crate::support::access(),
    ))
    .expect("the turn after a failed one did not run");
    assert_eq!(outcome.assessment.summary, "the print recovered");

    let turns = answering
        .recorded_turns(&print_id)
        .expect("the ledger is readable");
    assert_eq!(turns.len(), 2);
    assert!(turns[0].failure.is_some());
    assert_eq!(turns[1].failure, None);
}
