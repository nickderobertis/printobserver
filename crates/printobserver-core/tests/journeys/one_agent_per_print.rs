//! One agent per print: an event arriving during a turn reaches that turn, what
//! it never took gets one more turn, and an ended print starts none.

use std::time::Duration;

use printobserver_printer_api::PrinterState;

use crate::world::{World, detector_paused_alert, failure_alert};

/// An event arriving while a turn runs is handed to that turn, which takes it
/// on its next look, and opens no turn of its own.
#[test]
fn an_event_during_a_turn_reaches_that_turn_rather_than_a_second() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);
    world.agent.hold();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| world.handle(failure_alert(7)));
        world.wait_until("the first turn to start", || world.agent.entered() == 1);
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

/// What the running turn never took is handed to exactly one more turn once
/// it returns: the newest as its event, the rest as what arrived while busy.
#[test]
fn what_the_turn_never_took_gets_exactly_one_more_turn() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);
    world.agent.hold();
    let mut handed = Vec::new();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| world.handle(failure_alert(7)));
        world.wait_until("the first turn to start", || world.agent.entered() == 1);
        for _ in 0..2 {
            let received = printobserver_core::block_on(world.core.receive_event(failure_alert(7)))
                .expect("a later alert is written down");
            assert!(received.turn.is_none());
            handed.push(received.event.id);
        }
        world.agent.release();
        first.join().expect("the first turn ends").expect("handled");
    });
    let turns = world.agent.turns();
    assert_eq!(
        turns.len(),
        2,
        "what arrived was not given exactly one turn"
    );
    assert_eq!(turns[1].event.id, handed[1]);
    assert_eq!(
        turns[1]
            .situation
            .arrived_while_busy
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        vec![handed[0]]
    );
}

/// A turn is told what the supervisor knew as it began: the printer's state
/// and what the detector did.
#[test]
fn a_turn_is_told_its_situation() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Paused);
    let _ = world
        .handle(detector_paused_alert(7))
        .expect("the alert is handled");
    world.printer.reports_state(PrinterState::Printing);
    let _ = world.handle(failure_alert(7)).expect("handled");
    let turns = world.agent.turns();
    let detected = &turns[0].situation;
    assert_eq!(detected.printer_state.as_deref(), Some("paused"));
    assert_eq!(detected.detector_warned, Some(false));
    assert_eq!(detected.detector_paused_the_print, Some(true));
    assert!(detected.arrived_while_busy.is_empty());
    let undetected = &turns[1].situation;
    assert_eq!(undetected.printer_state.as_deref(), Some("printing"));
    assert_eq!(undetected.detector_warned, None);
    assert_eq!(undetected.detector_paused_the_print, None);
}

/// A print whose turn saw it end releases its inbox: what was waiting for it
/// gets no turn, and neither does an event that arrives for it afterwards.
#[test]
fn an_ended_print_releases_its_inbox() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Operational);
    world.agent.hold();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| world.handle(failure_alert(7)));
        world.wait_until("the first turn to start", || world.agent.entered() == 1);
        let received = printobserver_core::block_on(world.core.receive_event(failure_alert(7)))
            .expect("the second alert is written down");
        assert!(received.turn.is_none());
        world.agent.release();
        first.join().expect("the first turn ends").expect("handled");
    });
    assert_eq!(
        world.agent.turns().len(),
        1,
        "an ended print got a second turn"
    );
    let later = printobserver_core::block_on(world.core.receive_event(failure_alert(7)))
        .expect("a later alert is written down");
    assert!(later.turn.is_none(), "an ended print claimed a turn");
    assert!(
        later.event.print_id.is_some(),
        "the later alert was not attributed"
    );
    assert_eq!(world.agent.turns().len(), 1);
}
