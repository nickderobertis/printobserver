//! An event arriving while a turn is running is queued behind it.
//!
//! The property owed is **serialization**, not ordering, and it is asserted as
//! the absence of overlap the fake itself observes rather than as the order two
//! calls were recorded in: two turns recorded in order against one session are
//! equally consistent with an implementation that ran them side by side.
//!
//! So the fake records the moment each turn is entered and the moment it
//! returns, keeps a running count of turns entered and not yet returned, and
//! holds the first call until this test releases it. An implementation that
//! dispatches the second turn concurrently is entered while the first is held,
//! and fails on the entered-exactly-once assertion and on the high-water count
//! where an ordering assertion alone would pass it. One that serializes by
//! never handling the second event at all fails on both turns completing.

use std::sync::Arc;
use std::thread;

use printobserver_types::{EventKind, PrinterState};

use crate::journal::Call;
use crate::world::{World, failure_alert};

/// A second event of one print is handled only after the turn ahead of it.
#[test]
fn a_second_event_of_one_print_waits_for_the_turn_ahead_of_it() {
    let world = Arc::new(World::new());
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    world.agent.hold();

    let first = {
        let driving = Arc::clone(&world);
        thread::spawn(move || driving.handle(failure_alert(7)))
    };
    world.wait_until("the first turn is entered", || world.agent.entered() >= 1);

    let second = {
        let driving = Arc::clone(&world);
        thread::spawn(move || driving.handle(failure_alert(7)))
    };
    // The append is the first thing the loop does for any event, so an
    // implementation handling the second event beside the first has by now
    // reached the point where it would call the supervisor port.
    world.wait_until("the second event is appended", || {
        world
            .journal
            .count(&Call::AppendEvent(EventKind::ObicoFailureAlert))
            >= 2
    });
    assert_eq!(
        world.agent.entered(),
        1,
        "a second turn was entered while the first was still running"
    );
    assert_eq!(world.agent.live(), 1);

    world.agent.release();
    first
        .join()
        .expect("the first thread completes")
        .expect("the first event is handled");
    second
        .join()
        .expect("the second thread completes")
        .expect("the second event is handled");

    assert_eq!(
        world.agent.high_water(),
        1,
        "two turns were live at once at some point in the run"
    );
    let (entries, exits) = world.agent.instants();
    assert_eq!(entries.len(), 2, "both turns were entered");
    assert_eq!(exits.len(), 2, "both turns completed");
    assert!(
        entries[1] >= exits[0],
        "the second turn was entered before the first returned"
    );

    // One print, one session, and no close between the two turns.
    assert_eq!(
        world.store.prints().len(),
        1,
        "a second identifier was minted"
    );
    let turns = world.agent.turns();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].print_id, print.id);
    assert_eq!(turns[1].print_id, print.id);
    assert_eq!(
        world
            .store
            .events_of(print.id)
            .into_iter()
            .filter(|record| record.kind() == EventKind::SupervisionSessionOpened)
            .count(),
        1,
        "a second session was opened for the queued event"
    );
    assert_eq!(
        world
            .journal
            .calls()
            .iter()
            .filter(|call| matches!(call, Call::CloseSession(_, _)))
            .count(),
        0,
        "the session was closed between the two turns"
    );
    world.journal.assert_no_violations();
}
