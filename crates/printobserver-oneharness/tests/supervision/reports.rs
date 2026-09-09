//! What the port makes of a finished run's report, including the shapes a turn
//! cannot be written down from.
//!
//! `OneHarness`'s report answers every shape of run it serves: its session
//! block is absent for a run under no session handle, and its results are
//! several for a fan-out. This port asks for neither, so a report of either
//! shape is one it did not ask for. Every report here is one a real run
//! answered, read off the port's own seam; the ones a turn cannot be recorded
//! from are that report with a part taken away or repeated, which is the only
//! way this system meets any of them.

use std::sync::Arc;

use oneharness_core::domain::report::RunReport;
use printobserver_oneharness::{HarnessIdentity, TurnReport};
use printobserver_supervisor_api::{SupervisorError, SupervisorPort};
use printobserver_types::{EventPayload, MalformedExternalEventPayload, PrintId, SessionPhase};

use crate::support::{
    Fixture, HARNESS, OTHER_HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, port, schema_read_lock, turn,
};

/// An event to hang a turn off.
fn payload() -> EventPayload {
    EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
        detail: "the body was not JSON".to_owned(),
    })
}

/// The session every report here was answered about, read off the report
/// itself: what a turn asks for is what the port planned, and what these
/// journeys hand back to the narrowing is what that run answered.
fn asked_about(report: &RunReport) -> String {
    report
        .session
        .as_ref()
        .expect("a run under a session handle answers a session block")
        .name
        .clone()
}

/// The harness every journey here runs on.
fn asked_of() -> HarnessIdentity {
    HarnessIdentity::new(HARNESS).expect("a journey names a harness")
}

/// The report one real turn answered with.
fn one_real_report(tag: &str) -> RunReport {
    let fixture = Fixture::new(tag);
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            always("SID-REPORT", &assessment("the print is fine", "high")),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn runs");
    watch
        .reports()
        .into_iter()
        .next()
        .expect("the run answered a report")
}

/// The detail a refusal carries.
fn detail(error: &SupervisorError) -> String {
    error.to_string()
}

/// A report carrying a session and a result is the turn it reports.
#[test]
fn a_report_carrying_a_session_and_a_result_is_a_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let report = one_real_report("reports-answered");
    let session = report
        .session
        .clone()
        .expect("a run under a session handle answers a session block");

    let reported = TurnReport::of(report.clone(), &asked_about(&report), &asked_of())
        .expect("a finished run is the turn it reports");
    assert_eq!(reported.session().name, session.name);
    assert_eq!(
        reported.phase(),
        SessionPhase::Created,
        "the first turn of a print did not open its conversation"
    );
}

/// A report with no session block is a turn that cannot be written down.
#[test]
fn a_report_with_no_session_is_no_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let mut report = one_real_report("reports-sessionless");
    let asked = asked_about(&report);
    report.session = None;

    let refused = TurnReport::of(report, &asked, &asked_of())
        .expect_err("a report with no session was read as a turn");
    assert!(
        matches!(refused, SupervisorError::Unavailable { .. }),
        "a report with no session was refused as something else: {refused:?}"
    );
    assert!(
        detail(&refused).contains("exposed no session"),
        "the refusal does not say the conversation cannot be continued: {}",
        detail(&refused)
    );
}

/// A report answering more than once answers a run this port did not ask for.
#[test]
fn a_report_answering_more_than_once_is_no_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let mut report = one_real_report("reports-two-results");
    let asked = asked_about(&report);
    let answered = report
        .results
        .first()
        .expect("a finished run answers a result")
        .clone();
    report.results.push(answered);

    let refused = TurnReport::of(report, &asked, &asked_of())
        .expect_err("a report answering twice was read as one turn");
    assert!(
        detail(&refused).contains("2, and this turn asked one to answer once"),
        "the refusal does not say how many answered: {}",
        detail(&refused)
    );
}

/// A report with no result is a turn that cannot be written down.
#[test]
fn a_report_with_no_result_is_no_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let mut report = one_real_report("reports-resultless");
    let asked = asked_about(&report);
    report.results.clear();

    let refused = TurnReport::of(report, &asked, &asked_of())
        .expect_err("a report with no result was read as a turn");
    assert!(
        detail(&refused).contains("no result"),
        "the refusal does not say no turn was taken: {}",
        detail(&refused)
    );
}

/// A report about another session, or from another identity, is not this
/// print's turn however it came to be answered.
#[test]
fn a_report_about_another_session_or_identity_is_not_this_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let report = one_real_report("reports-mismatched");
    let asked = asked_about(&report);

    let elsewhere = TurnReport::of(report.clone(), "print-somebody-else", &asked_of())
        .expect_err("a report about another session was recorded as this turn");
    assert!(
        detail(&elsewhere).contains("this turn asked about `print-somebody-else`"),
        "the refusal does not say which session was asked about: {}",
        detail(&elsewhere)
    );

    let other_identity = HarnessIdentity::new(OTHER_HARNESS).expect("a second identity");
    let elsewhere = TurnReport::of(report, &asked, &other_identity)
        .expect_err("a report from another identity was recorded as this turn");
    assert!(
        detail(&elsewhere).contains(&format!("it was asked of `{OTHER_HARNESS}`")),
        "the refusal does not say which identity was asked: {}",
        detail(&elsewhere)
    );
}
