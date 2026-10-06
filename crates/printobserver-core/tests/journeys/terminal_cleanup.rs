//! A print reaching a terminal state closes its session and expires what it had.
//!
//! Each intervention takes the **same** three-way behaviour an ordinary expiry
//! takes rather than a cleanup path of its own that handles fewer outcomes.
//! Terminal cleanup is where a restoring call is least likely to succeed, the
//! print having ended moments before, so a path that handles a successful
//! restoration and a missing prior value but drops a **failed** one would leave
//! the printer holding the intervention's applied value with nothing in the
//! record saying so.

use std::sync::Arc;
use std::thread;

use printobserver_core::block_on;
use printobserver_core::{Actor, Intervention, InterventionOutcome, PrintAction};
use printobserver_printer_api::PrinterState;
use printobserver_printer_api::{
    FAN_PERCENT_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE, HeaterSnapshot, PrinterError,
    PrinterSnapshot,
};
use printobserver_types::{PrintId, Reported};

use crate::fakes::{PrinterMethod, printer_snapshot};
use crate::journal::Call;
use crate::world::{World, assert_same, failure_alert};

/// The printer this cleanup runs against: a fan and a bed it reports, and a
/// flowrate it does not.
fn reporting_snapshot(state: PrinterState) -> PrinterSnapshot {
    let mut snapshot = printer_snapshot(state);
    snapshot.fan_percent = Some(Reported::new(40.0, FAN_PERCENT_RANGE));
    snapshot.flowrate_factor = None;
    snapshot.bed = Some(HeaterSnapshot {
        actual_c: None,
        target_c: Some(Reported::new(60.0, HEATER_TARGET_C_RANGE)),
        offset_c: Some(Reported::new(0.0, HEATER_OFFSET_C_RANGE)),
    });
    snapshot
}

/// The three interventions this cleanup is driven with, in the order opened.
///
/// The one whose restoring call will fail is opened **first**, so that an
/// implementation abandoning the rest on the first failure is caught.
fn open_three(world: &World, print_id: PrintId) -> (Intervention, Intervention, Intervention) {
    let ask = |action| {
        world
            .request(print_id, action)
            .expect("the change is accepted")
            .intervention
            .expect("it opened an intervention")
    };
    let failing = ask(PrintAction::SetBedTargetC {
        target_c: 70.0,
        duration_s: Some(3600),
        reason: "a warmer bed for the first layers".to_owned(),
        actor: Actor::Operator,
    });
    let restorable = ask(PrintAction::SetFanPercent {
        percent: 80.0,
        duration_s: Some(3600),
        reason: "cooling the overhang".to_owned(),
        actor: Actor::Operator,
    });
    let unreported = ask(PrintAction::SetFlowrateFactor {
        factor: 1.1,
        duration_s: Some(3600),
        reason: "a little more material".to_owned(),
        actor: Actor::Operator,
    });
    assert_eq!(failing.prior_value, Some(60.0));
    assert_eq!(restorable.prior_value, Some(40.0));
    assert_eq!(unreported.prior_value, None);
    (failing, restorable, unreported)
}

/// The session was closed carrying that state as its reason, and the print ended.
fn assert_closed(world: &World, print_id: PrintId, state: &PrinterState) {
    let reason = format!("the print reached {state:?}");
    assert_eq!(
        world
            .journal
            .calls()
            .into_iter()
            .filter(|call| matches!(call, Call::CloseSession(_, _)))
            .collect::<Vec<_>>(),
        vec![Call::CloseSession(print_id, reason)],
        "{state:?}: the session was not closed carrying that state"
    );
    assert!(
        world
            .journal
            .position(&Call::EndPrint(format!("{state:?}")))
            .is_some(),
        "{state:?}: the print was not ended"
    );
}

