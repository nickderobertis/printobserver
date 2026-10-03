//! A print the detector paused: the agent's adjustment resumes it, nothing
//! else does, and the detector is told once it has.

use std::time::Duration;

use printobserver_core::{
    AcknowledgementDisposition, ActionKind, Actor, PolicyDecision, PortFailurePayload,
    PortFailureSite, PrintAction, RejectionReason, SafetyEnvelope,
};
use printobserver_printer_api::PrinterState;
use printobserver_types::{EventId, PrintId};
use printobserver_vision_api::VisionError;

use crate::journal::Call;
use crate::world::{DETECTOR_PRINTER_ID, World, detector_paused_alert, permissive_envelope};

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

/// Give the expiry driver long enough to have swept several times.
fn let_the_driver_sweep() {
    std::thread::sleep(Duration::from_millis(100));
}

/// Handle a detector-paused alert for a paused printer, and answer the print
/// and the alert's own event.
fn paused_by_the_detector(world: &World) -> (PrintId, EventId) {
    world.printer.reports_state(PrinterState::Paused);
    let event = world
        .handle(detector_paused_alert(7))
        .expect("the alert is handled");
    (event.print_id.expect("the alert names a print"), event.id)
}

/// An adjustment asked for during the turn is applied at once, and the print
/// is resumed — as the system — when the turn ends, after which the detector
/// is told.
#[test]
fn an_adjustment_during_the_turn_resumes_the_print_when_the_turn_ends() {
    let world = World::new();
    world.agent.acts_with(fan_up());
    let (print, _) = paused_by_the_detector(&world);

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
    let resume = world
        .store
        .action_records()
        .into_iter()
        .find(|record| record.request.action.kind() == ActionKind::Resume)
        .expect("the resume is recorded");
    assert_eq!(resume.request.actor, Actor::System);
    assert_eq!(resume.decision, PolicyDecision::Accepted);
    assert!(
        resume
            .request
            .action
            .reason()
            .contains("the overhang is curling"),
        "the resume does not say which adjustment earned it: {}",
        resume.request.action.reason()
    );
    assert_eq!(resume.print_id, print);
    world.journal.assert_no_violations();
}

