//! A bounded intervention lives its whole life correctly.
//!
//! The corpus is driven through the real core against fakes that record the
//! order of every call. Expiry is driven by **advancing the clock alone** —
//! calling no expiry method and issuing no action — because a bounded
//! intervention expires on its own and nothing has to ask it to.

use printobserver_types::{
    Actor, Adjustable, FEEDRATE_FACTOR_RANGE, Intervention, InterventionOutcome, PolicyDecision,
    PrintAction, PrintId, PrinterState, Range, RejectionReason, Reported,
};

use crate::journal::Call;
use crate::world::World;

/// How long every bounded adjustment in this corpus stands for.
const DURATION_S: i64 = 60;

/// Ask for one bounded fan change, and answer the intervention it opened.
fn fan_change(world: &World, print_id: PrintId, percent: f64, duration_s: i64) -> Intervention {
    world
        .request(
            print_id,
            PrintAction::SetFanPercent {
                percent,
                duration_s: Some(duration_s),
                reason: "cooling the overhang".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded")
        .intervention
        .expect("the adjustment opened an intervention")
}

/// An accepted adjustment reads the snapshot before it acts, and keeps what it
/// read as the value expiry will put back.
#[test]
fn an_accepted_adjustment_reads_the_snapshot_before_it_acts() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    world.journal.clear();

    let intervention = world
        .request(
            print.id,
            PrintAction::SetFanPercent {
                percent: 80.0,
                duration_s: Some(DURATION_S),
                reason: "cooling the overhang".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded")
        .intervention
        .expect("it opened an intervention");

    let read = world
        .journal
        .position(&Call::Snapshot)
        .expect("the printer was read");
    let acted = world
        .journal
        .position(&Call::SetFanPercent(80.0))
        .expect("the printer was acted on");
    assert!(read < acted, "the adjustment acted before it read the printer");
    assert_eq!(intervention.prior_value, Some(40.0));
    assert_eq!(intervention.applied_value, 80.0);
    assert_eq!(intervention.outcome, InterventionOutcome::StillActive);
    world.journal.assert_no_violations();
}

/// An intervention expires with nothing asking it to, and puts the value back.
#[test]
fn an_intervention_expires_on_the_clock_alone_and_restores_the_prior_value() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let intervention = fan_change(&world, print.id, 80.0, DURATION_S);
    world.journal.clear();

    world.clock.advance(DURATION_S + 1);

    world.wait_until("the intervention settles", || {
        world
            .store
            .intervention(intervention.id)
            .is_some_and(|held| held.outcome != InterventionOutcome::StillActive)
    });

    assert_eq!(
        world.journal.printer_actions(),
        vec![Call::SetFanPercent(40.0)],
        "the prior value did not go back"
    );
    let decided = world
        .journal
        .position(&Call::RecordAction(PolicyDecision::Accepted))
        .expect("the restoration was decided");
    let acted = world
        .journal
        .position(&Call::SetFanPercent(40.0))
        .expect("the restoration reached the printer");
    assert!(decided < acted, "the restoration acted before it was decided");

    let settled = world
        .store
        .intervention(intervention.id)
        .expect("the intervention reads back");
    assert_eq!(settled.outcome, InterventionOutcome::Restored);
    assert!(settled.restored_at.is_some());
    world.journal.assert_no_violations();
}

/// A prior value outside the bounds now in force is rejected, not written.
#[test]
fn a_prior_value_outside_the_current_bounds_is_rejected_and_recorded() {
    let world = World::new();
    let print = world.open_print(7);
    let mut snapshot = crate::fakes::printer_snapshot(PrinterState::Printing);
    // Plausible for the printer to report, and outside what any actor may ask
    // for: the two are different contracts and are never substituted.
    snapshot.feedrate_factor = Some(Reported::new(5.0, FEEDRATE_FACTOR_RANGE));
    world.printer.reports(snapshot);

    let intervention = world
        .request(
            print.id,
            PrintAction::SetFeedrateFactor {
                factor: 1.5,
                duration_s: Some(DURATION_S),
                reason: "slowing for the bridge".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded")
        .intervention
        .expect("it opened an intervention");
    assert_eq!(intervention.prior_value, Some(5.0));
    world.journal.clear();

    world.clock.advance(DURATION_S + 1);
    world.wait_until("the intervention settles", || {
        world
            .store
            .intervention(intervention.id)
            .is_some_and(|held| held.outcome != InterventionOutcome::StillActive)
    });

    assert_eq!(
        world.journal.printer_actions(),
        Vec::new(),
        "an out-of-bounds restoring value reached the printer"
    );
    let rejection = PolicyDecision::Rejected(RejectionReason::OutOfBounds {
        adjustable: Adjustable::Feedrate,
        requested: 5.0,
        allowed: Range::new(0.5, 2.0),
    });
    assert!(
        world
            .journal
            .position(&Call::RecordAction(rejection))
            .is_some(),
        "the refused restoration was not recorded"
    );
    let settled = world
        .store
        .intervention(intervention.id)
        .expect("the intervention reads back");
    assert!(
        matches!(settled.outcome, InterventionOutcome::RestoreFailed { .. }),
        "the outcome was {:?}",
        settled.outcome
    );
    assert_eq!(settled.applied_value, 1.5);
    assert_eq!(settled.prior_value, Some(5.0));
    world.journal.assert_no_violations();
}

/// An intervention whose prior value the printer did not report restores nothing.
#[test]
fn an_intervention_with_no_prior_value_restores_nothing_and_says_so() {
    let world = World::new();
    let print = world.open_print(7);
    let mut snapshot = crate::fakes::printer_snapshot(PrinterState::Printing);
    snapshot.fan_percent = None;
    world.printer.reports(snapshot);

    let intervention = world
        .request(
            print.id,
            PrintAction::SetFanPercent {
                percent: 80.0,
                duration_s: Some(DURATION_S),
                reason: "cooling the overhang".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded")
        .intervention
        .expect("it opened an intervention");
    assert_eq!(intervention.prior_value, None);
    world.journal.clear();

    world.clock.advance(DURATION_S + 1);
    world.wait_until("the intervention settles", || {
        world
            .store
            .intervention(intervention.id)
            .is_some_and(|held| held.outcome != InterventionOutcome::StillActive)
    });

    assert_eq!(
        world.journal.printer_actions(),
        Vec::new(),
        "a guessed default was written onto the machine"
    );
    assert_eq!(
        world
            .store
            .intervention(intervention.id)
            .expect("it reads back")
            .outcome,
        InterventionOutcome::RestoreUnavailable
    );
    world.journal.assert_no_violations();
}

/// An adjustable changed again before expiry supersedes rather than stacks, and
/// carries the earlier prior value forward so restoring reaches where it began.
#[test]
fn a_second_change_supersedes_the_first_and_carries_its_prior_value_forward() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);

    let first = world
        .request(
            print.id,
            PrintAction::SetFanPercent {
                percent: 80.0,
                duration_s: Some(600),
                reason: "cooling the overhang".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the first request")
        .intervention
        .expect("it opened an intervention");
    assert_eq!(first.prior_value, Some(40.0));

    // The machine now reports what the first change put on it, which is what a
    // second change would capture if it did not carry the earlier value forward.
    let mut snapshot = crate::fakes::printer_snapshot(PrinterState::Printing);
    snapshot.fan_percent = Some(Reported::new(
        80.0,
        printobserver_types::FAN_PERCENT_RANGE,
    ));
    world.printer.reports(snapshot);

    let second = world
        .request(
            print.id,
            PrintAction::SetFanPercent {
                percent: 60.0,
                duration_s: Some(DURATION_S),
                reason: "a little less".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the second request")
        .intervention
        .expect("it opened an intervention");

    assert_eq!(
        world
            .store
            .intervention(first.id)
            .expect("the first reads back")
            .outcome,
        InterventionOutcome::Superseded { by: second.id }
    );
    assert_eq!(
        second.prior_value,
        Some(40.0),
        "the later intervention did not carry the earlier's prior value forward"
    );

    world.journal.clear();
    world.clock.advance(DURATION_S + 1);
    world.wait_until("the second intervention settles", || {
        world
            .store
            .intervention(second.id)
            .is_some_and(|held| held.outcome != InterventionOutcome::StillActive)
    });

    assert_eq!(
        world.journal.printer_actions(),
        vec![Call::SetFanPercent(40.0)],
        "restoring did not reach the value the print started from"
    );
    assert_eq!(
        world
            .store
            .intervention(second.id)
            .expect("it reads back")
            .outcome,
        InterventionOutcome::Restored
    );
    world.journal.assert_no_violations();
}
