//! Closing a session and opening the next one, and none of it reaching the
//! caller as a failure.
//!
//! Four causes, and the same two questions of each: what the ledger records,
//! and what the port answered its caller. The second question is asked in all
//! four rather than only where a new session follows, because a close recorded
//! correctly while still handing the caller an error is exactly the defect a
//! narrower reading misses.

use std::sync::Arc;

use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{
    EventPayload, MalformedExternalEventPayload, PrintId, SessionPhase, SupervisionSession,
};

use crate::support::{
    Fixture, HARNESS, OTHER_HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, port, schema_read_lock, turn,
};

/// An event that says nothing this system could read.
fn unreadable() -> EventPayload {
    EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
        detail: "the body was not JSON".to_owned(),
    })
}

/// The environment every journey here scripts the responder with.
fn answering() -> Vec<String> {
    always("SID-CLOSING", &assessment("the print is fine", "medium"))
}

/// The one session a print has open, or the last one it had.
fn last(sessions: &[SupervisionSession]) -> &SupervisionSession {
    sessions.last().expect("the print has a session")
}

/// A print reaching a terminal state closes its session with that state as the
/// reason, and closing answers success.
#[test]
fn a_terminal_state_closes_the_session_with_that_state_as_the_reason() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("closing-terminal");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            answering(),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, unreadable()), None)))
        .expect("the turn runs");

    let answered = block_on(supervisor.close_session(print_id, "cancelled".to_owned()));
    assert_eq!(answered, Ok(()), "closing answered the caller a failure");

    let sessions = supervisor
        .recorded_sessions(&print_id)
        .expect("the ledger is readable");
    let closed = last(&sessions);
    assert!(closed.closed_at.is_some(), "the close was not recorded");
    assert_eq!(closed.close_reason.as_deref(), Some("cancelled"));
}

/// An abandoned print closes its session with abandonment as the reason, and
/// closing answers success.
#[test]
fn an_abandoned_print_closes_its_session_with_abandonment_as_the_reason() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("closing-abandoned");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            answering(),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, unreadable()), None)))
        .expect("the turn runs");

    let answered = block_on(supervisor.close_session(print_id, "abandoned".to_owned()));
    assert_eq!(answered, Ok(()), "closing answered the caller a failure");

    let sessions = supervisor
        .recorded_sessions(&print_id)
        .expect("the ledger is readable");
    let closed = last(&sessions);
    assert!(closed.closed_at.is_some(), "the close was not recorded");
    assert_eq!(closed.close_reason.as_deref(), Some("abandoned"));
}

/// An event arriving for a print whose session was closed opens a new one,
/// under a name carrying a sequence beside that print's identifier.
#[test]
fn an_event_after_a_close_opens_the_next_session_of_the_sequence() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("closing-reopen");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            answering(),
        ),
        &watch,
    );
    let print_id = PrintId::new();

    let first = block_on(supervisor.run_turn(turn(print_id, event(print_id, unreadable()), None)))
        .expect("the first turn runs");
    block_on(supervisor.close_session(print_id, "finished".to_owned()))
        .expect("closing answered the caller a failure");

    let next = block_on(supervisor.run_turn(turn(print_id, event(print_id, unreadable()), None)))
        .expect("the turn after the close answered the caller a failure");

    // Both names are read out of the reports the runs returned.
    assert_eq!(first.session.session_name, format!("print-{print_id}"));
    assert_eq!(next.session.session_name, format!("print-{print_id}-2"));
    assert_eq!(next.phase, SessionPhase::Created);

    let sessions = supervisor
        .recorded_sessions(&print_id)
        .expect("the ledger is readable");
    assert_eq!(sessions.len(), 2);
    assert!(sessions[0].closed_at.is_some());
    assert_eq!(sessions[0].close_reason.as_deref(), Some("finished"));
    assert_eq!(sessions[1].closed_at, None);
}

/// A harness that refuses to continue a session it did not create is a close
/// carrying that reason and a new session, not an error the caller sees.
#[test]
fn a_refused_identity_closes_the_session_and_opens_a_new_one() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("closing-identity");
    let schema = generated_assessment_schema();
    let print_id = PrintId::new();

    let watch = Arc::new(Watch::default());
    let first = port(config(&fixture, HARNESS, &schema, answering()), &watch);
    let opened = block_on(first.run_turn(turn(print_id, event(print_id, unreadable()), None)))
        .expect("the first turn runs");
    drop(first);

    // The same state directory, and so the same session, on another identity.
    let moved_watch = Arc::new(Watch::default());
    let moved = port(
        config(&fixture, OTHER_HARNESS, &schema, answering()),
        &moved_watch,
    );
    let after = block_on(moved.run_turn(turn(print_id, event(print_id, unreadable()), None)))
        .expect("the refusal reached the caller as an error");

    assert_eq!(opened.session.session_name, format!("print-{print_id}"));
    assert_eq!(after.session.session_name, format!("print-{print_id}-2"));
    assert_eq!(after.phase, SessionPhase::Created);

    let sessions = moved
        .recorded_sessions(&print_id)
        .expect("the ledger is readable");
    assert_eq!(sessions.len(), 2);
    assert!(
        sessions[0].closed_at.is_some(),
        "the close was not recorded"
    );
    let reason = sessions[0]
        .close_reason
        .as_deref()
        .expect("the close carries a reason");
    assert!(
        reason.contains(HARNESS) && reason.contains(OTHER_HARNESS),
        "the reason does not name the identity that refused: {reason}"
    );
}
