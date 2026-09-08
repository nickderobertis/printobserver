//! One session per print, whose continuation survives losing everything held in
//! memory.
//!
//! Two prints rather than one, because a port that used one fixture's session
//! name for everything — or mapped every print onto one shared session —
//! answers a created phase and then a continued one across a rebuild and
//! satisfies every assertion a one-print journey can make. What it cannot
//! satisfy is two names that differ, each derived from its own print's
//! identifier, each continued by its own print after the rebuild.

use std::sync::Arc;

use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{
    EventPayload, ObicoFailureAlertPayload, ObicoNotificationType, ObicoPrinterNotificationPayload,
    PrintId, SessionPhase,
};

use crate::support::{
    Fixture, HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, port, turn,
};

/// An alert about a print.
fn alert() -> EventPayload {
    EventPayload::ObicoFailureAlert(ObicoFailureAlertPayload {
        is_warning: false,
        print_paused: true,
        obico_print_id: Some(4_411),
        file_name: Some("bracket.gcode".to_owned()),
    })
}

/// A notification about a print.
fn notification() -> EventPayload {
    EventPayload::ObicoPrinterNotification(ObicoPrinterNotificationPayload {
        notification_type: ObicoNotificationType::Paused,
        obico_print_id: Some(4_411),
        file_name: Some("bracket.gcode".to_owned()),
    })
}

/// Each print keeps a session of its own, named for its own identifier, and
/// each continues that session after everything held in memory is dropped.
#[test]
fn each_print_keeps_its_own_session_across_a_rebuild() {
    let fixture = Fixture::new("sessions");
    let schema = generated_assessment_schema();
    let environment = always("SID-SESSIONS", &assessment("the print is fine", "high"));

    let first = PrintId::new();
    let second = PrintId::new();
    assert_ne!(first, second, "the two prints are distinct");

    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(&fixture, HARNESS, &schema, environment.clone()),
        &watch,
    );

    // Each print's first event opens its own session.
    let mut opened = Vec::new();
    for print_id in [first, second] {
        let outcome = block_on(supervisor.run_turn(turn(print_id, event(print_id, alert()), None)))
            .expect("the first turn of a print runs");
        assert_eq!(outcome.phase, SessionPhase::Created);
        opened.push(outcome.session.session_name);
    }

    // A later event of each print continues the session it opened.
    for (index, print_id) in [first, second].into_iter().enumerate() {
        let outcome =
            block_on(supervisor.run_turn(turn(print_id, event(print_id, notification()), None)))
                .expect("a later turn of a print runs");
        assert_eq!(outcome.phase, SessionPhase::Continued);
        assert_eq!(outcome.session.session_name, opened[index]);
    }

    // Everything held in memory goes, and the port is built again from the
    // state directory alone.
    drop(supervisor);
    drop(watch);
    let rebuilt_watch = Arc::new(Watch::default());
    let rebuilt = port(
        config(&fixture, HARNESS, &schema, environment),
        &rebuilt_watch,
    );

    for (index, print_id) in [first, second].into_iter().enumerate() {
        let outcome =
            block_on(rebuilt.run_turn(turn(print_id, event(print_id, notification()), None)))
                .expect("a turn after the rebuild runs");
        assert_eq!(
            outcome.phase,
            SessionPhase::Continued,
            "the rebuilt port did not continue the conversation of print {print_id}"
        );
        assert_eq!(outcome.session.session_name, opened[index]);
    }

    // The two names differ, and each is derived from its own print's identifier
    // rather than from the other's or from anything this journey supplied.
    assert_ne!(opened[0], opened[1]);
    assert_eq!(opened[0], format!("print-{first}"));
    assert_eq!(opened[1], format!("print-{second}"));
    assert!(!opened[0].contains(&second.to_string()));
    assert!(!opened[1].contains(&first.to_string()));

    // And each print's ledger holds exactly the one session it opened.
    for (index, print_id) in [first, second].into_iter().enumerate() {
        let sessions = rebuilt
            .recorded_sessions(&print_id)
            .expect("the ledger is readable");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_name, opened[index]);
        assert_eq!(sessions[0].print_id, print_id);
        assert_eq!(sessions[0].closed_at, None);
    }
}
