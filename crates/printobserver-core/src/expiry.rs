//! A bounded intervention expires on its own, and nothing has to ask it to.
//!
//! The expiry driver is a thread [`Supervisor::new`] starts. It reads the
//! injected clock and nothing else, so expiry is driven by time passing and by
//! nothing else, and a test drives it by advancing that clock alone.
//!
//! # Time passing, not the wall clock moving
//!
//! An intervention's deadline is persisted as an absolute instant, so that a
//! restart knows when it was due. But a live expiry is decided on
//! [`Clock::elapsed`](crate::Clock::elapsed), which nothing steps: each
//! active intervention's remaining time is taken **once** from its persisted
//! deadline — when this supervisor applies it, or, for one a previous
//! supervisor left active, against the [`ClockReading`] this one took as it
//! started — and is counted down on the clock nothing steps from then on. A
//! host whose time synchronisation steps the wall clock therefore neither cuts
//! a bounded change short nor draws it out.
//!
//! Expiry has exactly three outcomes and this module is the one place they are
//! decided, so that the sweep and a print's terminal cleanup cannot drift into
//! handling different sets of them:
//!
//! * **Restored** — the prior value went back, bounded by the same policy the
//!   original request passed.
//! * **Restore unavailable** — the printer reported no prior value, so there is
//!   nothing to put back and nothing is sent to the machine. Writing a guessed
//!   default would be inventing a value nobody observed.
//! * **Restore failed** — the restoration was refused, by policy or by the
//!   printer. The intervention is **settled** rather than left active, because
//!   the expiry did happen, and it reads back carrying the applied value the
//!   printer was left holding beside the prior value it should have been
//!   returned to: a reason string alone would not say what the machine is
//!   holding now, which is the whole point of recording it.
//!
//! Nothing here retries. A retry would be machinery this crate's manifest rule
//! forbids it, and absorbing a transient store failure belongs behind the store
//! port rather than here.

use core::time::Duration;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::records::{
    Actor, Intervention, InterventionId, InterventionOutcome, PolicyDecision, PrintAction,
    RejectionReason,
};
use printobserver_printer_api::Adjustable;
use printobserver_types::{PrintId, Timestamp};

use crate::clock::ClockReading;
use crate::decision::{DecisionInput, decide};
use crate::error::CoreError;
use crate::store::{EventDraft, SettleOutcome};
use crate::supervisor::Supervisor;

/// The active interventions this supervisor is counting down, each with the
/// reading of [`Clock::elapsed`](crate::Clock::elapsed) it expires at.
#[derive(Debug, Default)]
pub(crate) struct Deadlines {
    /// Each intervention, by its identifier, and when it expires.
    counting: Mutex<BTreeMap<InterventionId, (Intervention, Duration)>>,
    /// Whether the interventions a previous supervisor left active have been
    /// read and are being counted down.
    adopted: AtomicBool,
}

impl Deadlines {
    /// Count one intervention down from `reading`, replacing any count of it.
    fn count(&self, intervention: &Intervention, reading: ClockReading) {
        let due = reading.elapsed_at(intervention.expires_at);
        self.counting()
            .insert(intervention.id, (intervention.clone(), due));
    }

    /// Count one intervention down from `reading`, unless it is already.
    fn adopt(&self, intervention: &Intervention, reading: ClockReading) {
        let due = reading.elapsed_at(intervention.expires_at);
        self.counting()
            .entry(intervention.id)
            .or_insert_with(|| (intervention.clone(), due));
    }

    /// Stop counting one intervention down.
    fn forget(&self, intervention_id: InterventionId) {
        self.counting().remove(&intervention_id);
    }

    /// Every intervention whose count has reached `elapsed`, soonest first.
    fn due(&self, elapsed: Duration) -> Vec<Intervention> {
        let mut due: Vec<(Duration, Intervention)> = self
            .counting()
            .values()
            .filter(|(_, at)| *at <= elapsed)
            .map(|(intervention, at)| (*at, intervention.clone()))
            .collect();
        due.sort_by_key(|(at, _)| *at);
        due.into_iter()
            .map(|(_, intervention)| intervention)
            .collect()
    }

