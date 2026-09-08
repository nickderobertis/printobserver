//! A port that fails does not lose the event, and does not wedge the loop.
//!
//! The printer is walked over **every method its trait declares** rather than
//! failed at one of them, because that port fails at materially different
//! places and a read that fails says nothing about an action that fails. The
//! restoring call an expiry makes is walked beside them: it is an action-method
//! call this crate issues outside event handling, which no failure induced
//! during event handling reaches.
//!
//! What "recorded" means differs by site, and is asserted rather than left to
//! the word. A failure at a site reached **while handling an event** is
//! recorded against that event. A failure of an **action** the request minted a
//! record for is recorded on that record. A failure of the **restoring call**
//! is recorded on the intervention, there being no event at expiry to record it
//! against — and it is recorded so that a reader can tell what value the
//! printer was left holding, which is the whole point of recording it.

use printobserver_printer_api::PrinterError;
use printobserver_store_api::StoreError;
use printobserver_supervisor_api::SupervisorError;
use printobserver_types::{
    ActionKind, Actor, EventPayload, ExecutionOutcome, InterventionOutcome, PortFailurePayload,
    PortFailureSite, PrintAction, PrintId, PrinterState,
};
use printobserver_vision_api::VisionError;

use crate::action_vocabulary::{action_for, state_for};
use crate::fakes::{PrinterMethod, StoreMethod};
use crate::world::{World, assert_same, failure_alert, failure_alert_with_image};

/// The action one printer method is reached by, for the methods that are actions.
const fn action_reaching(method: PrinterMethod) -> Option<ActionKind> {
    match method {
        PrinterMethod::Snapshot | PrinterMethod::Job => None,
        PrinterMethod::Start => Some(ActionKind::StartPrint),
        PrinterMethod::Pause => Some(ActionKind::Pause),
        PrinterMethod::Resume => Some(ActionKind::Resume),
        PrinterMethod::Cancel => Some(ActionKind::Cancel),
        PrinterMethod::SetFeedrateFactor => Some(ActionKind::SetFeedrateFactor),
        PrinterMethod::SetFlowrateFactor => Some(ActionKind::SetFlowrateFactor),
        PrinterMethod::SetToolTargetC => Some(ActionKind::SetToolTargetC),
        PrinterMethod::SetBedTargetC => Some(ActionKind::SetBedTargetC),
        PrinterMethod::SetFanPercent => Some(ActionKind::SetFanPercent),
    }
}

/// The failure induced at every site of this walk.
fn induced(method: PrinterMethod) -> PrinterError {
    PrinterError::Unreachable {
        detail: format!("{} could not be reached", method.name()),
    }
}

/// Every port failure recorded against one print, in order.
fn recorded_failures(world: &World, print_id: PrintId) -> Vec<PortFailurePayload> {
    world
        .store
        .events_of(print_id)
        .into_iter()
        .filter_map(|record| match record.payload {
            EventPayload::PortFailure(payload) => Some(payload),
            _ => None,
        })
        .collect()
}

/// How many assessments one print's history holds.
fn assessments(world: &World, print_id: PrintId) -> usize {
    world
        .store
        .events_of(print_id)
        .into_iter()
        .filter(|record| matches!(record.payload, EventPayload::AgentAssessment(_)))
        .count()
}

/// Drive a further event through to completion, proving the loop is not wedged.
fn a_further_event_is_handled_to_completion(world: &World, print_id: PrintId) {
    world.printer.heals();
    world.store.heals();
    world.vision.heals();
    world.agent.heals();
    world.agent.acts_with_nothing();
    world.printer.reports_state(PrinterState::Printing);
    let before = assessments(world, print_id);

    let event = world
        .handle(failure_alert(7))
        .expect("the next event is handled");

    assert_eq!(event.print_id, Some(print_id));
    assert_eq!(
        assessments(world, print_id),
        before + 1,
        "the loop did not carry the next event through to its assessment"
    );
}

