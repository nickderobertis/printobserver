//! What a server adopts on start, written into a store before it exists.
//!
//! This journey does not restart a server: it writes the four states a restart
//! leaves behind straight into a state directory and then starts a server over
//! it, which is the only way to be sure the outcomes are the *startup's* rather
//! than a residue of whatever wrote them.
//!
//! The four are a print left open, a session left open, an intervention already
//! past its expiry, and the value that intervention should restore. Each of the
//! three adoptions is asserted twice: in what the start reports, and in what it
//! wrote into the history — because a supervisor that adopted a print and said
//! nothing leaves a print that carried on across a restart looking exactly like
//! one that was started again.

use std::sync::Arc;

use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_server::{Ports, Server, ServerConfig};
use printobserver_store_api::{HistoryQuery, StorePort};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::{
    ActionRequest, Actor, Adjustable, EventKind, EventPayload, PolicyDecision, PrintAction,
    PrintId, StartupOutcome, Timestamp,
};
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::printer::{Call, RecordingPrinter};
use crate::world::{document, write};

/// The value the print was running at before the intervention changed it.
const PRIOR: f64 = 1.0;

/// The value the intervention left the machine holding.
const APPLIED: f64 = 1.3;

/// Write the four states a restart leaves behind into one state directory.
async fn left_behind(state_dir: &std::path::Path) -> PrintId {
    let store = SqliteStore::open(state_dir).expect("the store opens");

    let print = store
        .open_print(Some(4211), Some("benchy.gcode".to_owned()))
        .await
        .expect("a print opens");

    store
        .put_session(printobserver_types::SupervisionSession {
            print_id: print.id,
            session_name: "watch-4211".to_owned(),
            harness_identity: "claude-code".to_owned(),
            created_at: Timestamp::now(),
            last_turn_at: Timestamp::now(),
            closed_at: None,
            close_reason: None,
        })
        .await
        .expect("a session is written");

    let action = store
        .record_action(
            ActionRequest {
                action: PrintAction::SetFeedrateFactor {
                    factor: APPLIED,
                    duration_s: Some(30),
                    reason: "before the restart".to_owned(),
                    actor: Actor::Operator,
                },
                actor: Actor::Operator,
                requested_at: Timestamp::now(),
            },
            PolicyDecision::Accepted,
        )
        .await
        .expect("an action is recorded");

    store
        .open_intervention(
            action.id,
            Adjustable::Feedrate,
            Some(PRIOR),
            APPLIED,
            Timestamp::from_unix_seconds(1_700_000_000).expect("an instant"),
            // Already past its expiry when the server starts.
            Timestamp::from_unix_seconds(1_700_000_030).expect("an instant"),
        )
        .await
        .expect("an intervention opens");

    print.id
}