    /// The counts, held for one read or write.
    fn counting(
        &self,
    ) -> std::sync::MutexGuard<'_, BTreeMap<InterventionId, (Intervention, Duration)>> {
        self.counting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The reason a restoring request carries, so the record says why it was made.
const RESTORE_REASON: &str = "the bounded intervention expired";

/// The request that puts one adjustable back to the value the print started at.
///
/// It carries no duration: a restoration opens no intervention of its own.
#[must_use]
pub fn restoring_action(adjustable: Adjustable, prior_value: f64) -> PrintAction {
    match adjustable {
        Adjustable::Feedrate => PrintAction::SetFeedrateFactor {
            factor: prior_value,
            duration_s: None,
            reason: RESTORE_REASON.to_owned(),
            actor: Actor::System,
        },
        Adjustable::Flowrate => PrintAction::SetFlowrateFactor {
            factor: prior_value,
            duration_s: None,
            reason: RESTORE_REASON.to_owned(),
            actor: Actor::System,
        },
        Adjustable::ToolTarget { tool } => PrintAction::SetToolTargetC {
            tool,
            target_c: prior_value,
            duration_s: None,
            reason: RESTORE_REASON.to_owned(),
            actor: Actor::System,
        },
        Adjustable::BedTarget => PrintAction::SetBedTargetC {
            target_c: prior_value,
            duration_s: None,
            reason: RESTORE_REASON.to_owned(),
            actor: Actor::System,
        },
        Adjustable::Fan => PrintAction::SetFanPercent {
            percent: prior_value,
            duration_s: None,
            reason: RESTORE_REASON.to_owned(),
            actor: Actor::System,
        },
    }
}

/// Why policy refused a request, in words a reader of the record can act on.
#[must_use]
pub fn rejection_detail(reason: &RejectionReason) -> String {
    match reason {
        RejectionReason::OutOfBounds {
            adjustable,
            requested,
            allowed,
        } => format!(
            "{requested} is outside the {} to {} allowed for {adjustable}",
            allowed.min, allowed.max
        ),
        RejectionReason::ActorMayNotRequest {
            actor_class,
            action,
        } => format!("{actor_class:?} may not request {action:?}"),
        RejectionReason::NoActivePrint => "there is no active print to act on".to_owned(),
        RejectionReason::InvalidFromState { state } => {
            format!("this is not valid from {state:?}")
        }
        RejectionReason::MinIntervalNotElapsed {
            interval_s,
            since_last_s,
        } => format!(
            "the agent last acted {since_last_s}s ago and its minimum interval is {interval_s}s"
        ),
        RejectionReason::UnsupportedAdjustable { adjustable } => {
            format!("this printer has no {adjustable}")
        }
        RejectionReason::InsufficientFilament {
            tool,
            spool_name,
            required_g,
            buffer_g,
            remaining_g,
            ..
        } => format!(
            "spool {spool_name} on tool {tool} holds {remaining_g}g, and the print needs \
             {required_g}g and a {buffer_g}g buffer"
        ),
        RejectionReason::UnverifiedSpool {
            tool, spool_name, ..
        } => format!("spool {spool_name} on tool {tool} is unverified"),
        RejectionReason::AnalysisUnavailable {
            file_name,
            waited_s,
        } => format!("{file_name} had no analysis after waiting {waited_s}s for one"),
        RejectionReason::SpoolRefused { detail } => detail.clone(),
    }
}

impl Supervisor {
    /// Expire every bounded intervention whose time has passed.
    ///
    /// One intervention's failure does not cost the rest: every one that is due
    /// is attempted on the same sweep.
    ///
    /// The expiry driver calls this on its own poll. Its first sweep reads the
    /// interventions a previous supervisor left active and counts each down
    /// from the reading this supervisor took as it started; every later one
    /// reads nothing, because every intervention opened since is counted down
    /// from the moment it was applied.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the interventions left active could
    /// not be read, which the next sweep reads again. A failure settling one of
    /// them is recorded on that intervention rather than answered here.
    pub async fn sweep_expired(&self) -> Result<(), CoreError> {
        self.adopt_left_active().await?;
        for intervention in self.deadlines().due(self.clock().elapsed()) {
            let _ = self.expire_intervention(&intervention).await;
        }
        Ok(())
    }

    /// Count down every intervention a previous supervisor left active, once.
    async fn adopt_left_active(&self) -> Result<(), CoreError> {
        let deadlines = self.deadlines();
        if deadlines.adopted.load(Ordering::Acquire) {
            return Ok(());
        }
        // Every active intervention is due by the latest instant a deadline can
        // be recorded at, so this reads every one.
        for intervention in self
            .stores()
            .actions
            .due_interventions(Timestamp::latest())
            .await?
        {
            deadlines.adopt(&intervention, self.started());
        }
        deadlines.adopted.store(true, Ordering::Release);
        Ok(())
    }

    /// Count one intervention this supervisor just applied down from now.
    pub(crate) fn count_down(&self, intervention: &Intervention) {
        self.deadlines()
            .count(intervention, ClockReading::of(self.clock().as_ref()));
    }

    /// Stop counting down one intervention settled other than by expiring.
    pub(crate) fn stop_counting_down(&self, intervention_id: InterventionId) {
        self.deadlines().forget(intervention_id);
    }

    /// Expire every intervention still active on one print.
    ///
    /// Each takes the same three-way behaviour an ordinary expiry takes, and
    /// one that fails does not cost the rest.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the print's active interventions
    /// could not be read.
    pub(crate) async fn expire_all_active(&self, print_id: PrintId) -> Result<(), CoreError> {
        for intervention in self.stores().actions.active_interventions(print_id).await? {
            let _ = self.expire_intervention(&intervention).await;
        }
        Ok(())
    }

