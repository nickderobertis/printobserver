//! Every rejection policy can make is reached, changes nothing, and is recorded.
//!
//! The walk is over every rejected variant the contracts declare rather than
//! over a list kept here, so a rejection added there cannot go unproven. What
//! is asserted **beside** the printer port depends on how the rejection was
//! reached, and the two paths are stated apart because an absence that holds on
//! one does not hold on the other: a rejection reached by a direct action
//! request touches no other port at all, while one reached while an event is
//! being handled necessarily follows the loop's own append, image record and
//! turn — none of which the rejection undoes.
//!
//! The absence asserted unconditionally is at the printer port's **action**
//! methods rather than at every port or every method. Persisting a rejection is
//! itself a store write and deciding may itself be a printer read: what is owed
//! is a printer that was not acted on.

use std::collections::BTreeMap;

use printobserver_types::{
    ActionKind, Actor, ActorClass, Adjustable, PolicyDecision, PrintAction, PrintId, PrinterState,
    Range, RejectionReason,
};

use crate::journal::{Call, Port};
use crate::source::{crate_dir, enum_variant_names, parse, read};
use crate::world::{
    ACTION_KINDS, World, agent_actor, failure_alert_with_image, permissive_envelope,
};

/// One rejection driven by a direct action request.
struct Refused {
    /// The decision that was recorded.
    decision: PolicyDecision,
    /// Every write the store received for that request.
    writes: Vec<Call>,
    /// Every action call the printer received.
    actions: Vec<Call>,
    /// Every call the vision port received.
    vision: Vec<Call>,
    /// Every call the supervising agent's harness received.
    supervisor: Vec<Call>,
}

/// Drive one rejected variant by a direct action request.
fn refuse(reason: &RejectionReason) -> Refused {
    let world = match reason {
        RejectionReason::ActorMayNotRequest { .. } => {
            let mut envelope = permissive_envelope();
            let mut actions = BTreeMap::new();
            actions.insert(ActorClass::Operator, ACTION_KINDS.to_vec());
            actions.insert(ActorClass::Agent, Vec::new());
            envelope.actions = actions;
            World::with_envelope(envelope)
        }
        RejectionReason::MinIntervalNotElapsed { .. } => {
            let mut envelope = permissive_envelope();
            envelope.agent_min_interval_s = 300;
            World::with_envelope(envelope)
        }
        _ => World::new(),
    };
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);

    let action = match reason {
        RejectionReason::OutOfBounds { .. } => PrintAction::SetFeedrateFactor {
            factor: 5.0,
            duration_s: Some(60),
            reason: "far too fast".to_owned(),
            actor: Actor::Operator,
        },
        RejectionReason::ActorMayNotRequest { .. } => PrintAction::Pause {
            reason: "the agent is not permitted this".to_owned(),
            actor: agent_actor(print.id),
        },
        RejectionReason::NoActivePrint => PrintAction::Pause {
            reason: "against a print nothing holds".to_owned(),
            actor: Actor::Operator,
        },
        RejectionReason::InvalidFromState { .. } => {
            world.printer.reports_state(PrinterState::Operational);
            PrintAction::Pause {
                reason: "there is nothing to pause".to_owned(),
                actor: Actor::Operator,
            }
        }
        RejectionReason::MinIntervalNotElapsed { .. } => {
            let accepted = world
                .request(
                    print.id,
                    PrintAction::Pause {
                        reason: "the agent's first action".to_owned(),
                        actor: agent_actor(print.id),
                    },
                )
                .expect("the first request is recorded");
            assert_eq!(accepted.record.decision, PolicyDecision::Accepted);
            world.printer.reports_state(PrinterState::Paused);
            PrintAction::Resume {
                reason: "the agent's second action, too soon".to_owned(),
                actor: agent_actor(print.id),
            }
        }
        RejectionReason::UnsupportedAdjustable { .. } => PrintAction::SetToolTargetC {
            tool: 1,
            target_c: 200.0,
            duration_s: Some(60),
            reason: "this printer has no second tool".to_owned(),
            actor: Actor::Operator,
        },
    };

    let against = match reason {
        RejectionReason::NoActivePrint => PrintId::new(),
        _ => print.id,
    };
    world.journal.clear();
    let outcome = world
        .request(against, action)
        .expect("the request is recorded");
    world.journal.assert_no_violations();
    Refused {
        decision: outcome.record.decision.clone(),
        writes: world.journal.store_writes(),
        actions: world.journal.printer_actions(),
        vision: world.journal.at(Port::Vision),
        supervisor: world.journal.at(Port::Supervisor),
    }
}

