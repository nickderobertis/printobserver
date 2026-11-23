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

/// The events a turn's facts carry, newest first.
fn facts_events(
    turn: &printobserver_supervisor_api::TurnRequest,
) -> Vec<printobserver_types::serde_json::Value> {
    turn.facts
        .as_ref()
        .and_then(|facts| facts.get("recent_events"))
        .and_then(printobserver_types::serde_json::Value::as_array)
        .expect("the facts carry their events")
        .clone()
}

/// A fan change asked for by `actor`.
fn fan(percent: f64, actor: Actor) -> PrintAction {
    PrintAction::SetFanPercent {
        percent,
        duration_s: None,
        reason: "the fan, by whoever asked".to_owned(),
        actor,
    }
}

/// Hold the facts of `turn` to carrying the person's fan request taken while
/// the turn before it ran, with that request's outcome, and to leaving out the
/// agent's own request of that turn, its outcome and the turn's session.
fn carries_the_persons_request_and_not_the_agents_own(
    world: &World,
    print_id: printobserver_types::PrintId,
    turn: &printobserver_supervisor_api::TurnRequest,
    agents_own: &str,
) {
    let facts = facts_events(turn);
    let ids: Vec<&str> = facts
        .iter()
        .filter_map(|event| event["id"].as_str())
        .collect();
    let requested = |actor_is_agent: bool, action: &str| -> Vec<(String, String)> {
        world
            .store
            .events_of(print_id)
            .iter()
            .filter(|record| record.kind().as_str() == "action_requested")
            .filter_map(|record| {
                let payload = &record.body.payload;
                (payload["actor"].get("agent").is_some() == actor_is_agent
                    && payload["action"]["action"] == action)
                    .then(|| {
                        (
                            record.id.to_string(),
                            payload["action_id"].as_str().unwrap_or_default().to_owned(),
                        )
                    })
            })
            .collect()
    };
    let persons = requested(false, "set_fan_percent");
    assert_eq!(persons.len(), 1, "{persons:?}");
    let (person, action) = &persons[0];
    assert!(
        ids.contains(&person.as_str()),
        "the person's request is missing: {facts:?}"
    );
    assert!(
        facts.iter().any(|event| event["kind"] != "action_requested"
            && event["payload"]["action_id"] == action.as_str()),
        "the person's request's outcome is missing: {facts:?}"
    );
    for (own, own_action) in requested(true, agents_own) {
        assert!(
            !ids.contains(&own.as_str()),
            "the agent's own request is back: {facts:?}"
        );
        assert!(
            !facts
                .iter()
                .any(|event| event["payload"]["action_id"] == own_action.as_str()),
            "the agent's own request's outcome is back: {facts:?}"
        );
    }
    assert!(
        !facts.iter().any(
            |event| event["kind"] == SupervisionSessionOpenedPayload::KIND
                || event["kind"] == AgentAssessmentPayload::KIND
        ),
        "the facts carry the turn's own bookkeeping: {facts:?}"
    );
}

/// What somebody else asked for while the last turn ran, and so recorded
/// before its answer, is in the next turn's facts; what that turn asked for
/// itself is not.
#[test]
fn a_turns_facts_carry_what_others_did_while_the_last_turn_ran() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    world.agent.acts_with(fan(40.0, agent_actor(print.id)));
    world.agent.meanwhile(Some(fan(80.0, Actor::Operator)));
    world
        .handle(failure_alert_with_image(7))
        .expect("the first event is handled");
    world.agent.acts_with_nothing();
    world.agent.meanwhile(None);
    world
        .handle(failure_alert_with_image(7))
        .expect("the second event is handled");

    let turns = world.agent.turns();
    assert_eq!(turns.len(), 2);
    carries_the_persons_request_and_not_the_agents_own(
        &world,
        print.id,
        &turns[1],
        "set_fan_percent",
    );
}

/// When the event the last turn answered cannot be found — that turn
/// acknowledged another by command — the next turn's facts still carry what
/// somebody else did while it ran, and still leave out what it did itself.
#[test]
fn a_turn_whose_answered_event_is_unknown_still_hands_on_what_others_did() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let first = world
        .handle(failure_alert_with_image(7))
        .expect("the first event is handled");
    world.agent.acts_with(PrintAction::AcknowledgeFailure {
        event_id: first.id,
        disposition: printobserver_core::AcknowledgementDisposition::Continue,
        reason: "the earlier one, again".to_owned(),
        actor: agent_actor(print.id),
    });
    world.agent.meanwhile(Some(fan(80.0, Actor::Operator)));
    let second = world
        .handle(failure_alert_with_image(7))
        .expect("the second event is handled");
    world.agent.acts_with_nothing();
    world.agent.meanwhile(None);
    world
        .handle(failure_alert_with_image(7))
        .expect("the third event is handled");

    // Nothing the agent asked for names the second event, so the facts cannot
    // place where its turn began.
    let second_id = second.id.to_string();
    assert!(
        !world.store.events_of(print.id).iter().any(|record| {
            record.kind().as_str() == "action_requested"
                && record.body.payload["action"]["event_id"] == second_id.as_str()
        }),
        "the second event was acknowledged after all"
    );
    let turns = world.agent.turns();
    assert_eq!(turns.len(), 3);
    carries_the_persons_request_and_not_the_agents_own(
        &world,
        print.id,
        &turns[2],
        "acknowledge_failure",
    );
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

