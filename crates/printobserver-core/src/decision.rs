//! The one decision, as a total function over what it is given.
//!
//! Nothing here reaches a port. The decision is a pure function of the request,
//! the print, the state the printer is in, the bounds in force and when the
//! agent last acted, so that the rule can be read in one place and the
//! chokepoint that issues actions has only to record what this answered.
//!
//! **A request outside the bounds is rejected, not clamped.** Silently clamping
//! would leave an agent believing it had done something it had not, and would
//! leave the operator's own record saying it asked for a value it never asked
//! for.

use printobserver_types::{
    ActionKind, Actor, ActorClass, Adjustable, EffectiveBounds, PolicyDecision, PrintAction,
    PrintRecord, PrinterState, RejectionReason, SafetyEnvelope, Timestamp,
};

use crate::clock::seconds_between;

/// Everything one decision is taken from.
#[derive(Debug, Clone, Copy)]
pub struct DecisionInput<'a> {
    /// What is being asked for.
    pub action: &'a PrintAction,
    /// Who is asking.
    pub actor: &'a Actor,
    /// When they asked.
    pub requested_at: Timestamp,
    /// The print it is being asked against, when there is one.
    pub print: Option<&'a PrintRecord>,
    /// The state to judge this request from, when there is one to judge it from.
    ///
    /// Absent means the state check does not apply: either no snapshot could be
    /// taken, or the request is a **restoration**, which is the closing half of
    /// a change this policy already accepted rather than a fresh request for a
    /// machine change — the state that justified the original check is by
    /// definition the state that has just ended. The bounds check applies to a
    /// restoration exactly as it does to the request that opened it.
    pub printer_state: Option<&'a PrinterState>,
    /// The bounds in force for this print.
    pub bounds: &'a EffectiveBounds,
    /// The operator's safety envelope.
    pub envelope: &'a SafetyEnvelope,
    /// When the agent last acted on this print, if it has.
    pub last_agent_action: Option<Timestamp>,
}

/// What an adjustment asks to change, and the value it asks for.
///
/// Absent for an action that changes no adjustable, which is what makes the
/// bounds arms of the decision reachable from the action alone.
#[must_use]
pub fn adjustment(action: &PrintAction) -> Option<(Adjustable, f64)> {
    match action {
        PrintAction::SetFeedrateFactor { factor, .. } => Some((Adjustable::Feedrate, *factor)),
        PrintAction::SetFlowrateFactor { factor, .. } => Some((Adjustable::Flowrate, *factor)),
        PrintAction::SetToolTargetC { tool, target_c, .. } => {
            Some((Adjustable::ToolTarget { tool: *tool }, *target_c))
        }
        PrintAction::SetBedTargetC { target_c, .. } => Some((Adjustable::BedTarget, *target_c)),
        PrintAction::SetFanPercent { percent, .. } => Some((Adjustable::Fan, *percent)),
        PrintAction::Pause { .. }
        | PrintAction::Resume { .. }
        | PrintAction::Cancel { .. }
        | PrintAction::StartPrint { .. }
        | PrintAction::AcknowledgeFailure { .. } => None,
    }
}

/// Whether one action is valid from the state the printer is in.
///
/// Acknowledging a failure is valid from every state, because it changes
/// nothing at the machine; everything else names the states it makes sense
/// from.
#[must_use]
pub fn valid_from(kind: ActionKind, state: &PrinterState) -> bool {
    match kind {
        ActionKind::Pause => matches!(state, PrinterState::Printing),
        ActionKind::Resume => matches!(state, PrinterState::Paused),
        ActionKind::Cancel => matches!(state, PrinterState::Printing | PrinterState::Paused),
        ActionKind::StartPrint => matches!(state, PrinterState::Operational),
        ActionKind::SetFeedrateFactor
        | ActionKind::SetFlowrateFactor
        | ActionKind::SetToolTargetC
        | ActionKind::SetBedTargetC
        | ActionKind::SetFanPercent => {
            matches!(state, PrinterState::Printing | PrinterState::Paused)
        }
        ActionKind::AcknowledgeFailure => true,
    }
}

/// Whether a print is one an action may still be taken against.
fn is_active(print: Option<&PrintRecord>) -> bool {
    print.is_some_and(|record| record.ended_at.is_none())
}

/// The decision policy takes on one request.
///
/// Every action from every actor passes through this, and the arms are ordered
/// so that each rejection is reachable: there is no point asking whether a
/// value is in bounds for an actor that may not ask for the action at all.
#[must_use]
pub fn decide(input: &DecisionInput<'_>) -> PolicyDecision {
    let kind = input.action.kind();
    let actor_class = input.actor.class();

    if !is_active(input.print) {
        return PolicyDecision::Rejected(RejectionReason::NoActivePrint);
    }

    let granted = input.envelope.actions.get(&actor_class);
    if !granted.is_some_and(|kinds| kinds.contains(&kind)) {
        return PolicyDecision::Rejected(RejectionReason::ActorMayNotRequest {
            actor_class,
            action: kind,
        });
    }

    if actor_class == ActorClass::Agent
        && let Some(last) = input.last_agent_action
    {
        let since_last_s = seconds_between(last, input.requested_at);
        if since_last_s < input.envelope.agent_min_interval_s {
            return PolicyDecision::Rejected(RejectionReason::MinIntervalNotElapsed {
                interval_s: input.envelope.agent_min_interval_s,
                since_last_s,
            });
        }
    }

    if let Some(state) = input.printer_state
        && !valid_from(kind, state)
    {
        return PolicyDecision::Rejected(RejectionReason::InvalidFromState {
            state: state.clone(),
        });
    }

    if let Some((adjustable, requested)) = adjustment(input.action) {
        let Some(allowed) = input.bounds.allowed.get(&adjustable) else {
            return PolicyDecision::Rejected(RejectionReason::UnsupportedAdjustable { adjustable });
        };
        if !allowed.contains(requested) {
            return PolicyDecision::Rejected(RejectionReason::OutOfBounds {
                adjustable,
                requested,
                allowed: *allowed,
            });
        }
    }

    PolicyDecision::Accepted
}