/// Every rejected variant the contracts declare, each built to be reached.
fn stated_rejections() -> Vec<RejectionReason> {
    vec![
        RejectionReason::OutOfBounds {
            adjustable: Adjustable::Feedrate,
            requested: 5.0,
            allowed: Range::new(0.5, 2.0),
        },
        RejectionReason::ActorMayNotRequest {
            actor_class: ActorClass::Agent,
            action: ActionKind::Pause,
        },
        RejectionReason::NoActivePrint,
        RejectionReason::InvalidFromState {
            state: PrinterState::Operational,
        },
        RejectionReason::MinIntervalNotElapsed {
            interval_s: 300,
            since_last_s: 0,
        },
        RejectionReason::UnsupportedAdjustable {
            adjustable: Adjustable::ToolTarget { tool: 1 },
        },
    ]
}

/// The walk produces every rejected variant the contracts declare.
#[test]
fn the_walk_produces_every_rejected_variant_the_contracts_declare() {
    let path = crate_dir("printobserver-types")
        .join("src")
        .join("policy.rs");
    let declared = enum_variant_names(&parse(&read(&path)), "RejectionReason");
    let produced: Vec<String> = stated_rejections()
        .iter()
        .map(|reason| {
            format!("{reason:?}")
                .split_whitespace()
                .next()
                .expect("a variant name")
                .trim_end_matches('{')
                .to_owned()
        })
        .collect();
    assert_eq!(
        produced, declared,
        "the walk has fallen behind the rejection vocabulary"
    );
}

/// Every rejection is reached, changes nothing, and is recorded distinguishably.
///
/// The printer's action methods receiving nothing is what establishes that a
/// request outside the bounds is **rejected rather than clamped**: a clamped
/// value would arrive here as a call.
#[test]
fn every_rejection_is_reached_and_changes_nothing_at_the_printer() {
    let mut decisions = Vec::new();
    for expected in stated_rejections() {
        let refused = refuse(&expected);
        assert_eq!(
            refused.decision,
            PolicyDecision::Rejected(expected.clone()),
            "the recorded decision is not the rejection this fixture was built for"
        );
        assert_eq!(
            refused.actions,
            Vec::new(),
            "{expected:?} reached the printer's action methods"
        );
        assert_eq!(
            refused.vision,
            Vec::new(),
            "{expected:?} reached the vision port"
        );
        assert_eq!(
            refused.supervisor,
            Vec::new(),
            "{expected:?} reached the supervising agent"
        );
        assert_eq!(
            refused.writes,
            vec![Call::RecordAction(PolicyDecision::Rejected(
                expected.clone()
            ))],
            "{expected:?} wrote something beside the record of its own rejection"
        );
        decisions.push(refused.decision);
    }
    let mut distinct = decisions.clone();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        decisions.len(),
        "two rejections are indistinguishable from one another"
    );
}

/// The out-of-bounds rejection carries the adjustable, the value and the range.
#[test]
fn the_out_of_bounds_rejection_reads_back_carrying_its_own_fields() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    world
        .request(
            print.id,
            PrintAction::SetFeedrateFactor {
                factor: 5.0,
                duration_s: Some(60),
                reason: "far too fast".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded");

    let records = world.store.action_records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].decision,
        PolicyDecision::Rejected(RejectionReason::OutOfBounds {
            adjustable: Adjustable::Feedrate,
            requested: 5.0,
            allowed: Range::new(0.5, 2.0),
        })
    );
    assert_eq!(records[0].outcome, None);
    assert_eq!(records[0].executed_at, None);
}

/// A rejection reached while an event is handled undoes none of the loop's work.
#[test]
fn a_rejection_while_an_event_is_handled_leaves_the_loops_own_writes_standing() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    world.agent.acts_with(PrintAction::SetFeedrateFactor {
        factor: 5.0,
        duration_s: Some(60),
        reason: "the agent asked for far too much".to_owned(),
        actor: agent_actor(print.id),
    });
    world.journal.clear();

    world
        .handle(failure_alert_with_image(7))
        .expect("the event is handled");

    let rejection = PolicyDecision::Rejected(RejectionReason::OutOfBounds {
        adjustable: Adjustable::Feedrate,
        requested: 5.0,
        allowed: Range::new(0.5, 2.0),
    });
    assert_eq!(
        world.journal.store_writes(),
        vec![
            Call::AppendEvent(printobserver_types::EventKind::ObicoFailureAlert),
            Call::PutImage,
            Call::RecordAction(rejection.clone()),
            Call::AppendEvent(printobserver_types::EventKind::SupervisionSessionOpened),
            Call::PutSession,
            Call::AppendEvent(printobserver_types::EventKind::AgentAssessment),
        ],
        "the loop's own writes for the event, plus the record of the rejection, and nothing else"
    );
    assert_eq!(
        world.journal.printer_actions(),
        Vec::new(),
        "an adjustment reached the printer"
    );
    assert_eq!(
        world.store.action_records()[0].decision,
        rejection,
        "the rejection does not read back out of the store"
    );
    world.journal.assert_no_violations();
}
