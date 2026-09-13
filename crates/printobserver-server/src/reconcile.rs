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
//! [`StartupReconciliationPayload`] kind this module declares and alone writes,
//! because a print that carried on across a restart and one that was started
//! again are otherwise indistinguishable in the history.
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

use printobserver_core::store::{ActionStore, EventDraft, EventStore, PrintStore, SessionStore};
use printobserver_core::{CoreError, Supervisor, system_source};
use printobserver_core::{Intervention, InterventionId, InterventionOutcome};
use printobserver_printer_api::Adjustable;
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventBody, EventPayload, PrintId, Timestamp};

/// What one restart put back the way it found it.
///
/// A supervisor that has been restarted adopts whatever the store holds rather
/// than starting empty, and each of these is one of those adoptions. They are
/// recorded rather than merely done, because a print that carried on across a
/// restart and one that was started again look identical afterwards unless the
/// history says which happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum StartupOutcome {
    /// A print left open was adopted as the print this supervisor is watching.
    PrintAdopted,
    /// A session left open was resumed rather than replaced.
    SessionResumed {
        /// The session's own name in the harness.
        session_name: String,
    },
    /// An intervention already past its expiry was expired on start.
    InterventionExpired {
        /// The intervention that had outlived its bound.
        intervention_id: InterventionId,
        /// What it had changed.
        adjustable: Adjustable,
        /// What became of putting the prior value back.
        outcome: InterventionOutcome,
    },
}

impl Sample for StartupOutcome {
    fn sample_full() -> Self {
        Self::InterventionExpired {
            intervention_id: InterventionId::sample_full(),
            adjustable: Adjustable::Fan,
            outcome: InterventionOutcome::Restored,
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::PrintAdopted,
            Self::SessionResumed {
                session_name: "watch-4211".to_owned(),
            },
        ]
    }
}

/// A supervisor reconciled one thing the store held when it started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct StartupReconciliationPayload {
    /// The print it is about.
    pub print_id: PrintId,
    /// What was reconciled.
    pub outcome: StartupOutcome,
}

impl EventPayload for StartupReconciliationPayload {
    const KIND: &'static str = "startup_reconciliation";
}

impl Sample for StartupReconciliationPayload {
    fn sample_full() -> Self {
        Self {
            print_id: PrintId::sample_full(),
            outcome: StartupOutcome::sample_full(),
        }
    }
}

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
    actions: &Arc<dyn ActionStore>,
    at: Timestamp,
) -> Result<Vec<Intervention>, CoreError> {
    Ok(actions.due_interventions(at).await?)
}

/// The three stores a reconciliation reads and writes: the prints it adopts,
/// the sessions it resumes, and the log it records each adoption in.
#[derive(Clone)]
pub struct ReconcileStores {
    /// The print records.
    pub prints: Arc<dyn PrintStore>,
    /// The supervision sessions.
    pub sessions: Arc<dyn SessionStore>,
    /// The event log.
    pub events: Arc<dyn EventStore>,
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
    stores: &ReconcileStores,
    overdue: Vec<Intervention>,
) -> Result<Reconciliation, CoreError> {
    let mut found = Reconciliation::default();

    for print in stores.prints.open_prints().await? {
        record(&stores.events, print.id, StartupOutcome::PrintAdopted).await?;
        found.adopted.push(print.id);
        let Some(session) = stores.sessions.session(print.id).await? else {
            continue;
        };
        if session.closed_at.is_some() {
            continue;
        }
        record(
            &stores.events,
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
            &stores.events,
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
    events: &Arc<dyn EventStore>,
    print_id: PrintId,
    outcome: StartupOutcome,
) -> Result<(), CoreError> {
    events
        .append_event(EventDraft {
            print_id: Some(print_id),
            source: system_source(),
            received_at: Timestamp::now(),
            body: EventBody::of(&StartupReconciliationPayload { print_id, outcome })?,
            raw: None,
        })
        .await?;
    Ok(())
}
