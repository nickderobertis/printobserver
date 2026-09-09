//! What the port makes of a finished run's report, including the two shapes a
//! turn cannot be written down from.
//!
//! `OneHarness`'s report holds the session block and the results behind an
//! `Option` each, because the same document describes runs this port never asks
//! for. Every report here is one a real run answered, read off the port's own
//! seam; the two that a turn cannot be recorded from are that report with the
//! part in question taken away, which is the only way this system meets either
//! of them.

use std::sync::Arc;

use oneharness_core::domain::report::RunReport;
use printobserver_oneharness::TurnReport;
use printobserver_supervisor_api::{SupervisorError, SupervisorPort};
use printobserver_types::{EventPayload, MalformedExternalEventPayload, PrintId, SessionPhase};

use crate::support::{
    Fixture, HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, port, schema_read_lock, turn,
};

/// An event to hang a turn off.
fn payload() -> EventPayload {
    EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
        detail: "the body was not JSON".to_owned(),
    })
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

    let reported = TurnReport::of(report).expect("a finished run is the turn it reports");
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
    report.session = None;

    let refused = TurnReport::of(report).expect_err("a report with no session was read as a turn");
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

/// A report with no result is a turn that cannot be written down.
#[test]
fn a_report_with_no_result_is_no_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let mut report = one_real_report("reports-resultless");
    report.results.clear();

    let refused = TurnReport::of(report).expect_err("a report with no result was read as a turn");
    assert!(
        detail(&refused).contains("no result"),
        "the refusal does not say no turn was taken: {}",
        detail(&refused)
    );
}