/// One request a check image was asked for, as the recording maker kept it.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    print_id: Option<printobserver_types::PrintId>,
    captured_at: printobserver_types::Timestamp,
    made_for: printobserver_core::ImageUse,
    position: Option<u64>,
}

/// What makes a check's image in this journey: it keeps every request and
/// answers the stored frame, saying so.
#[derive(Default)]
struct RecordingCheckImages {
    asked: std::sync::Mutex<Vec<Asked>>,
}

impl printobserver_core::CheckImages for RecordingCheckImages {
    fn check_image(
        &self,
        _print_id: Option<printobserver_types::PrintId>,
        frame: &std::path::Path,
    ) -> Result<std::path::PathBuf, String> {
        Ok(frame.to_path_buf())
    }

    fn shown_image(
        &self,
        request: &printobserver_core::CheckImageRequest<'_>,
    ) -> Result<printobserver_core::ShownImage, String> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Asked {
                print_id: request.print_id,
                captured_at: request.captured_at,
                made_for: request.made_for,
                position: request.plan.as_ref().map(|plan| plan.file_position_bytes),
            });
        Ok(printobserver_core::ShownImage {
            path: request.frame.to_path_buf(),
            note: Some("the stored frame, as recorded".to_owned()),
            composite: false,
        })
    }
}

/// A check's image is asked for with the print, the moment its frame was
/// captured — what the alert's source said, else the observation's own record
/// of it — the check's use, and where the job stands in the print's own file:
/// none when the job runs another file, reports no position or a negative one.
/// What it answers reaches the turn as the image and its line.
#[test]
fn a_checks_image_is_asked_for_with_its_capture_time_and_its_place_in_the_plan() {
    use printobserver_core::ImageUse;

    let world = World::with_camera();
    let images = std::sync::Arc::new(RecordingCheckImages::default());
    assert!(world.core.install_check_images(images.clone()));
    world
        .printer
        .holds_gcode("benchy.gcode", ";LAYER_CHANGE\n;Z:0.2\nG1 X10 Y10 E1\n");
    let print = world.open_print(7);
    let source_said =
        printobserver_types::Timestamp::from_unix_seconds(1_790_000_000).expect("a moment");
    world.vision.serves_captured_at(Some(source_said));

    let mut positions = Vec::new();
    for (file, position) in [
        (Some("benchy.gcode"), Some(30)),
        (Some("another.gcode"), Some(30)),
        (Some("benchy.gcode"), None),
        (Some("benchy.gcode"), Some(-5)),
    ] {
        world
            .printer
            .reports_job(file, printobserver_printer_api::PrinterState::Printing);
        world.printer.reports_position(position, Some(4_194_304));
        world
            .handle(failure_alert_with_image(7))
            .expect("the event is handled");
        positions.push(position);
    }
    let asked = images
        .asked
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        asked.iter().map(|each| each.position).collect::<Vec<_>>(),
        vec![Some(30), None, None, None],
        "the plan position of the job's own file alone, for {positions:?}"
    );
    for each in &asked {
        assert_eq!(each.print_id, Some(print.id));
        assert_eq!(each.made_for, ImageUse::Check);
        assert_eq!(
            each.captured_at, source_said,
            "the alert's source's capture time"
        );
    }
    let turn = &world.agent.turns()[0];
    assert_eq!(
        turn.situation.image.as_deref(),
        Some("the stored frame, as recorded"),
        "the maker's line reaches the turn"
    );

    // An observation's frame says nothing of when it was captured, so its
    // own record of the moment it answered is the capture time.
    world.vision.serves_captured_at(None);
    world.printer.reports_job(
        Some("benchy.gcode"),
        printobserver_printer_api::PrinterState::Printing,
    );
    world.printer.reports_position(Some(30), Some(4_194_304));
    let observed = printobserver_core::block_on(world.core.observe_active_prints())
        .expect("the open prints are read");
    for turn in observed.turns {
        printobserver_core::block_on(world.core.run_supervision(turn)).expect("supervised");
    }
    let observed_at = world
        .agent
        .turns()
        .last()
        .and_then(|turn| turn.event.body.payload.get("frame_captured_at").cloned())
        .and_then(|value| printobserver_types::serde_json::from_value(value).ok())
        .expect("the observation records when its frame answered");
    let last = images
        .asked
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last()
        .cloned()
        .expect("the observation's check asked for an image");
    assert_eq!(last.captured_at, observed_at);
    assert_eq!(last.position, Some(30));
}
