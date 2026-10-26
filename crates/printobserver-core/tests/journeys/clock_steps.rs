//! What the supervisor measures as time passing is time passing, whatever the
//! host's wall clock does.
//!
//! A host's time synchronisation steps its wall clock, and the clock here is
//! stepped the same way: [`FakeClock::step_wall`](crate::fakes::FakeClock)
//! moves the wall reading by an hour, forward or back, while no time passes on
//! the clock nothing steps. A bounded intervention still expires after exactly
//! the duration it was asked for, and the agent's minimum interval still holds
//! for exactly the configured interval — while one supervisor lives, and across
//! a restart onto the same store, where what was left of each is taken from
//! the absolute instants the store recorded. The history keeps those absolute
//! instants, as written.

use std::time::Duration;

use printobserver_core::store::{EventStore as _, HistoryQuery};
use printobserver_core::{
    ActionRequestedPayload, Actor, Clock as _, Intervention, InterventionOutcome, PolicyDecision,
    PrintAction, RejectionReason,
};
use printobserver_printer_api::PrinterState;
use printobserver_types::{EventPayload as _, PrintId, Timestamp};

use crate::journal::Call;
use crate::world::{World, agent_actor, permissive_envelope};

/// How long the bounded change in these journeys stands for.
const DURATION_S: i64 = 60;

/// The agent's minimum interval in these journeys.
const INTERVAL_S: i64 = 300;

/// How far the wall clock is stepped.
const HOUR_S: i64 = 3_600;

/// Give the expiry driver long enough to have swept several times.
fn let_the_driver_sweep() {
    std::thread::sleep(Duration::from_millis(100));
}

