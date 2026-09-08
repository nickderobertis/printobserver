//! Every variant of the action vocabulary, driven through the real core.
//!
//! The walk is over the vocabulary the contracts declare rather than over a
//! list kept here, so a variant added there cannot go undriven. Each is driven
//! once as the agent and once as an operator, and the pair is asserted to
//! produce the same decision, the same call at the printer and the same
//! intervention — differing only in the recorded actor. An agent's action takes
//! the operator's path rather than a path of its own.

use std::collections::BTreeMap;

use printobserver_types::{
    AcknowledgementDisposition, ActionKind, Actor, ActorClass, Adjustable, EventId, FileName,
    PolicyDecision, PrintAction, PrinterState, RejectionReason, SafetyEnvelope,
};

use crate::journal::Call;
use crate::source::{crate_dir, enum_variant_names, parse, read};
use crate::world::{ACTION_KINDS, World, agent_actor, permissive_envelope};

/// How long every bounded adjustment in this walk stands for.
const DURATION_S: i64 = 60;

/// The request one kind is driven with, made by one actor.
fn action_for(kind: ActionKind, actor: Actor) -> PrintAction {
    let reason = "the walk drives every variant".to_owned();
    match kind {
        ActionKind::Pause => PrintAction::Pause { reason, actor },
        ActionKind::Resume => PrintAction::Resume { reason, actor },
        ActionKind::Cancel => PrintAction::Cancel { reason, actor },
        ActionKind::StartPrint => PrintAction::StartPrint {
            file_name: FileName::new("benchy.gcode").expect("a name"),
            manifest: crate::world::manifest(&[]),
            reason,
            actor,
        },
        ActionKind::SetFeedrateFactor => PrintAction::SetFeedrateFactor {
            factor: 1.5,
            duration_s: Some(DURATION_S),
            reason,
            actor,
        },
        ActionKind::SetFlowrateFactor => PrintAction::SetFlowrateFactor {
            factor: 1.1,
            duration_s: Some(DURATION_S),
            reason,
            actor,
        },
        ActionKind::SetToolTargetC => PrintAction::SetToolTargetC {
            tool: 0,
            target_c: 220.0,
            duration_s: Some(DURATION_S),
            reason,
            actor,
        },
        ActionKind::SetBedTargetC => PrintAction::SetBedTargetC {
            target_c: 65.0,
            duration_s: Some(DURATION_S),
            reason,
            actor,
        },
        ActionKind::SetFanPercent => PrintAction::SetFanPercent {
            percent: 80.0,
            duration_s: Some(DURATION_S),
            reason,
            actor,
        },
        ActionKind::AcknowledgeFailure => PrintAction::AcknowledgeFailure {
            event_id: EventId::new(),
            disposition: AcknowledgementDisposition::Stop,
            reason,
            actor,
        },
    }
}

/// The state each kind is valid from, so that the walk reaches the printer.
const fn state_for(kind: ActionKind) -> PrinterState {
    match kind {
        ActionKind::Resume => PrinterState::Paused,
        ActionKind::StartPrint => PrinterState::Operational,
        _ => PrinterState::Printing,
    }
}

/// The call each kind is expected to make at the printer port.
///
/// Acknowledging a failure with the `stop` disposition is the one kind whose
/// call is another kind's: stopping a print is cancelling it, and one decision
/// still stands in front of the one call.
fn expected_call(kind: ActionKind) -> Call {
    match kind {
        ActionKind::Pause => Call::Pause,
        ActionKind::Resume => Call::Resume,
        ActionKind::Cancel | ActionKind::AcknowledgeFailure => Call::Cancel,
        ActionKind::StartPrint => Call::Start("benchy.gcode".to_owned()),
        ActionKind::SetFeedrateFactor => Call::SetFeedrateFactor(1.5),
        ActionKind::SetFlowrateFactor => Call::SetFlowrateFactor(1.1),
        ActionKind::SetToolTargetC => Call::SetToolTargetC(0, 220.0),
        ActionKind::SetBedTargetC => Call::SetBedTargetC(65.0),
        ActionKind::SetFanPercent => Call::SetFanPercent(80.0),
    }
}

/// What the adjustable and value one kind changes, when it changes one.
const fn adjusts(kind: ActionKind) -> Option<(Adjustable, f64)> {
    match kind {
        ActionKind::SetFeedrateFactor => Some((Adjustable::Feedrate, 1.5)),
        ActionKind::SetFlowrateFactor => Some((Adjustable::Flowrate, 1.1)),
        ActionKind::SetToolTargetC => Some((Adjustable::ToolTarget { tool: 0 }, 220.0)),
        ActionKind::SetBedTargetC => Some((Adjustable::BedTarget, 65.0)),
        ActionKind::SetFanPercent => Some((Adjustable::Fan, 80.0)),
        _ => None,
    }
}

/// What driving one kind as one actor produced.
struct Driven {
    /// The decision policy took.
    decision: PolicyDecision,
    /// Every action call the printer received.
    actions: Vec<Call>,
    /// The bounded intervention it opened, as the fields that identify it.
    intervention: Option<(Adjustable, Option<f64>, f64)>,
    /// The actor the record carries.
    actor: Actor,
}

