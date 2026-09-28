//! A print the detector paused: the agent's adjustment resumes it, nothing
//! else does, and an event arriving during a turn reaches that turn.

use std::time::Duration;

use printobserver_core::{
    AcknowledgementDisposition, Actor, PolicyDecision, PrintAction, RejectionReason,
    SafetyEnvelope,
};
use printobserver_printer_api::PrinterState;
use printobserver_types::{EventId, PrintId};

use crate::journal::Call;
use crate::world::{
    DETECTOR_PRINTER_ID, World, detector_paused_alert, failure_alert, permissive_envelope,
};

/// The agent, under a session name no journey reads.
fn agent() -> Actor {
    Actor::Agent {
        session_name: "print-under-test".to_owned(),
    }
}

/// The part fan up, as the agent asks for it.
fn fan_up() -> PrintAction {
    PrintAction::SetFanPercent {
        percent: 100.0,
        duration_s: None,
        reason: "the overhang is curling: more cooling".to_owned(),
        actor: agent(),
    }
}

/// Give the expiry driver long enough to have swept at least once.
fn let_the_driver_sweep() {
    std::thread::sleep(Duration::from_millis(100));
}

/// Handle a detector-paused alert for a paused printer, and answer the print.
fn paused_by_the_detector(world: &World) -> PrintId {
    world.printer.reports_state(PrinterState::Paused);
    world
        .handle(detector_paused_alert(7))
        .expect("the alert is handled")
        .print_id
        .expect("the alert names a print")
}

/// An adjustment asked for during the turn resumes the print when the turn
/// ends, after the adjustment, and tells the detector.
#[test]
fn an_adjustment_during_the_turn_resumes_the_print_when_the_turn_ends() {
    let world = World::new();
    world.agent.acts_with(fan_up());
    let _ = paused_by_the_detector(&world);

    let fan = world
        .journal
        .position(&Call::SetFanPercent(100.0))
        .expect("the fan was set");
    let resumed = world
        .journal
        .position(&Call::Resume)
        .expect("the print was resumed");
    let told = world
        .journal
        .position(&Call::ClearDetection(DETECTOR_PRINTER_ID))
        .expect("the detector was told");
    assert!(fan < resumed, "the print resumed before the adjustment");
    assert!(resumed < told, "the detector was told before the resume");
    world.journal.assert_no_violations();
}

/// An adjustment asked for after the turn resumes the print once the grace
/// has passed, and not before.
#[test]
fn an_adjustment_outside_a_turn_resumes_the_print_after_the_grace() {
    let world = World::new();
    let print = paused_by_the_detector(&world);
    world.journal.clear();

    let outcome = world.request(print, fan_up()).expect("the request is recorded");
    assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
    let_the_driver_sweep();
    assert_eq!(world.journal.position(&Call::Resume), None, "resumed early");

    world.clock.advance(21);
    world.wait_until("the resume", || {
        world.journal.position(&Call::Resume).is_some()
    });
    world.wait_until("the detector being told", || {
        world
            .journal
            .position(&Call::ClearDetection(DETECTOR_PRINTER_ID))
            .is_some()
    });
}

/// With no adjustment the detector's pause holds, however long it stands.
#[test]
fn without_an_adjustment_the_detectors_pause_holds() {
    let world = World::new();
    let _ = paused_by_the_detector(&world);
    world.clock.advance(3_600);
    let_the_driver_sweep();
    assert_eq!(world.journal.position(&Call::Resume), None);
}

/// An agent acknowledging the detection with `stop` leaves the pause for a
/// person, even after it adjusted.
#[test]
fn acknowledging_stop_leaves_the_pause_for_a_person() {
    let world = World::new();
    let print = paused_by_the_detector(&world);
    let _ = world.request(print, fan_up()).expect("the request is recorded");
    let _ = world
        .request(
            print,
            PrintAction::AcknowledgeFailure {
                event_id: EventId::new(),
                disposition: AcknowledgementDisposition::Stop,
                reason: "nothing I can change will save this".to_owned(),
                actor: agent(),
            },
        )
        .expect("the acknowledgement is recorded");
    world.clock.advance(60);
    let_the_driver_sweep();
    assert_eq!(world.journal.position(&Call::Resume), None);
}

/// A supervisor the envelope does not grant resume leaves the print paused,
/// and says so in the record rather than resuming it another way.
#[test]
fn a_supervisor_not_granted_resume_leaves_the_print_paused() {
    let mut envelope: SafetyEnvelope = permissive_envelope();
    for kinds in envelope.actions.values_mut() {
        kinds.retain(|kind| *kind != printobserver_core::ActionKind::Resume);
    }
    let world = World::with_envelope(envelope);
    world.agent.acts_with(fan_up());
    let _ = paused_by_the_detector(&world);
    assert!(world.journal.position(&Call::SetFanPercent(100.0)).is_some());
    assert_eq!(world.journal.position(&Call::Resume), None);
    assert_eq!(
        world
            .journal
            .position(&Call::ClearDetection(DETECTOR_PRINTER_ID)),
        None
    );
}

/// While the print is paused the agent's interval does not apply; once it is
/// moving, it does.
#[test]
fn the_interval_spaces_out_changes_to_a_moving_print_alone() {
    let mut envelope = permissive_envelope();
    envelope.agent_min_interval_s = 30;
    let world = World::with_envelope(envelope);
    let print = world.open_print(7).id;
    world.printer.reports_state(PrinterState::Paused);
    for _ in 0..2 {
        let outcome = world.request(print, fan_up()).expect("recorded");
        assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
    }
    world.printer.reports_state(PrinterState::Printing);
    let outcome = world.request(print, fan_up()).expect("recorded");
    assert!(
        matches!(
            outcome.record.decision,
            PolicyDecision::Rejected(RejectionReason::MinIntervalNotElapsed { .. })
        ),
        "a change to a moving print did not wait on the interval: {:?}",
        outcome.record.decision
    );
}

/// An event arriving while a turn runs is handed to that turn, which takes it
/// on its next look, and opens no turn of its own.
#[test]
fn an_event_during_a_turn_reaches_that_turn_rather_than_a_second() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);
    world.agent.hold();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| world.handle(failure_alert(7)));
        world.wait_until("the first turn to start", || !world.agent.turns().is_empty());
        let received = printobserver_core::block_on(world.core.receive_event(failure_alert(7)))
            .expect("the second alert is written down");
        assert!(received.turn.is_none(), "the second alert claimed a turn");
        let print = received.event.print_id.expect("it names the print");
        let taken = world.core.await_arrivals(print, Duration::ZERO);
        assert_eq!(taken.len(), 1, "the running turn did not take the arrival");
        assert_eq!(taken[0].event.id, received.event.id);
        world.agent.release();
        first.join().expect("the first turn ends").expect("handled");
    });
    assert_eq!(world.agent.turns().len(), 1, "a second turn ran");
}

/// An event the running turn never took is handed to one more turn, in the
/// same session, once that turn returns.
#[test]
fn an_event_the_turn_never_took_gets_one_more_turn() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);
    world.agent.hold();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| world.handle(failure_alert(7)));
        world.wait_until("the first turn to start", || !world.agent.turns().is_empty());
        let received = printobserver_core::block_on(world.core.receive_event(failure_alert(7)))
            .expect("the second alert is written down");
        assert!(received.turn.is_none());
        world.agent.release();
        first.join().expect("the first turn ends").expect("handled");
    });
    let turns = world.agent.turns();
    assert_eq!(turns.len(), 2, "the arrival was not given a turn");
    assert_ne!(turns[0].event.id, turns[1].event.id);
}