/// Every method the printer trait declares is induced to fail where this crate
/// calls it, and the failure is recorded and survived.
#[test]
fn every_printer_method_fails_where_this_crate_calls_it_and_is_recorded() {
    let mut walked = 0_usize;
    for method in PrinterMethod::ALL {
        let error = induced(method);
        let world = World::new();
        let print = world.open_print(7);
        world.printer.reports_state(PrinterState::Printing);

        match action_reaching(method) {
            None => {
                // A read this crate makes while handling an event: recorded
                // against that event, and the handling carries on without it.
                world.printer.fails(method, error.clone());
                let event = world
                    .handle(failure_alert(7))
                    .expect("the event is handled");
                let site = if method == PrinterMethod::Snapshot {
                    PortFailureSite::PrinterSnapshot
                } else {
                    PortFailureSite::PrinterJob
                };
                let failures = recorded_failures(&world, print.id);
                assert_eq!(
                    failures,
                    vec![PortFailurePayload {
                        event_id: event.id,
                        site,
                        detail: error.to_string(),
                    }],
                    "{} did not record its failure against the event",
                    method.name()
                );
            }
            Some(kind) => {
                // An action: recorded on the record the request minted.
                world.printer.reports_state(state_for(kind));
                world.printer.fails(method, error.clone());
                let outcome = world
                    .request(print.id, action_for(kind, Actor::Operator))
                    .expect("the request is recorded");
                assert_eq!(
                    outcome.record.outcome,
                    Some(ExecutionOutcome::Failed {
                        reason: error.to_string()
                    }),
                    "{} did not record its failure on the action record",
                    method.name()
                );
                assert!(outcome.record.executed_at.is_some());
                assert_eq!(outcome.intervention, None);
            }
        }

        a_further_event_is_handled_to_completion(&world, print.id);
        world.journal.assert_no_violations();
        walked += 1;
    }
    assert_eq!(
        walked,
        PrinterMethod::ALL.len(),
        "the walk has fallen behind the printer port"
    );
}

/// An action failing while an event is handled leaves the loop able to go on.
#[test]
fn an_action_failing_during_a_turn_is_recorded_and_survived() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let error = induced(PrinterMethod::Pause);
    world.printer.fails(PrinterMethod::Pause, error.clone());
    world.agent.acts_with(PrintAction::Pause {
        reason: "the agent saw spaghetti".to_owned(),
        actor: crate::world::agent_actor(print.id),
    });

    world
        .handle(failure_alert(7))
        .expect("the event is handled");

    let record = world
        .store
        .action_records()
        .into_iter()
        .find(|record| record.request.action.kind() == ActionKind::Pause)
        .expect("the agent's action reads back");
    assert_eq!(
        record.outcome,
        Some(ExecutionOutcome::Failed {
            reason: error.to_string()
        })
    );
    assert_eq!(assessments(&world, print.id), 1, "the turn still completed");

    a_further_event_is_handled_to_completion(&world, print.id);
    world.journal.assert_no_violations();
}

/// The restoring call failing at expiry is recorded on the intervention.
///
/// The record has to leave a reader able to tell what value the printer was
/// left holding: a restoring call that failed leaves the machine carrying the
/// intervention's applied value after the window that justified it has closed.
#[test]
fn the_restoring_call_failing_is_recorded_on_the_intervention_and_settled() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let intervention = world
        .request(
            print.id,
            PrintAction::SetFanPercent {
                percent: 80.0,
                duration_s: Some(60),
                reason: "cooling the overhang".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded")
        .intervention
        .expect("it opened an intervention");

    let error = induced(PrinterMethod::SetFanPercent);
    world
        .printer
        .fails(PrinterMethod::SetFanPercent, error.clone());
    world.clock.advance(61);
    world.wait_until("the intervention settles", || {
        world
            .store
            .intervention(intervention.id)
            .is_some_and(|held| held.outcome != InterventionOutcome::StillActive)
    });

    let settled = world
        .store
        .intervention(intervention.id)
        .expect("the intervention reads back");
    assert_eq!(
        settled.outcome,
        InterventionOutcome::RestoreFailed {
            reason: error.to_string()
        }
    );
    assert_same(settled.applied_value, 80.0);
    assert_eq!(
        settled.prior_value,
        Some(40.0),
        "the record does not say what it should have been returned to"
    );
    assert_ne!(settled.outcome, InterventionOutcome::StillActive);

    a_further_event_is_handled_to_completion(&world, print.id);
    world.journal.assert_no_violations();
}

/// The image write failing is recorded against the event and survived.
#[test]
fn the_image_write_failing_is_recorded_against_the_event() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let error = StoreError::Io {
        detail: "the state directory is read only".to_owned(),
    };
    world.store.fails(StoreMethod::PutImage, error.clone());

    let event = world
        .handle(failure_alert_with_image(7))
        .expect("the event is handled");

    assert_eq!(
        recorded_failures(&world, print.id),
        vec![PortFailurePayload {
            event_id: event.id,
            site: PortFailureSite::ImageWrite,
            detail: error.to_string(),
        }]
    );
    assert_eq!(assessments(&world, print.id), 1, "the turn still ran");
    a_further_event_is_handled_to_completion(&world, print.id);
    world.journal.assert_no_violations();
}