/// Ask for one bounded fan change, and answer the intervention it opened.
fn fan_change(world: &World, print_id: PrintId) -> Intervention {
    world
        .request(
            print_id,
            PrintAction::SetFanPercent {
                percent: 80.0,
                duration_s: Some(DURATION_S),
                reason: "cooling the overhang".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded")
        .intervention
        .expect("the adjustment opened an intervention")
}

/// Whether the intervention is still active.
fn active(world: &World, intervention: &Intervention) -> bool {
    world
        .store
        .intervention(intervention.id)
        .is_some_and(|held| held.outcome == InterventionOutcome::StillActive)
}

/// Wait for the intervention to be put back, and say it was.
fn restored(world: &World, intervention: &Intervention) {
    world.wait_until("the intervention settles", || !active(world, intervention));
    assert_eq!(
        world
            .store
            .intervention(intervention.id)
            .expect("the intervention reads back")
            .outcome,
        InterventionOutcome::Restored
    );
    assert_eq!(
        world.journal.printer_actions(),
        vec![Call::SetFanPercent(40.0)]
    );
}

/// Hold the intervention active through several sweeps, putting nothing back.
fn still_active(world: &World, intervention: &Intervention) {
    let_the_driver_sweep();
    assert!(
        active(world, intervention),
        "the intervention expired early"
    );
    assert_eq!(
        world.journal.printer_actions(),
        Vec::new(),
        "something was put back"
    );
}

/// A world whose agent must wait the interval between changes to a printing
/// machine, and the print it acts on.
fn spaced_world() -> (World, PrintId) {
    let mut envelope = permissive_envelope();
    envelope.agent_min_interval_s = INTERVAL_S;
    let world = World::with_envelope(envelope);
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    (world, print.id)
}

/// The agent's fan change, which changes the machine, and the decision it got.
fn agent_fan(world: &World, print_id: PrintId) -> PolicyDecision {
    world
        .request(
            print_id,
            PrintAction::SetFanPercent {
                percent: 70.0,
                duration_s: None,
                reason: "a little more cooling".to_owned(),
                actor: agent_actor(print_id),
            },
        )
        .expect("the request is recorded")
        .record
        .decision
}

/// The refusal an agent change gets `since_last_s` into the interval.
const fn too_soon(since_last_s: i64) -> PolicyDecision {
    PolicyDecision::Rejected(RejectionReason::MinIntervalNotElapsed {
        interval_s: INTERVAL_S,
        since_last_s,
    })
}

/// When each action on the print was recorded as requested, oldest first.
fn requested_at(world: &World, print_id: PrintId) -> Vec<Timestamp> {
    let mut events = printobserver_core::block_on(world.store.history(HistoryQuery {
        print_id,
        kinds: vec![ActionRequestedPayload::kind()],
        since: None,
        until: None,
        limit: None,
    }))
    .expect("the history reads");
    events.reverse();
    events.into_iter().map(|event| event.received_at).collect()
}

/// An hour's step of the wall clock forward does not expire an intervention
/// early, and an hour's step back does not hold it past its duration: it is
/// put back after exactly the sixty seconds it was asked for.
#[test]
fn an_intervention_expires_after_its_duration_whatever_the_wall_clock_does() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let intervention = fan_change(&world, print.id);
    world.journal.clear();

    world.clock.step_wall(HOUR_S);
    world.clock.advance(DURATION_S - 1);
    still_active(&world, &intervention);

    world.clock.step_wall(-2 * HOUR_S);
    world.clock.advance(1);
    restored(&world, &intervention);
}

/// A restart takes what was left of an intervention from its persisted
/// deadline, once, and counts that down: twenty seconds into a sixty-second
/// change, the restarted supervisor puts it back forty seconds later, with the
/// wall clock stepped an hour either way meanwhile.
#[test]
fn a_restart_counts_down_what_was_left_of_an_intervention() {
    let mut world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let intervention = fan_change(&world, print.id);
    world.clock.advance(20);

    world.restart();
    world.journal.clear();
    world.clock.step_wall(HOUR_S);
    world.clock.advance(DURATION_S - 20 - 1);
    still_active(&world, &intervention);

    world.clock.step_wall(-2 * HOUR_S);
    world.clock.advance(1);
    restored(&world, &intervention);
}

/// The agent's next change is refused one second short of the interval and
/// accepted at it, with the wall clock stepped an hour forward and then an
/// hour back between them.
#[test]
fn the_agents_interval_holds_for_exactly_its_length_whatever_the_wall_clock_does() {
    let (world, print_id) = spaced_world();
    assert_eq!(agent_fan(&world, print_id), PolicyDecision::Accepted);

    world.clock.step_wall(HOUR_S);
    world.clock.advance(INTERVAL_S - 1);
    assert_eq!(agent_fan(&world, print_id), too_soon(INTERVAL_S - 1));

    world.clock.step_wall(-2 * HOUR_S);
    assert_eq!(agent_fan(&world, print_id), too_soon(INTERVAL_S - 1));
    world.clock.advance(1);
    assert_eq!(agent_fan(&world, print_id), PolicyDecision::Accepted);
}

/// A restart part-way through the agent's interval waits out what was left of
/// it on the restarted supervisor's own clock, with its wall clock stepped,
/// and the history still reads every request at the absolute instant the wall
/// clock gave when it was written.
#[test]
fn a_restart_waits_out_what_was_left_of_the_agents_interval() {
    let (mut world, print_id) = spaced_world();
    let mut written = vec![world.clock.now()];
    assert_eq!(agent_fan(&world, print_id), PolicyDecision::Accepted);
    world.clock.advance(100);

    world.restart();
    world.clock.step_wall(HOUR_S);
    world.clock.advance(INTERVAL_S - 100 - 1);
    written.push(world.clock.now());
    assert_eq!(agent_fan(&world, print_id), too_soon(INTERVAL_S - 1));

    world.clock.step_wall(-2 * HOUR_S);
    written.push(world.clock.now());
    assert_eq!(agent_fan(&world, print_id), too_soon(INTERVAL_S - 1));

    world.clock.advance(1);
    written.push(world.clock.now());
    assert_eq!(agent_fan(&world, print_id), PolicyDecision::Accepted);

    assert_eq!(requested_at(&world, print_id), written);
}
