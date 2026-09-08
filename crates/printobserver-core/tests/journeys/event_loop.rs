//! The whole of the handling one event gets, and the order it happens in.
//!
//! Ordering is part of it rather than the whole: the store's event append is
//! the loop's own first port call, so that everything after it has an event in
//! the history to be recorded against. Opening a session and continuing one are
//! distinguished rather than collapsed, because an implementation opening a
//! fresh session for every event satisfies a one-turn assertion exactly as a
//! continuing one does.

use printobserver_types::{
    Actor, EventKind, EventPayload, FileName, PolicyDecision, PrintAction, PrinterState,
};

use crate::journal::{Call, Port};
use crate::world::{
    World, agent_actor, failure_alert, failure_alert_with_image, manifest, notification_alert,
    unattributed_alert,
};

/// A print carrying a manifest and one active intervention, ready for an event.
fn prepared() -> (World, printobserver_types::PrintRecord) {
    let world = World::new();
    let print = world.open_print(7);

    world.printer.reports_state(PrinterState::Operational);
    world
        .request(
            print.id,
            PrintAction::StartPrint {
                file_name: FileName::new("benchy.gcode").expect("a name"),
                manifest: manifest(&[(
                    printobserver_types::Adjustable::Feedrate,
                    printobserver_types::Range::new(0.9, 1.6),
                )]),
                reason: "the operator started it".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the print starts");

    world.printer.reports_state(PrinterState::Printing);
    world
        .request(
            print.id,
            PrintAction::SetFeedrateFactor {
                factor: 1.5,
                duration_s: Some(3600),
                reason: "slowing for the bridge".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the adjustment is accepted");

    world.journal.clear();
    (world, print)
}

/// The store's event append precedes every printer call, image write and turn.
fn assert_the_append_came_first(world: &World) {
    let calls = world.journal.calls();
    let appended = calls
        .iter()
        .position(|call| *call == Call::AppendEvent(EventKind::ObicoFailureAlert))
        .expect("the event was appended");
    for (index, call) in calls.iter().enumerate() {
        // A decision is in this set too, and deliberately: the append is the
        // loop's own first operation and no decision could exist before it, so
        // an implementation deciding first has collapsed the two properties
        // this crate keeps apart.
        let owed_the_append = matches!(call.port(), Port::Printer)
            || matches!(
                call,
                Call::PutImage | Call::RunTurn(_) | Call::RecordAction(_)
            );
        assert!(
            !owed_the_append || index > appended,
            "{call:?} happened before the event was appended"
        );
    }
}

/// The context the turn was handed carries every part the loop collects.
fn assert_the_context_carries_everything(
    world: &World,
    print_id: printobserver_types::PrintId,
    event_id: printobserver_types::EventId,
    image: &printobserver_types::ImageRef,
) {
    let contexts = world.agent.contexts();
    assert_eq!(contexts.len(), 1);
    let context = contexts[0].as_ref().expect("the turn read its context");
    assert_eq!(context.print.id, print_id);
    assert_eq!(
        context
            .printer
            .as_ref()
            .map(|snapshot| &snapshot.connection),
        Some(&PrinterState::Printing)
    );
    assert_eq!(context.job.as_ref(), Some(&crate::fakes::job_snapshot()));
    assert_eq!(
        context.manifest.as_ref().map(|held| held.file_name.clone()),
        Some("benchy.gcode".to_owned())
    );
    assert_eq!(
        context
            .bounds
            .allowed
            .get(&printobserver_types::Adjustable::Feedrate),
        Some(&printobserver_types::Range::new(0.9, 1.6)),
        "the bounds are the envelope narrowed by the manifest"
    );
    assert_eq!(context.interventions.len(), 1);
    assert_eq!(
        context.interventions[0].adjustable,
        printobserver_types::Adjustable::Feedrate
    );
    assert!(
        context
            .recent_events
            .iter()
            .any(|record| record.id == event_id),
        "the context carries the event that prompted the turn"
    );
    assert_eq!(context.latest_image.as_ref(), Some(image));
}

/// The agent's action reached the printer only behind a recorded decision.
fn assert_the_agents_action_took_the_ordinary_path(world: &World) {
    let decided = world
        .journal
        .position(&Call::RecordAction(PolicyDecision::Accepted))
        .expect("the agent's action was decided");
    let acted = world
        .journal
        .position(&Call::Pause)
        .expect("it reached the printer");
    assert!(decided < acted);

    let taken = world.agent.acted();
    assert_eq!(taken.len(), 1);
    let outcome = taken[0].as_ref().expect("the agent's action was answered");
    assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
    assert_eq!(
        outcome.record.request.actor.class(),
        printobserver_types::ActorClass::Agent,
        "the agent's action was not recorded with the agent as its actor"
    );
}

/// The loop performs the whole of the handling, in the order it owes.
#[test]
fn the_loop_performs_the_whole_of_the_handling_for_one_event() {
    let (world, print) = prepared();
    world.agent.acts_with(PrintAction::Pause {
        reason: "the agent saw spaghetti".to_owned(),
        actor: agent_actor(print.id),
    });

    let event = world
        .handle(failure_alert_with_image(7))
        .expect("the event is handled");

    assert_the_append_came_first(&world);

    let held = world.store.events_of(print.id);
    let alert = held
        .iter()
        .find(|record| record.id == event.id)
        .expect("the event reads back out of the store");
    let image = alert.image.clone().expect("the image was written");

    assert_the_context_carries_everything(&world, print.id, event.id, &image);

    let turns = world.agent.turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].print_id, print.id);
    assert_eq!(turns[0].event.id, event.id);
    assert_eq!(turns[0].event.image.as_ref(), Some(&image));
    assert!(
        turns[0]
            .image_path
            .as_ref()
            .is_some_and(|path| path.exists())
    );
    assert!(
        turns[0].context_command.contains(&print.id.to_string()),
        "the context command names the print it is about"
    );

    assert_the_agents_action_took_the_ordinary_path(&world);

    let assessment = held
        .iter()
        .find_map(|record| match &record.payload {
            EventPayload::AgentAssessment(payload) => Some(payload.clone()),
            _ => None,
        })
        .expect("the assessment was persisted");
    assert_eq!(assessment.assessment.summary, "the print is running");
    world.journal.assert_no_violations();
}

/// A second event of the same print continues the one session it opened.
#[test]
fn a_second_event_of_one_print_continues_its_session() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);

    let first = world.handle(failure_alert(7)).expect("the first event");
    let second = world.handle(failure_alert(7)).expect("the second event");
    assert_ne!(first.id, second.id);

    let print_id = first.print_id.expect("the first event names its print");
    assert_eq!(second.print_id, Some(print_id));

    let turns = world.agent.turns();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].print_id, print_id);
    assert_eq!(turns[1].print_id, print_id);

    let calls = world.journal.calls();
    let first_turn = calls
        .iter()
        .position(|call| *call == Call::RunTurn(print_id))
        .expect("the first turn");
    let second_turn = calls
        .iter()
        .rposition(|call| *call == Call::RunTurn(print_id))
        .expect("the second turn");
    assert!(first_turn < second_turn);
    assert!(
        !calls[first_turn..second_turn]
            .iter()
            .any(|call| matches!(call, Call::CloseSession(_, _))),
        "the session was closed between the two turns"
    );

    // The phase reads back off the print: opened once, and not again.
    let opened = world
        .store
        .events_of(print_id)
        .into_iter()
        .filter(|record| record.kind() == EventKind::SupervisionSessionOpened)
        .count();
    assert_eq!(opened, 1, "a fresh session was opened for the second event");
    world.journal.assert_no_violations();
}