/// A start adopts the print, resumes the session, expires what is overdue, and
/// records each of the three.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_adopts_what_the_store_holds_and_records_each_adoption() {
    let root = TempDir::new().expect("a journey's own root");
    let path = write(root.path(), &document(root.path(), "http://127.0.0.1:1"));
    let config = ServerConfig::load(&path).expect("the configuration is accepted");
    let print_id = left_behind(&config.state_dir).await;

    let printer = RecordingPrinter::printing();
    let store: Arc<dyn StorePort> =
        Arc::new(SqliteStore::open(&config.state_dir).expect("the store reopens"));
    let running = Server::start_with(
        config,
        Ports {
            printer: Arc::clone(&printer) as Arc<dyn printobserver_printer_api::PrinterPort>,
            store: Arc::clone(&store),
            vision: Arc::new(
                ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"),
            ),
            agent: StandInAgent::new() as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("the server starts over what was left behind");

    let adopted = running.reconciliation();
    assert_eq!(
        adopted.adopted,
        vec![print_id],
        "the print left open was not adopted"
    );
    assert_eq!(
        adopted.resumed,
        vec![print_id],
        "the session left open was not resumed"
    );
    assert_eq!(
        adopted.expired.len(),
        1,
        "the intervention past its expiry was not expired: {adopted:?}"
    );

    // The value that intervention should restore went back, through the
    // ordinary policy: the machine was asked for the prior value and nothing
    // else.
    assert!(
        printer.calls().contains(&Call::Feedrate(PRIOR)),
        "the value the intervention should restore did not go back: {:?}",
        printer.calls()
    );
    assert_eq!(
        printer.value_of(Adjustable::Feedrate),
        Some(PRIOR),
        "the machine is not holding the value the intervention should have restored"
    );
    assert!(
        store
            .active_interventions(print_id)
            .await
            .expect("the interventions read")
            .is_empty(),
        "the intervention past its expiry is still active"
    );

    // Each of the three is recorded as having happened at startup.
    let recorded: Vec<StartupOutcome> = store
        .history(HistoryQuery {
            print_id,
            kinds: vec![EventKind::StartupReconciliation],
            since: None,
            until: None,
            limit: None,
        })
        .await
        .expect("the history reads")
        .into_iter()
        .filter_map(|event| match event.payload {
            EventPayload::StartupReconciliation(payload) => Some(payload.outcome),
            _ => None,
        })
        .collect();

    assert!(
        recorded.contains(&StartupOutcome::PrintAdopted),
        "adopting the print was not recorded: {recorded:?}"
    );
    assert!(
        recorded.iter().any(|outcome| matches!(
            outcome,
            StartupOutcome::SessionResumed { session_name } if session_name == "watch-4211"
        )),
        "resuming the session was not recorded: {recorded:?}"
    );
    assert!(
        recorded.iter().any(|outcome| matches!(
            outcome,
            StartupOutcome::InterventionExpired { adjustable, outcome, .. }
                if *adjustable == Adjustable::Feedrate
                    && *outcome == printobserver_types::InterventionOutcome::Restored
        )),
        "expiring the intervention and restoring its value was not recorded: {recorded:?}"
    );

    running.stop().await;
}

/// A start over a store holding nothing adopts nothing and records nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_over_an_empty_store_adopts_nothing() {
    let root = TempDir::new().expect("a journey's own root");
    let path = write(root.path(), &document(root.path(), "http://127.0.0.1:1"));
    let config = ServerConfig::load(&path).expect("the configuration is accepted");
    let store: Arc<dyn StorePort> =
        Arc::new(SqliteStore::open(&config.state_dir).expect("the store opens"));

    let running = Server::start_with(
        config,
        Ports {
            printer: RecordingPrinter::printing()
                as Arc<dyn printobserver_printer_api::PrinterPort>,
            store,
            vision: Arc::new(
                ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"),
            ),
            agent: StandInAgent::new() as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("the server starts");

    assert_eq!(
        running.reconciliation(),
        &printobserver_server::Reconciliation::default(),
        "a start over an empty store adopted something"
    );
    running.stop().await;
}

/// A print with no session, and one whose session was closed, are adopted
/// without being resumed.
///
/// Resuming a session that was closed would reopen a conversation somebody
/// ended, and there is nothing to resume for a print that never had one — but
/// both prints are still adopted, because a print with no end recorded is one
/// this supervisor goes on watching whatever else it holds.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_with_no_open_session_is_adopted_without_being_resumed() {
    let root = TempDir::new().expect("a journey's own root");
    let path = write(root.path(), &document(root.path(), "http://127.0.0.1:1"));
    let config = ServerConfig::load(&path).expect("the configuration is accepted");

    let (silent, closed) = {
        let store = SqliteStore::open(&config.state_dir).expect("the store opens");
        let silent = store
            .open_print(Some(1), None)
            .await
            .expect("a print opens")
            .id;
        let closed = store
            .open_print(Some(2), None)
            .await
            .expect("a second print opens")
            .id;
        store
            .put_session(printobserver_types::SupervisionSession {
                print_id: closed,
                session_name: "watch-2".to_owned(),
                harness_identity: "claude-code".to_owned(),
                created_at: Timestamp::now(),
                last_turn_at: Timestamp::now(),
                closed_at: Some(Timestamp::now()),
                close_reason: Some("the print ended".to_owned()),
            })
            .await
            .expect("a closed session is written");
        (silent, closed)
    };

    let store: Arc<dyn StorePort> =
        Arc::new(SqliteStore::open(&config.state_dir).expect("the store reopens"));
    let running = Server::start_with(
        config,
        Ports {
            printer: RecordingPrinter::printing()
                as Arc<dyn printobserver_printer_api::PrinterPort>,
            store,
            vision: Arc::new(
                ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"),
            ),
            agent: StandInAgent::new() as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("the server starts");

    let adopted = running.reconciliation();
    assert!(
        adopted.adopted.contains(&silent) && adopted.adopted.contains(&closed),
        "a print left open was not adopted: {adopted:?}"
    );
    assert!(
        adopted.resumed.is_empty(),
        "a session that was never open, or was closed, was resumed: {adopted:?}"
    );
    running.stop().await;
}
