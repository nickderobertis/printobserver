//! An answer that does not conform to the schema is asked for again, and one
//! that still fails after it is a failed turn the loop carries on past.
//!
//! Who asks again depends on how the answer is held to the schema. On a
//! harness `OneHarness` holds natively, the retrying is `OneHarness`'s own: the
//! port issues exactly one run request for the turn, and every responder
//! invocation happens under it. On Claude Code, which the port holds to the
//! schema by its prompt, the port asks again itself, in the turn's own session,
//! up to [`SCHEMA_RETRIES`] times — the engine's own default, which the engine
//! does not export, so one journey here holds the two to each other by counting
//! how many times the engine's own loop runs the responder.

use std::sync::Arc;

use printobserver_supervisor_api::Confidence;
use printobserver_supervisor_api::{SupervisorError, SupervisorPort};
use printobserver_types::{EventBody, PrintId, serde_json};

use printobserver_oneharness::SCHEMA_RETRIES;

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

/// On a harness the engine holds natively, an answer that fails once and
/// conforms next time is a turn that succeeds, under one run request.
#[test]
fn a_retry_inside_one_request_recovers_the_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let schemas = schema_read_lock();
    let fixture = Fixture::new("retries-recovered");
    let counter = fixture.path("attempts");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            OTHER_HARNESS,
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

/// On Claude Code, an answer that never conforms is asked for again in the
/// turn's session [`SCHEMA_RETRIES`] times, and is then a failed turn, written
/// down, that the next turn of the same print runs past.
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
    let requests = watch.requests();
    assert_eq!(
        requests.len(),
        1 + SCHEMA_RETRIES,
        "the answer was not asked for again exactly as often as the engine asks"
    );
    assert!(
        requests
            .iter()
            .all(|request| request.session == requests[0].session),
        "an answer was asked for again outside the turn's session"
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

/// On Claude Code, an answer that fails once is asked for again in the turn's
/// own session, with the engine's own feedback, and the conforming answer that
/// follows is the turn's.
#[test]
fn a_claude_code_answer_out_of_the_schema_is_asked_again_and_recovers() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("retries-by-prompt");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            in_turn(
                "SID-AGAIN",
                &[&invalid(), &assessment("the print is fine", "medium")],
                &fixture.path("attempts"),
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
    assert_eq!(outcome.assessment.summary, "the print is fine");

    let requests = watch.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].session, requests[0].session);
    assert!(
        requests[1].prompt[0]
            .starts_with("Your previous response did not conform to the required JSON Schema."),
        "{:?}",
        requests[1].prompt
    );
    let turns = supervisor
        .recorded_turns(&print_id)
        .expect("the ledger is readable");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].failure, None);
}

/// The engine's own loop, on a harness it holds natively, runs a never
/// conforming answer exactly `1 + SCHEMA_RETRIES` times under one request: the
/// count the port asks again by on Claude Code is the engine's own.
#[test]
fn the_port_asks_again_as_often_as_the_engines_own_loop() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("retries-engine-count");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            OTHER_HARNESS,
            &generated_assessment_schema(),
            always("SID-COUNT", &invalid()),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    let refused = block_on(supervisor.run_turn(
        turn(print_id, event(print_id, payload()), None),
        crate::support::access(),
    ));
    assert!(
        matches!(refused, Err(SupervisorError::InvalidAnswer { .. })),
        "{refused:?}"
    );
    one_request_and_only_the_responder(&watch);
    assert_eq!(
        watch.programs().len(),
        1 + SCHEMA_RETRIES,
        "the engine's own loop asked a different number of times than the port asks"
    );
}