/// Drive one kind once, as one actor, under one envelope.
fn drive(kind: ActionKind, actor: Actor, envelope: SafetyEnvelope) -> Driven {
    let world = World::with_envelope(envelope);
    let print = world.open_print(7);
    world.printer.reports_state(state_for(kind));
    world.journal.clear();
    let outcome = world
        .request(print.id, action_for(kind, actor))
        .expect("the request is recorded");
    world.journal.assert_no_violations();
    Driven {
        decision: outcome.record.decision.clone(),
        actions: world.journal.printer_actions(),
        intervention: outcome.intervention.as_ref().map(|held| {
            (held.adjustable, held.prior_value, held.applied_value)
        }),
        actor: outcome.record.request.actor.clone(),
    }
}

/// The walk drives exactly the vocabulary the contracts declare.
#[test]
fn the_walk_covers_every_variant_the_vocabulary_declares() {
    let path = crate_dir("printobserver-types").join("src").join("action.rs");
    let declared = enum_variant_names(&parse(&read(&path)), "PrintAction");
    let driven: Vec<String> = ACTION_KINDS
        .iter()
        .map(|kind| format!("{kind:?}"))
        .collect();
    assert_eq!(
        driven, declared,
        "the walk has fallen behind the action vocabulary"
    );
}

/// Every variant reaches the printer with a bounded value, behind a decision.
#[test]
fn every_variant_reaches_the_printer_only_behind_a_recorded_decision() {
    for kind in ACTION_KINDS {
        let world = World::new();
        let print = world.open_print(7);
        world.printer.reports_state(state_for(kind));
        world.journal.clear();
        let outcome = world
            .request(print.id, action_for(kind, Actor::Operator))
            .expect("the request is recorded");

        assert_eq!(
            outcome.record.decision,
            PolicyDecision::Accepted,
            "{kind:?} was not accepted"
        );
        assert_eq!(
            world.journal.printer_actions(),
            vec![expected_call(kind)],
            "{kind:?} did not reach the printer as expected"
        );
        let decided = world
            .journal
            .position(&Call::RecordAction(PolicyDecision::Accepted))
            .unwrap_or_else(|| panic!("{kind:?} recorded no decision"));
        let acted = world
            .journal
            .position(&expected_call(kind))
            .unwrap_or_else(|| panic!("{kind:?} reached no printer call"));
        assert!(decided < acted, "{kind:?} acted before its decision was recorded");

        if let Some((adjustable, value)) = adjusts(kind) {
            let allowed = world
                .context(print.id)
                .expect("the context reads back")
                .bounds
                .allowed
                .get(&adjustable)
                .copied()
                .unwrap_or_else(|| panic!("{adjustable} has no bound"));
            assert!(
                allowed.contains(value),
                "{kind:?} sent {value}, outside {allowed:?}"
            );
        }
        world.journal.assert_no_violations();
    }
}

/// An agent's action takes the operator's own path, not a path of its own.
#[test]
fn an_agents_action_and_an_operators_differ_only_in_the_recorded_actor() {
    for kind in ACTION_KINDS {
        let print_id = printobserver_types::PrintId::new();
        let as_agent = drive(kind, agent_actor(print_id), permissive_envelope());
        let as_operator = drive(kind, Actor::Operator, permissive_envelope());

        assert_eq!(as_agent.decision, as_operator.decision, "{kind:?}");
        assert_eq!(as_agent.actions, as_operator.actions, "{kind:?}");
        assert_eq!(as_agent.intervention, as_operator.intervention, "{kind:?}");
        assert_eq!(as_agent.actor.class(), ActorClass::Agent, "{kind:?}");
        assert_eq!(as_operator.actor, Actor::Operator, "{kind:?}");
        assert_ne!(as_agent.actor, as_operator.actor, "{kind:?}");
    }
}

/// Where the envelope grants an action to one class and not the other, the
/// difference is the recorded rejection naming that class.
#[test]
fn an_action_granted_to_one_class_and_not_the_other_is_rejected_by_class() {
    let mut envelope = permissive_envelope();
    let mut actions = BTreeMap::new();
    actions.insert(ActorClass::Operator, ACTION_KINDS.to_vec());
    actions.insert(
        ActorClass::Agent,
        ACTION_KINDS
            .iter()
            .copied()
            .filter(|kind| *kind != ActionKind::Cancel)
            .collect(),
    );
    envelope.actions = actions;

    let print_id = printobserver_types::PrintId::new();
    let as_operator = drive(ActionKind::Cancel, Actor::Operator, envelope.clone());
    let as_agent = drive(ActionKind::Cancel, agent_actor(print_id), envelope);

    assert_eq!(as_operator.decision, PolicyDecision::Accepted);
    assert_eq!(as_operator.actions, vec![Call::Cancel]);
    assert_eq!(
        as_agent.decision,
        PolicyDecision::Rejected(RejectionReason::ActorMayNotRequest {
            actor_class: ActorClass::Agent,
            action: ActionKind::Cancel,
        })
    );
    assert_eq!(as_agent.actions, Vec::new());
}
