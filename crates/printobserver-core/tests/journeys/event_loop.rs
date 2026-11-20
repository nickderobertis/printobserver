//! The whole of the handling one event gets, and the order it happens in.
//!
//! Ordering is part of it rather than the whole: the store's event append is
//! the loop's own first port call, so that everything after it has an event in
//! the history to be recorded against. Opening a session and continuing one are
//! distinguished rather than collapsed, because an implementation opening a
//! fresh session for every event satisfies a one-turn assertion exactly as a
//! continuing one does.

use printobserver_core::AgentAssessmentPayload;
use printobserver_core::{Actor, PolicyDecision, PrintAction};
use printobserver_printer_api::PrinterState;
use printobserver_supervisor_api::SupervisionSessionOpenedPayload;
use printobserver_types::{EventPayload as _, FileName};
use printobserver_vision_api::MalformedExternalEventPayload;

use crate::journal::{Call, Port};
use crate::world::{
    World, agent_actor, alert_kind, failure_alert, failure_alert_with_image, manifest,
    notification_alert, notification_kind, unattributed_alert,
};

/// A print carrying a manifest and one active intervention, ready for an event.
fn prepared() -> (World, printobserver_core::PrintRecord) {
    let world = World::new();
    let print = world.open_print(7);

    world.printer.reports_state(PrinterState::Operational);
    world
        .request(
            print.id,
            PrintAction::StartPrint {
                file_name: FileName::new("benchy.gcode").expect("a name"),
                manifest: manifest(&[(
                    printobserver_printer_api::Adjustable::Feedrate,
                    printobserver_types::Range::new(0.9, 1.6),
                )]),
                override_filament_check: false,
                override_material_check: false,
                override_sheet_check: false,
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
        .position(|call| *call == Call::AppendEvent(alert_kind()))
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
            .get(&printobserver_printer_api::Adjustable::Feedrate),
        Some(&printobserver_types::Range::new(0.9, 1.6)),
        "the bounds are the envelope narrowed by the manifest"
    );
    assert_eq!(context.interventions.len(), 1);
    assert_eq!(
        context.interventions[0].adjustable,
        printobserver_printer_api::Adjustable::Feedrate
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
        printobserver_core::ActorClass::Agent,
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
        .find_map(printobserver_types::EventRecord::payload_as::<AgentAssessmentPayload>)
        .expect("the assessment was persisted")
        .expect("the assessment is of its own type");
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
        .filter(|record| record.body.is::<SupervisionSessionOpenedPayload>())
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
            .filter(|record| record.body.is::<SupervisionSessionOpenedPayload>())
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
    // The body is carried through under the adapter's own kind, which no crate
    // of this workspace declares: the loop read the correlation beside it and
    // nothing in the payload.
    assert_eq!(event.kind(), &notification_kind());
    assert_eq!(
        event.body.payload,
        printobserver_types::serde_json::json!({ "what": "paused" })
    );
    let opened = world
        .store
        .print_now(print_id)
        .expect("the print was opened");
    assert_eq!(opened.provider_print_id, Some(9));
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
        vec![Call::AppendEvent(MalformedExternalEventPayload::kind())]
    );
    world.journal.assert_no_violations();
}

/// What makes a check's image in these journeys: it answers `made`, or fails,
/// and keeps every stored frame it was handed.
struct ScriptedCheckImages {
    made: Option<std::path::PathBuf>,
    handed: std::sync::Mutex<Vec<std::path::PathBuf>>,
}

impl printobserver_core::CheckImages for ScriptedCheckImages {
    fn check_image(
        &self,
        _print_id: Option<printobserver_types::PrintId>,
        frame: &std::path::Path,
    ) -> Result<std::path::PathBuf, String> {
        self.handed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(frame.to_path_buf());
        self.made
            .clone()
            .ok_or_else(|| "the frame could not be read".to_owned())
    }
}

/// A turn is shown the image what is installed makes of the stored frame, and
/// the stored frame itself when that fails: a check shown more than it needs
/// rather than nothing.
#[test]
fn a_check_is_shown_the_check_image_or_the_stored_frame_when_none_can_be_made() {
    let made = std::env::temp_dir().join("a-check-image.jpg");
    for answer in [Some(made.clone()), None] {
        let world = World::new();
        let images = std::sync::Arc::new(ScriptedCheckImages {
            made: answer.clone(),
            handed: std::sync::Mutex::new(Vec::new()),
        });
        assert!(world.core.install_check_images(images.clone()));
        world.open_print(7);
        world
            .handle(failure_alert_with_image(7))
            .expect("the event is handled");

        let handed = images
            .handed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(handed.len(), 1, "the stored frame was not handed over once");
        assert!(
            handed[0].exists(),
            "what was handed over is no stored frame"
        );
        let shown = world.agent.turns()[0].image_path.clone();
        let expected = answer.unwrap_or_else(|| handed[0].clone());
        assert_eq!(shown, Some(expected));
    }
}

/// An answer whose acknowledgement cannot be made still stands: the assessment
/// is kept, the acknowledgement's failure is recorded against the turn's event,
/// and the next event of the print gets its own turn.
#[test]
fn an_acknowledgement_the_answer_cannot_make_is_recorded_and_supervision_goes_on() {
    use printobserver_core::store::StoreError;

    let world = World::new();
    let print = world.open_print(7);
    let store = std::sync::Arc::clone(&world.store);
    // The agent's interval is read off the history as its request is decided,
    // so a history that cannot be read refuses the acknowledgement alone.
    world.agent.before_answering(move || {
        store.fails(
            crate::fakes::StoreMethod::History,
            StoreError::Io {
                detail: "the disk is unreadable".to_owned(),
            },
        );
    });
    let event = world
        .handle(failure_alert_with_image(7))
        .expect("the event is handled");
    world.store.heals();

    let held = world.store.events_of(print.id);
    assert!(
        held.iter()
            .any(|record| record.kind() == &AgentAssessmentPayload::kind()),
        "the turn's assessment was not kept"
    );
    let failed: Vec<String> = held
        .iter()
        .filter(|record| record.kind().as_str() == "port_failure")
        .filter_map(|record| {
            record.body.payload["detail"]
                .as_str()
                .map(ToOwned::to_owned)
        })
        .collect();
    assert!(
        failed
            .iter()
            .any(|detail| detail.starts_with("the answer's acknowledgement could not be made")),
        "{failed:?}"
    );
    assert!(
        held.iter().any(
            |record| record.body.payload["event_id"] == event.id.to_string()
                && record.kind().as_str() == "port_failure"
        ),
        "the failure is not recorded against the turn's event"
    );

    world.agent.before_answering(|| {});
    world
        .handle(failure_alert_with_image(7))
        .expect("the next event is handled");
    assert_eq!(world.agent.turns().len(), 2, "the next event got no turn");
}

/// The event identifiers a turn's facts carry, newest first.
fn facts_event_ids(turn: &printobserver_supervisor_api::TurnRequest) -> Vec<String> {
    turn.facts
        .as_ref()
        .and_then(|facts| facts.get("recent_events"))
        .and_then(printobserver_types::serde_json::Value::as_array)
        .expect("the facts carry their events")
        .iter()
        .map(|event| {
            event["id"]
                .as_str()
                .expect("an event's identifier")
                .to_owned()
        })
        .collect()
}

/// A turn's facts carry what happened since the agent last answered: before
/// any answer, the print's whole recent window; after one, every event newer
/// than it and none from before it, its own event among them.
#[test]
fn a_turns_facts_carry_the_events_since_the_agents_last_answer() {
    let world = World::new();
    let print = world.open_print(7);
    let first = world
        .handle(failure_alert_with_image(7))
        .expect("the first event is handled");
    let second = world
        .handle(failure_alert_with_image(7))
        .expect("the second event is handled");

    let turns = world.agent.turns();
    assert_eq!(turns.len(), 2);
    let before = facts_event_ids(&turns[0]);
    assert!(before.contains(&first.id.to_string()), "{before:?}");

    // The store keeps a print's events in the order they were appended, which
    // is the order "newer" means while the clock stands still.
    let held = world.store.events_of(print.id);
    let answered = held
        .iter()
        .position(|record| record.kind() == &AgentAssessmentPayload::kind())
        .expect("the first turn answered");
    let asked = held
        .iter()
        .position(|record| record.id == second.id)
        .expect("the second event is kept");
    let after = facts_event_ids(&turns[1]);
    assert!(after.contains(&second.id.to_string()), "{after:?}");
    assert!(
        !after.contains(&first.id.to_string()),
        "an event before the answer: {after:?}"
    );
    let newer: Vec<String> = held[answered + 1..=asked]
        .iter()
        .map(|record| record.id.to_string())
        .collect();
    for id in &newer {
        assert!(
            after.contains(id),
            "an event since the answer is missing: {id} from {after:?}"
        );
    }
    assert_eq!(after.len(), newer.len(), "{after:?} against {newer:?}");
}

/// A print that ends while its turn runs is not acknowledged from that turn's
/// answer: its record is closed, and nothing is left to carry on or stop. The
/// answer itself is kept.
#[test]
fn a_print_that_ended_during_its_turn_is_not_acknowledged_from_the_answer() {
    let world = World::new();
    let print = world.open_print(7);
    let store = std::sync::Arc::clone(&world.store);
    let ended = print.id;
    world.agent.before_answering(move || {
        printobserver_core::block_on(printobserver_core::store::PrintStore::end_print(
            store.as_ref(),
            ended,
            PrinterState::Operational,
            printobserver_types::Timestamp::now(),
            "the operator cancelled it".to_owned(),
        ))
        .expect("the print ends");
    });
    world
        .handle(failure_alert_with_image(7))
        .expect("the event is handled");

    let held = world.store.events_of(print.id);
    assert!(
        held.iter()
            .any(|record| record.kind() == &AgentAssessmentPayload::kind()),
        "the turn's answer was not kept"
    );
    let acknowledged: Vec<_> = held
        .iter()
        .filter(|record| record.body.payload["action"]["action"] == "acknowledge_failure")
        .collect();
    assert!(
        acknowledged.is_empty(),
        "an ended print was acknowledged: {acknowledged:?}"
    );
    assert!(
        !held
            .iter()
            .any(|record| record.kind().as_str() == "port_failure"),
        "skipping the acknowledgement was recorded as a failure"
    );
}

/// What makes the check images is installed once: a second installation is
/// refused, and the first goes on making every check's image.
#[test]
fn the_first_check_images_installed_stand() {
    let world = World::new();
    let made = std::env::temp_dir().join("the-first-check-image.jpg");
    let first = std::sync::Arc::new(ScriptedCheckImages {
        made: Some(made.clone()),
        handed: std::sync::Mutex::new(Vec::new()),
    });
    let second = std::sync::Arc::new(ScriptedCheckImages {
        made: Some(std::env::temp_dir().join("the-second-check-image.jpg")),
        handed: std::sync::Mutex::new(Vec::new()),
    });
    assert!(world.core.install_check_images(first.clone()));
    assert!(
        !world.core.install_check_images(second.clone()),
        "a second installation stood"
    );
    world.open_print(7);
    world
        .handle(failure_alert_with_image(7))
        .expect("the event is handled");
    assert_eq!(world.agent.turns()[0].image_path, Some(made));
    assert!(
        second
            .handed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty(),
        "the refused installation was handed a frame"
    );
}
