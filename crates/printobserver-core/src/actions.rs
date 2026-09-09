//! The one path an action takes, whoever asked for it.
//!
//! # An action is written into the history as it happens
//!
//! Every request appends [`EventKind::ActionRequested`], carrying the whole
//! action — its own values and the reason the actor gave for it. A rejection
//! appends the decision beside it and an execution appends what it opened. The
//! reason a mutating request is required to carry is what makes the history
//! worth reading afterwards, and a reason that reached the `actions` table and
//! no further would be a reason nothing could ever read back.
//!
//! The agent, an operator, the command line and a client all arrive here. There
//! is no second path: [`Supervisor::request_action`] gathers what the decision
//! is taken from, takes it, and hands the pair to
//! [`Supervisor::issue_decided_action`], which is the only function of this
//! crate that reaches an action method of the printer port.

use printobserver_types::{
    ActionExecutedPayload, ActionRecord, ActionRejectedPayload, ActionRequest,
    ActionRequestedPayload, Actor, ActorClass, Adjustable, EventPayload, EventSource, Intervention,
    InterventionOutcome, PolicyDecision, PrintAction, PrintId, PrintRecord, PrinterSnapshot,
    Timestamp,
};

use crate::bounds::{Bounds, effective_bounds};
use crate::clock::plus_seconds;
use crate::decision::{DecisionInput, adjustment, decide};
use crate::error::CoreError;
use crate::supervisor::{Issued, Supervisor};

/// What became of one request, from the decision through to its intervention.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionOutcome {
    /// The record of the request and the decision taken on it.
    pub record: ActionRecord,
    /// What the printer made of it, absent when it was rejected.
    pub executed: Option<Result<(), printobserver_printer_api::PrinterError>>,
    /// The bounded intervention it opened, when it opened one.
    pub intervention: Option<Intervention>,
}

impl ActionOutcome {
    /// The decision policy took on the request.
    #[must_use]
    pub const fn decision(&self) -> &PolicyDecision {
        &self.record.decision
    }
}

/// Where an event about one actor's action came from.
const fn source_of(actor: &Actor) -> EventSource {
    match actor.class() {
        ActorClass::Agent => EventSource::Agent,
        ActorClass::Operator => EventSource::Operator,
        ActorClass::System => EventSource::System,
    }
}

/// How long a bounded change stands for, when the request bounds it.
fn duration_of(action: &PrintAction) -> Option<i64> {
    match action {
        PrintAction::SetFeedrateFactor { duration_s, .. }
        | PrintAction::SetFlowrateFactor { duration_s, .. }
        | PrintAction::SetToolTargetC { duration_s, .. }
        | PrintAction::SetBedTargetC { duration_s, .. }
        | PrintAction::SetFanPercent { duration_s, .. } => *duration_s,
        PrintAction::Pause { .. }
        | PrintAction::Resume { .. }
        | PrintAction::Cancel { .. }
        | PrintAction::StartPrint { .. }
        | PrintAction::AcknowledgeFailure { .. } => None,
    }
}

/// What the printer reported for one adjustable, when it reported anything.
///
/// Absent means the printer reported no value for it, which is what makes an
/// intervention's expiry restore nothing and say so rather than writing a
/// guessed default onto the machine.
#[must_use]
pub fn reported_value(snapshot: &PrinterSnapshot, adjustable: Adjustable) -> Option<f64> {
    let reported = match adjustable {
        Adjustable::Feedrate => snapshot.feedrate_factor,
        Adjustable::Flowrate => snapshot.flowrate_factor,
        Adjustable::Fan => snapshot.fan_percent,
        Adjustable::BedTarget => snapshot.bed.as_ref().and_then(|heater| heater.target_c),
        Adjustable::ToolTarget { tool } => usize::try_from(tool)
            .ok()
            .and_then(|index| snapshot.tools.get(index))
            .and_then(|heater| heater.target_c),
    };
    reported.map(|value| value.value())
}

