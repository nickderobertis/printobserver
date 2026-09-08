//! A print reaching a terminal state closes its session and expires what it had.
//!
//! Each intervention takes the **same** three-way behaviour an ordinary expiry
//! takes rather than a cleanup path of its own that handles fewer outcomes.
//! Terminal cleanup is where a restoring call is least likely to succeed, the
//! print having ended moments before, so a path that handles a successful
//! restoration and a missing prior value but drops a **failed** one would leave
//! the printer holding the intervention's applied value with nothing in the
//! record saying so.

use printobserver_printer_api::PrinterError;
use printobserver_types::{
    Actor, FAN_PERCENT_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE, HeaterSnapshot,
    Intervention, InterventionOutcome, PrintAction, PrintId, PrinterState, Reported,
};

use crate::fakes::{PrinterMethod, printer_snapshot};
use crate::journal::Call;
use crate::world::{World, assert_same, failure_alert};

/// The printer this cleanup runs against: a fan and a bed it reports, and a
/// flowrate it does not.
fn reporting_snapshot(state: PrinterState) -> printobserver_types::PrinterSnapshot {
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