/// An adjustment asked for after the turn resumes the print once the grace
/// has passed, and not before.
#[test]
fn an_adjustment_outside_a_turn_resumes_the_print_after_the_grace() {
    let world = World::new();
    let (print, _) = paused_by_the_detector(&world);
    world.journal.clear();

    let outcome = world
        .request(print, fan_up())
        .expect("the request is recorded");
    assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
    world.clock.advance(19);
    let_the_driver_sweep();
    assert_eq!(world.journal.position(&Call::Resume), None, "resumed early");

    world.clock.advance(2);
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

/// A second adjustment inside the grace moves the resume to twenty seconds
/// after it, so the changes reach the print together.
#[test]
fn the_grace_runs_from_the_last_adjustment() {
    let world = World::new();
    let (print, _) = paused_by_the_detector(&world);
    world.journal.clear();

    let _ = world.request(print, fan_up()).expect("recorded");
    world.clock.advance(15);
    let _ = world
        .request(
            print,
            PrintAction::SetFeedrateFactor {
                factor: 0.8,
                duration_s: None,
                reason: "slower over the overhang".to_owned(),
                actor: agent(),
            },
        )
        .expect("recorded");
    world.clock.advance(15);
    let_the_driver_sweep();
    assert_eq!(world.journal.position(&Call::Resume), None, "resumed early");
    world.clock.advance(6);
    world.wait_until("the resume", || {
        world.journal.position(&Call::Resume).is_some()
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
    assert_eq!(
        world
            .journal
            .position(&Call::ClearDetection(DETECTOR_PRINTER_ID)),
        None
    );
}

/// An agent acknowledging the detection with `stop` leaves the pause for a
/// person, even after it adjusted.
#[test]
fn acknowledging_stop_leaves_the_pause_for_a_person() {
    let world = World::new();
    let (print, event) = paused_by_the_detector(&world);
    let _ = world
        .request(print, fan_up())
        .expect("the request is recorded");
    let _ = world
        .request(
            print,
            PrintAction::AcknowledgeFailure {
                event_id: event,
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

/// A system the envelope does not grant resume leaves the print paused, and
/// the refusal is in the record rather than the print resumed another way.
#[test]
fn a_system_not_granted_resume_leaves_the_print_paused() {
    let mut envelope: SafetyEnvelope = permissive_envelope();
    for kinds in envelope.actions.values_mut() {
        kinds.retain(|kind| *kind != ActionKind::Resume);
    }
    let world = World::with_envelope(envelope);
    world.agent.acts_with(fan_up());
    let _ = paused_by_the_detector(&world);
    assert!(
        world
            .journal
            .position(&Call::SetFanPercent(100.0))
            .is_some()
    );
    assert_eq!(world.journal.position(&Call::Resume), None);
    assert_eq!(
        world
            .journal
            .position(&Call::ClearDetection(DETECTOR_PRINTER_ID)),
        None
    );
    let refused = world
        .store
        .action_records()
        .into_iter()
        .find(|record| record.request.action.kind() == ActionKind::Resume)
        .expect("the refused resume is recorded");
    assert_eq!(refused.request.actor, Actor::System);
    assert_eq!(
        refused.decision,
        PolicyDecision::Rejected(RejectionReason::ActorMayNotRequest {
            actor_class: printobserver_core::ActorClass::System,
            action: ActionKind::Resume,
        })
    );
}

/// A detector that will not take the acknowledgement is recorded as a port
/// failure against the detection's own event, after the print resumed.
#[test]
fn an_acknowledgement_the_detector_refuses_is_recorded_against_the_detection() {
    let world = World::new();
    world.vision.refuses_clearing(VisionError::Unreachable {
        detail: "the detector answered 403 Forbidden acknowledging the alert".to_owned(),
    });
    world.agent.acts_with(fan_up());
    let (print, detection) = paused_by_the_detector(&world);
    assert!(world.journal.position(&Call::Resume).is_some());
    let failures: Vec<PortFailurePayload> = world
        .store
        .events_of(print)
        .into_iter()
        .filter_map(|record| record.payload_as::<PortFailurePayload>())
        .map(|read| read.expect("a port failure is of its own type"))
        .collect();
    assert_eq!(
        failures
            .iter()
            .map(|failure| (failure.event_id, failure.site))
            .collect::<Vec<_>>(),
        vec![(detection, PortFailureSite::DetectorAcknowledgement)]
    );
    assert!(failures[0].detail.contains("403 Forbidden"));
}

/// While the print is paused the agent's interval does not hold adjustments
/// back; pausing, resuming and cancelling still wait on it, and once the print
/// is moving every change does.
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
    for waiting in [
        PrintAction::Resume {
            reason: "carry on".to_owned(),
            actor: agent(),
        },
        PrintAction::Cancel {
            reason: "give up".to_owned(),
            actor: agent(),
        },
    ] {
        let outcome = world.request(print, waiting).expect("recorded");
        assert!(
            matches!(
                outcome.record.decision,
                PolicyDecision::Rejected(RejectionReason::MinIntervalNotElapsed { .. })
            ),
            "a resume or cancel of a paused print did not wait on the interval: {:?}",
            outcome.record.decision
        );
    }
    world.printer.reports_state(PrinterState::Printing);
    for moving in [
        fan_up(),
        PrintAction::Pause {
            reason: "hold it".to_owned(),
            actor: agent(),
        },
    ] {
        let outcome = world.request(print, moving).expect("recorded");
        assert!(
            matches!(
                outcome.record.decision,
                PolicyDecision::Rejected(RejectionReason::MinIntervalNotElapsed { .. })
            ),
            "a change to a moving print did not wait on the interval: {:?}",
            outcome.record.decision
        );
    }
}

/// A print an operator resumed is no longer the detector's to hold: the
/// agent's adjustment after it earns no second resume.
#[test]
fn a_print_somebody_else_resumed_is_not_resumed_again() {
    let world = World::new();
    let (print, _) = paused_by_the_detector(&world);
    let resumed = world
        .request(
            print,
            PrintAction::Resume {
                reason: "I looked, it is fine".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("recorded");
    assert_eq!(resumed.record.decision, PolicyDecision::Accepted);
    world.journal.clear();
    let _ = world.request(print, fan_up()).expect("recorded");
    world.clock.advance(60);
    let_the_driver_sweep();
    assert_eq!(world.journal.position(&Call::Resume), None);
}