impl Supervisor {
    /// Request one action against one print.
    ///
    /// The same call an operator's action makes, an agent's action makes and a
    /// client's action makes. A rejected request reaches no action method of
    /// the printer port at all, and the rejection is persisted carrying its own
    /// reason so that the caller can ask again inside the range.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the decision, the execution or the
    /// intervention could not be recorded. A refusal by the printer is not an
    /// error: it is recorded and reported in [`ActionOutcome::executed`].
    pub async fn request_action(
        &self,
        print_id: PrintId,
        action: PrintAction,
    ) -> Result<ActionOutcome, CoreError> {
        let requested_at = self.clock().now();
        let actor = action.actor().clone();
        let print = self.store().print(print_id).await?;
        let bounds = self.bounds_for(print.as_ref(), print_id).await?;
        let snapshot = self.read_snapshot().await.ok();
        let decision = decide(&DecisionInput {
            action: &action,
            actor: &actor,
            requested_at,
            print: print.as_ref(),
            printer_state: snapshot.as_ref().map(|taken| &taken.connection),
            bounds: &bounds.effective,
            envelope: &self.config().envelope,
            last_agent_action: self.last_agent_action(print_id),
        });
        let request = ActionRequest {
            action: action.clone(),
            actor: actor.clone(),
            requested_at,
        };
        let issued = self.issue_decided_action(request, decision).await?;
        let source = source_of(&actor);
        self.append_action_event(
            print_id,
            source,
            EventPayload::ActionRequested(ActionRequestedPayload {
                action_id: issued.record.id,
                action: action.clone(),
                actor: actor.clone(),
            }),
        )
        .await?;
        if let PolicyDecision::Rejected(_) = &issued.record.decision {
            self.append_action_event(
                print_id,
                source,
                EventPayload::ActionRejected(ActionRejectedPayload {
                    action_id: issued.record.id,
                    decision: issued.record.decision.clone(),
                }),
            )
            .await?;
        }
        if !issued.succeeded() {
            return Ok(ActionOutcome {
                record: issued.record,
                executed: issued.executed,
                intervention: None,
            });
        }
        if actor.class() == ActorClass::Agent {
            self.note_agent_action(print_id, requested_at);
        }
        if let PrintAction::StartPrint { manifest, .. } = &action {
            self.attach_manifest(print_id, manifest.clone()).await?;
        }
        let intervention = self
            .open_bounded_intervention(print_id, &action, &issued, snapshot.as_ref(), requested_at)
            .await?;
        self.append_action_event(
            print_id,
            source,
            EventPayload::ActionExecuted(ActionExecutedPayload {
                action_id: issued.record.id,
                intervention_id: intervention.as_ref().map(|opened| opened.id),
            }),
        )
        .await?;
        Ok(ActionOutcome {
            record: issued.record,
            executed: issued.executed,
            intervention,
        })
    }

    /// Append one event about an action, sourced from whoever asked for it.
    async fn append_action_event(
        &self,
        print_id: PrintId,
        source: EventSource,
        payload: EventPayload,
    ) -> Result<(), CoreError> {
        self.store()
            .append_event(printobserver_store_api::EventDraft {
                print_id: Some(print_id),
                source,
                received_at: self.clock().now(),
                payload,
                raw: None,
            })
            .await?;
        Ok(())
    }

    /// The bounds in force for one print: the envelope, narrowed by its manifest.
    pub(crate) async fn bounds_for(
        &self,
        print: Option<&PrintRecord>,
        print_id: PrintId,
    ) -> Result<Bounds, CoreError> {
        let manifest = if print.is_some() {
            self.store().manifest(print_id).await?
        } else {
            None
        };
        Ok(effective_bounds(&self.config().envelope, manifest.as_ref()))
    }

    /// Store a print's manifest, recording every range it asked wider than the
    /// envelope's, so that nobody has to wonder later which bound applied.
    async fn attach_manifest(
        &self,
        print_id: PrintId,
        manifest: printobserver_types::JobManifest,
    ) -> Result<(), CoreError> {
        let narrowed = effective_bounds(&self.config().envelope, Some(&manifest));
        self.store().put_manifest(print_id, manifest).await?;
        for narrowing in narrowed.narrowings {
            self.store().record_narrowing(print_id, narrowing).await?;
        }
        Ok(())
    }

    /// Open the bounded intervention an accepted adjustment asked for.
    ///
    /// An adjustable changed again before its expiry supersedes the earlier
    /// intervention rather than stacking with it, and the later one carries the
    /// earlier's prior value forward so that restoring still reaches the value
    /// the print started from.
    async fn open_bounded_intervention(
        &self,
        print_id: PrintId,
        action: &PrintAction,
        issued: &Issued,
        snapshot: Option<&PrinterSnapshot>,
        applied_at: Timestamp,
    ) -> Result<Option<Intervention>, CoreError> {
        let (Some((adjustable, applied_value)), Some(duration_s)) =
            (adjustment(action), duration_of(action))
        else {
            return Ok(None);
        };
        let mut superseded = self.store().active_interventions(print_id).await?;
        superseded.retain(|held| held.adjustable == adjustable);
        superseded.sort_by_key(|held| held.applied_at);
        let prior_value = superseded.first().map_or_else(
            || snapshot.and_then(|taken| reported_value(taken, adjustable)),
            |earliest| earliest.prior_value,
        );
        let expires_at =
            plus_seconds(applied_at, duration_s).map_err(|error| CoreError::Unrepresentable {
                detail: error.to_string(),
            })?;
        let opened = self
            .store()
            .open_intervention(
                issued.record.id,
                adjustable,
                prior_value,
                applied_value,
                applied_at,
                expires_at,
            )
            .await?;
        for earlier in superseded {
            self.store()
                .settle_intervention(
                    earlier.id,
                    InterventionOutcome::Superseded { by: opened.id },
                )
                .await?;
        }
        Ok(Some(opened))
    }
}