/// An event of a different print drives a turn carrying that print's identifier.
#[test]
fn an_event_of_another_print_drives_a_turn_under_that_prints_identifier() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);

    let first = world
        .handle(failure_alert(7))
        .expect("the first print's event");
    let other = world
        .handle(failure_alert(8))
        .expect("the other print's event");

    let first_print = first.print_id.expect("a print");
    let other_print = other.print_id.expect("a print");
    assert_ne!(first_print, other_print);

    let turns = world.agent.turns();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].print_id, first_print);
    assert_eq!(turns[1].print_id, other_print);

    for print_id in [first_print, other_print] {
        let opened = world
            .store
            .events_of(print_id)
            .into_iter()
            .filter(|record| record.kind() == EventKind::SupervisionSessionOpened)
            .count();
        assert_eq!(opened, 1);
    }
    world.journal.assert_no_violations();
}

/// A printer notification opens the print it names and drives a turn on it.
#[test]
fn a_printer_notification_opens_the_print_it_names() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);

    let event = world
        .handle(notification_alert(9))
        .expect("the notification is handled");

    let print_id = event.print_id.expect("the notification names its print");
    let opened = world
        .store
        .print_now(print_id)
        .expect("the print was opened");
    assert_eq!(opened.obico_print_id, Some(9));
    assert_eq!(opened.file_name, Some("benchy.gcode".to_owned()));
    assert_eq!(world.agent.turns().len(), 1);
    assert_eq!(world.agent.turns()[0].print_id, print_id);
    world.journal.assert_no_violations();
}

/// An alert naming no print is held in the history, and reaches no other port.
///
/// An externally sourced body may name no print this system knows. It is
/// written down rather than dropped, and nothing is supervised on its account.
#[test]
fn an_alert_naming_no_print_is_recorded_and_reaches_no_other_port() {
    let world = World::new();
    world.journal.clear();

    let event = world
        .handle(unattributed_alert())
        .expect("the alert is handled");

    assert_eq!(event.print_id, None);
    assert_eq!(world.journal.at(Port::Printer), Vec::new());
    assert_eq!(world.journal.at(Port::Supervisor), Vec::new());
    assert_eq!(world.journal.at(Port::Vision), Vec::new());
    assert_eq!(
        world.journal.store_writes(),
        vec![Call::AppendEvent(EventKind::MalformedExternalEvent)]
    );
    world.journal.assert_no_violations();
}
