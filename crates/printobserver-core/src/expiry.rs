//! A bounded intervention expires on its own, and nothing has to ask it to.
//!
//! The expiry driver is a thread [`Supervisor::new`] starts. It reads the
//! injected clock and nothing else, so expiry is driven by time passing and by
//! nothing else, and a test drives it by advancing that clock alone.
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

use printobserver_types::{
    Actor, Adjustable, Intervention, InterventionOutcome, PolicyDecision, PrintAction, PrintId,
    RejectionReason,
};

use crate::decision::{DecisionInput, decide};
use crate::error::CoreError;
use crate::supervisor::Supervisor;

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
    }
}

impl Supervisor {
    /// Expire every bounded intervention that is due at the clock's own instant.
    ///
    /// One intervention's failure does not cost the rest: every one that is due
    /// is attempted on the same sweep.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the due interventions could not be
    /// read. A failure settling one of them is recorded on that intervention
    /// rather than answered here.
    pub(crate) async fn sweep_expired(&self) -> Result<(), CoreError> {
        let at = self.clock().now();
        for intervention in self.store().due_interventions(at).await? {
            let _ = self.expire_intervention(&intervention).await;
        }
        Ok(())
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
        for intervention in self.store().active_interventions(print_id).await? {
            let _ = self.expire_intervention(&intervention).await;
        }
        Ok(())
    }

    /// Expire one intervention: restore, report nothing to restore, or record
    /// that the restoration failed.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the outcome could not be settled.
    pub(crate) async fn expire_intervention(
        &self,
        intervention: &Intervention,
    ) -> Result<InterventionOutcome, CoreError> {
        if intervention.outcome != InterventionOutcome::StillActive {
            return Ok(intervention.outcome.clone());
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
        let print = self.store().print(intervention.print_id).await?;
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
            last_agent_action: None,
        });
        let request = printobserver_types::ActionRequest {
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
            .store()
            .settle_intervention(intervention.id, outcome)
            .await?;
        Ok(match settled {
            printobserver_store_api::SettleOutcome::Settled { intervention } => {
                intervention.outcome
            }
            printobserver_store_api::SettleOutcome::AlreadySettled { outcome } => outcome,
        })
    }
}