    /// Expire one intervention: restore, report nothing to restore, or record
    /// that the restoration failed.
    ///
    /// Settling is once-only, so an intervention this and the expiry driver
    /// both reach answers the outcome that won rather than being restored
    /// twice. That is what lets a composition root expire what a restart left
    /// past its bound and record the outcome, whichever of the two got there
    /// first.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the outcome could not be settled.
    pub async fn expire_intervention(
        &self,
        intervention: &Intervention,
    ) -> Result<InterventionOutcome, CoreError> {
        let outcome = self.expire_once(intervention).await?;
        self.deadlines().forget(intervention.id);
        Ok(outcome)
    }

    /// Expire one intervention, whatever was counting it down.
    async fn expire_once(
        &self,
        intervention: &Intervention,
    ) -> Result<InterventionOutcome, CoreError> {
        if intervention.outcome != InterventionOutcome::StillActive {
            return Ok(intervention.outcome.clone());
        }
        // Another expiry may have reached it first — the driver's sweep, a
        // close-out, a start's reconciliation — so it is read again once this
        // one holds the print's expiries, and one already settled is answered
        // with the outcome that won rather than put back a second time.
        let _expiring = self.expiring().acquire(intervention.print_id).await;
        let still_active = self
            .stores()
            .actions
            .active_interventions(intervention.print_id)
            .await?
            .iter()
            .any(|active| active.id == intervention.id);
        if !still_active {
            // Settling is once-only, so this writes nothing and answers the
            // outcome already settled.
            return self
                .settle(intervention, InterventionOutcome::RestoreUnavailable)
                .await;
        }
        let Some(prior_value) = intervention.prior_value else {
            return self
                .settle(intervention, InterventionOutcome::RestoreUnavailable)
                .await;
        };
        let outcome = self.restore(intervention, prior_value).await?;
        self.settle(intervention, outcome).await
    }

    /// Put one adjustable back, bounded by the same policy the request passed.
    async fn restore(
        &self,
        intervention: &Intervention,
        prior_value: f64,
    ) -> Result<InterventionOutcome, CoreError> {
        let requested_at = self.clock().now();
        let action = restoring_action(intervention.adjustable, prior_value);
        let actor = action.actor().clone();
        let print = self.stores().prints.print(intervention.print_id).await?;
        let bounds = self
            .bounds_for(print.as_ref(), intervention.print_id)
            .await?;
        let decision = decide(&DecisionInput {
            action: &action,
            actor: &actor,
            requested_at,
            print: print.as_ref(),
            // A restoration is judged from no state: see `DecisionInput`.
            printer_state: None,
            bounds: &bounds.effective,
            envelope: &self.config().envelope,
            since_last_agent_action: None,
        });
        let request = crate::records::ActionRequest {
            action,
            actor,
            requested_at,
        };
        let issued = self.issue_decided_action(request, decision).await?;
        if let PolicyDecision::Rejected(reason) = &issued.record.decision {
            return Ok(InterventionOutcome::RestoreFailed {
                reason: rejection_detail(reason),
            });
        }
        if let Some(error) = issued.printer_error() {
            return Ok(InterventionOutcome::RestoreFailed {
                reason: error.to_string(),
            });
        }
        Ok(InterventionOutcome::Restored)
    }

    /// Settle one intervention, answering the outcome that won.
    async fn settle(
        &self,
        intervention: &Intervention,
        outcome: InterventionOutcome,
    ) -> Result<InterventionOutcome, CoreError> {
        let settled = self
            .stores()
            .actions
            .settle_intervention(intervention.id, outcome)
            .await?;
        Ok(match settled {
            // Written into the print's own history, because what became of a
            // bounded change is the whole point of it having been bounded: an
            // outcome that reached the intervention row and went no further
            // would be one no read of this system could answer.
            SettleOutcome::Settled {
                intervention: settled,
            } => {
                self.append_expiry_event(intervention, settled.outcome.clone())
                    .await?;
                settled.outcome
            }
            SettleOutcome::AlreadySettled { outcome } => outcome,
        })
    }

    /// Write what became of one bounded intervention into the print's history.
    async fn append_expiry_event(
        &self,
        intervention: &Intervention,
        outcome: InterventionOutcome,
    ) -> Result<(), CoreError> {
        self.stores()
            .events
            .append_event(EventDraft {
                print_id: Some(intervention.print_id),
                source: crate::kinds::system_source(),
                received_at: self.clock().now(),
                body: printobserver_types::EventBody::of(
                    &crate::kinds::InterventionExpiredPayload {
                        intervention_id: intervention.id,
                        adjustable: intervention.adjustable,
                        outcome,
                    },
                )?,
                raw: None,
            })
            .await?;
        Ok(())
    }
}