/// Each of the three took the outcome its own prior value and printer earned it.
fn assert_three_outcomes(
    world: &World,
    state: &PrinterState,
    failure: &PrinterError,
    held: (&Intervention, &Intervention, &Intervention),
) {
    let (failing, restorable, unreported) = held;

    assert!(
        world
            .journal
            .printer_actions()
            .contains(&Call::SetFanPercent(40.0)),
        "{state:?}: the restorable intervention did not go back"
    );
    assert_eq!(
        world
            .store
            .intervention(restorable.id)
            .expect("it reads back")
            .outcome,
        InterventionOutcome::Restored
    );

    assert!(
        !world
            .journal
            .printer_actions()
            .iter()
            .any(|call| matches!(call, Call::SetFlowrateFactor(_))),
        "{state:?}: a guessed default was written onto the machine"
    );
    assert_eq!(
        world
            .store
            .intervention(unreported.id)
            .expect("it reads back")
            .outcome,
        InterventionOutcome::RestoreUnavailable
    );

    let settled = world.store.intervention(failing.id).expect("it reads back");
    assert_eq!(
        settled.outcome,
        InterventionOutcome::RestoreFailed {
            reason: failure.to_string()
        },
        "{state:?}: the failed restoration was not recorded as one"
    );
    assert_same(settled.applied_value, 70.0);
    assert_eq!(settled.prior_value, Some(60.0));

    for one in [failing.id, restorable.id, unreported.id] {
        assert_ne!(
            world
                .store
                .intervention(one)
                .expect("it reads back")
                .outcome,
            InterventionOutcome::StillActive,
            "{state:?}: an intervention was left active by the cleanup"
        );
    }
}

/// A print reaching each terminal state expires all three of its interventions
/// and closes its session, whichever order cleanup takes them in.
#[test]
fn each_terminal_state_expires_every_intervention_and_closes_the_session() {
    let failure = PrinterError::StateConflict {
        detail: "the print has ended".to_owned(),
    };

    for state in printobserver_core::TERMINAL_STATES {
        let world = World::new();
        let print = world.open_print(7);
        world
            .printer
            .reports(reporting_snapshot(PrinterState::Printing));
        let (failing, restorable, unreported) = open_three(&world, print.id);

        world.printer.reports(reporting_snapshot(state.clone()));
        world
            .printer
            .fails(PrinterMethod::SetBedTargetC, failure.clone());
        world.journal.clear();

        world
            .handle(failure_alert(7))
            .expect("the terminal event is handled");

        assert_closed(&world, print.id, &state);
        assert_three_outcomes(
            &world,
            &state,
            &failure,
            (&failing, &restorable, &unreported),
        );
        world.journal.assert_no_violations();
    }
}

/// The three ways a print is read with no supervision turn involved.
const READS: [&str; 3] = ["listing", "status", "context"];

/// Read one print the way `read` names, with no turn involved.
///
/// The status read and the context read are one read of the core — the
/// server's status answer is the print's context with its session beside it.
fn read(world: &World, read: &str, print_id: PrintId) {
    match read {
        "listing" => {
            block_on(world.core.list_and_adopt_prints()).expect("the prints are listed");
        }
        "status" | "context" => {
            let context = world.context(print_id).expect("the context reads");
            assert_eq!(
                context.print,
                world.store.print_now(print_id).expect("the print reads"),
                "{read}: the context answered the print as it stood before the read"
            );
            assert!(
                context.interventions.is_empty(),
                "{read}: the context answered interventions the close-out expired"
            );
        }
        other => panic!("no read is called {other}"),
    }
}

/// A read that finds the printer's job over closes the print through the same
/// cleanup a turn takes — every intervention expired with the outcome it
/// earned, the print ended, the session closed — and no turn is run for it.
#[test]
fn a_read_that_finds_the_job_over_closes_the_print_through_the_same_cleanup() {
    let failure = PrinterError::StateConflict {
        detail: "the print has ended".to_owned(),
    };
    for one in READS {
        for state in [
            PrinterState::Operational,
            PrinterState::Cancelling,
            PrinterState::Error,
            PrinterState::Offline,
        ] {
            let world = World::new();
            let print = world.open_print(7);
            world
                .printer
                .reports(reporting_snapshot(PrinterState::Printing));
            let (failing, restorable, unreported) = open_three(&world, print.id);

            world.printer.reports(reporting_snapshot(state.clone()));
            world.printer.reports_job(Some("benchy.gcode"), state.clone());
            world
                .printer
                .fails(PrinterMethod::SetBedTargetC, failure.clone());
            world.journal.clear();

            read(&world, one, print.id);

            assert_closed(&world, print.id, &state);
            assert_three_outcomes(
                &world,
                &state,
                &failure,
                (&failing, &restorable, &unreported),
            );
            let ended = world.store.print_now(print.id).expect("the print reads");
            assert_eq!(
                ended.end_reason,
                Some(format!("the print reached {state:?}")),
                "{one}: {state:?}"
            );
            assert_eq!(ended.state, state, "{one}");
            assert!(
                !world
                    .journal
                    .calls()
                    .iter()
                    .any(|call| matches!(call, Call::RunTurn(_))),
                "{one}: closing the print ran a supervision turn"
            );

            world.journal.clear();
            read(&world, one, print.id);
            assert_eq!(
                world.journal.count(&Call::EndPrint(format!("{state:?}"))),
                0,
                "{one}: a second read ended the print a second time"
            );
            world.journal.assert_no_violations();
        }
    }
}