/// Retrieving the image failing is recorded at that same site and survived.
#[test]
fn retrieving_the_image_failing_is_recorded_against_the_event() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let error = VisionError::TimedOut;
    world.vision.fails(error.clone());

    let event = world
        .handle(failure_alert_with_image(7))
        .expect("the event is handled");

    assert_eq!(
        recorded_failures(&world, print.id),
        vec![PortFailurePayload {
            event_id: event.id,
            site: PortFailureSite::ImageWrite,
            detail: error.to_string(),
        }]
    );
    a_further_event_is_handled_to_completion(&world, print.id);
    world.journal.assert_no_violations();
}

/// The supervision turn failing is recorded against the event and survived.
#[test]
fn the_supervision_turn_failing_is_recorded_against_the_event() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let error = SupervisorError::TimedOut;
    world.agent.fails(error.clone());

    let event = world
        .handle(failure_alert(7))
        .expect("the event is handled");

    assert_eq!(
        recorded_failures(&world, print.id),
        vec![PortFailurePayload {
            event_id: event.id,
            site: PortFailureSite::SupervisionTurn,
            detail: error.to_string(),
        }]
    );
    assert_eq!(
        assessments(&world, print.id),
        0,
        "no assessment was answered"
    );
    a_further_event_is_handled_to_completion(&world, print.id);
    world.journal.assert_no_violations();
}

/// The store's own initial append failing is the one failure that is answered.
///
/// The store is where the record lives, so when the append itself fails the
/// event is recorded nowhere. The loop then calls no other port for that event,
/// answers the store's own error to its caller rather than swallowing it, and
/// goes on. Losing the alert that way is an availability failure; acting on the
/// printer with no record behind it would be a safety one.
#[test]
fn the_initial_append_failing_calls_no_other_port_and_is_answered() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let error = StoreError::Database {
        detail: "the database is locked".to_owned(),
    };
    world.store.fails(StoreMethod::AppendEvent, error.clone());
    world.journal.clear();

    let answered = world.handle(failure_alert_with_image(7));

    assert_eq!(
        answered,
        Err(printobserver_core::CoreError::Store(error)),
        "the store's own error was not answered to the caller"
    );
    assert_eq!(
        world.journal.at(crate::journal::Port::Vision),
        Vec::new(),
        "the vision port was called for an event nothing recorded"
    );
    assert_eq!(
        world.journal.at(crate::journal::Port::Printer),
        Vec::new(),
        "the printer was called for an event nothing recorded"
    );
    assert_eq!(
        world.journal.at(crate::journal::Port::Supervisor),
        Vec::new(),
        "a supervision turn ran for an event nothing recorded"
    );
    let writes = world.journal.store_writes();
    assert_eq!(
        writes,
        vec![crate::journal::Call::AppendEvent(
            printobserver_types::EventKind::ObicoFailureAlert
        )],
        "the store was written to after the failed append"
    );

    a_further_event_is_handled_to_completion(&world, print.id);
    world.journal.assert_no_violations();
}
