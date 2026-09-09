//! What a supervisor adopts when it starts.
//!
//! Restarting loses nothing, because the store and the session directory are
//! the whole of the state. This is what makes that true rather than merely
//! claimed: on start the server reads what the store holds and adopts it — a
//! print left open is the print it goes on watching, a session left open is
//! resumed rather than replaced, and an intervention already past its expiry is
//! expired through the ordinary policy so the value it should restore is
//! restored.
//!
//! Each of the three is **recorded** as having happened at startup, under the
//! [`StartupReconciliation`](printobserver_types::EventKind::StartupReconciliation)
//! kind, because a print that carried on across a restart and one that was
//! started again are otherwise indistinguishable in the history.
//!
//! # The due interventions are read before the supervisor exists
//!
//! Building a supervisor starts its expiry driver, which polls for what is due.
//! So the interventions this reconciliation is about are read from the store
//! first, and then expired through
//! [`Supervisor::expire_intervention`](printobserver_core::Supervisor::expire_intervention),
//! which settles once: if the driver reached one first, this answers the
//! outcome that won rather than restoring it a second time.

use std::sync::Arc;

use printobserver_core::{CoreError, Supervisor};
use printobserver_store_api::{EventDraft, StorePort};
use printobserver_types::{
    EventPayload, EventSource, Intervention, InterventionId, PrintId, StartupOutcome,
    StartupReconciliationPayload, Timestamp,
};

/// What one start adopted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// Every print left open, which this supervisor goes on watching.
    pub adopted: Vec<PrintId>,
    /// Every print whose session was resumed rather than replaced.
    pub resumed: Vec<PrintId>,
    /// Every intervention that had outlived its bound and was expired.
    pub expired: Vec<InterventionId>,
}

/// The interventions already past their expiry, read before the supervisor
/// exists to start expiring them.
///
/// # Errors
///
/// Returns the store's own error when they could not be read.
pub async fn overdue(
    store: &Arc<dyn StorePort>,
    at: Timestamp,
) -> Result<Vec<Intervention>, CoreError> {
    Ok(store.due_interventions(at).await?)
}

/// Adopt what the store holds, and record each adoption.
///
/// # Errors
///
/// Returns the store's own error when the open prints could not be read or an
/// adoption could not be recorded. An intervention that could not be expired is
/// recorded rather than answered here: one that fails does not cost the rest,
/// which is the same rule the ordinary expiry sweep takes.
pub async fn reconcile(
    supervisor: &Arc<Supervisor>,
    store: &Arc<dyn StorePort>,
    overdue: Vec<Intervention>,
) -> Result<Reconciliation, CoreError> {
    let mut found = Reconciliation::default();

    for print in store.open_prints().await? {
        record(store, print.id, StartupOutcome::PrintAdopted).await?;
        found.adopted.push(print.id);
        let Some(session) = store.session(print.id).await? else {
            continue;
        };
        if session.closed_at.is_some() {
            continue;
        }
        record(
            store,
            print.id,
            StartupOutcome::SessionResumed {
                session_name: session.session_name,
            },
        )
        .await?;
        found.resumed.push(print.id);
    }

    for intervention in overdue {
        let outcome = supervisor.expire_intervention(&intervention).await?;
        record(
            store,
            intervention.print_id,
            StartupOutcome::InterventionExpired {
                intervention_id: intervention.id,
                adjustable: intervention.adjustable,
                outcome,
            },
        )
        .await?;
        found.expired.push(intervention.id);
    }

    Ok(found)
}

/// Write one adoption down.
async fn record(
    store: &Arc<dyn StorePort>,
    print_id: PrintId,
    outcome: StartupOutcome,
) -> Result<(), CoreError> {
    store
        .append_event(EventDraft {
            print_id: Some(print_id),
            source: EventSource::System,
            received_at: Timestamp::now(),
            payload: EventPayload::StartupReconciliation(StartupReconciliationPayload {
                print_id,
                outcome,
            }),
            raw: None,
        })
        .await?;
    Ok(())
}