/// A printer that cannot be read, or that reports a state this vocabulary does
/// not name, closes nothing: neither is a finding that the job is over.
#[test]
fn a_read_that_cannot_tell_whether_the_job_is_over_closes_nothing() {
    for one in READS {
        let world = World::new();
        let print = world.open_print(7);
        world.printer.fails(
            PrinterMethod::Job,
            PrinterError::Unreachable {
                detail: "the machine is switched off".to_owned(),
            },
        );
        read(&world, one, print.id);
        assert_eq!(
            world.store.print_now(print.id).and_then(|held| held.ended_at),
            None,
            "{one}: an unreadable printer closed the print"
        );

        world.printer.heals();
        world.printer.reports_job(
            Some("benchy.gcode"),
            PrinterState::Unknown("Detecting serial connection".to_owned()),
        );
        read(&world, one, print.id);
        assert_eq!(
            world.store.print_now(print.id).and_then(|held| held.ended_at),
            None,
            "{one}: a state nobody named closed the print"
        );
        assert!(
            !world
                .journal
                .calls()
                .iter()
                .any(|call| matches!(call, Call::EndPrint(_) | Call::CloseSession(_, _))),
            "{one}: something was closed"
        );
    }
}

/// A read made while a turn holds the print — the agent's own read, from
/// inside that turn — closes nothing and does not wait on the turn; the next
/// read after the turn closes it.
#[test]
fn a_read_inside_a_running_turn_leaves_the_print_to_the_turn() {
    for one in READS {
        let world = Arc::new(World::new());
        let print = world.open_print(7);
        world.agent.hold();
        let turn = {
            let driving = Arc::clone(&world);
            thread::spawn(move || driving.handle(failure_alert(7)))
        };
        world.wait_until("the turn is entered", || world.agent.entered() >= 1);

        world
            .printer
            .reports_job(Some("benchy.gcode"), PrinterState::Operational);
        let reading = {
            let reader = Arc::clone(&world);
            thread::spawn(move || read(&reader, one, print.id))
        };
        world.wait_until("the read returns while the turn is held", || {
            reading.is_finished()
        });
        reading.join().expect("the read ran");
        assert_eq!(
            world.store.print_now(print.id).and_then(|held| held.ended_at),
            None,
            "{one}: a read closed the print a running turn holds"
        );

        world.agent.release();
        turn.join()
            .expect("the turn ran")
            .expect("the event is handled");
        read(&world, one, print.id);
        assert_closed(&world, print.id, &PrinterState::Operational);
    }
}

/// A turn claimed for a print that a read then found over runs nothing: the
/// print ended before the turn held it, and a turn acts on no ended print.
#[test]
fn a_turn_whose_print_a_read_closed_first_runs_nothing() {
    let world = World::new();
    let print = world.open_print(7);
    let received =
        block_on(world.core.receive_event(failure_alert(7))).expect("the event is written down");
    let turn = received.turn.expect("the event claimed a turn");

    world
        .printer
        .reports_job(Some("benchy.gcode"), PrinterState::Operational);
    read(&world, "listing", print.id);
    assert_closed(&world, print.id, &PrinterState::Operational);
    world.journal.clear();

    block_on(world.core.run_supervision(turn)).expect("the claimed turn settles");

    assert_eq!(
        world.journal.calls(),
        vec![Call::ReadPrint],
        "a turn for an ended print did more than find it ended"
    );
    let next = block_on(world.core.receive_event(failure_alert(7))).expect("written down");
    assert_eq!(next.turn, None, "an ended print claimed another turn");
}
