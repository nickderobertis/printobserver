//! The periodic observation: which prints it reaches, what it carries, and
//! what becomes of it when a port fails.
//!
//! Each journey asks the supervisor to observe exactly as the server's driver
//! does once per interval — [`Supervisor::observe_active_prints`], then
//! [`Supervisor::run_supervision`] for every turn it claimed — and reads what
//! the agent was handed and what the history recorded.
//!
//! [`Supervisor::observe_active_prints`]: printobserver_core::Supervisor::observe_active_prints
//! [`Supervisor::run_supervision`]: printobserver_core::Supervisor::run_supervision

use printobserver_core::store::StoreError;
use printobserver_core::{
    PendingTurn, PeriodicObservationPayload, PortFailurePayload, PortFailureSite, block_on,
};
use printobserver_printer_api::{PrinterError, PrinterState};
use printobserver_types::{EventKind, EventPayload, EventRecord, PrintId};
use printobserver_vision_api::VisionError;

use crate::fakes::{PrinterMethod, StoreMethod};
use crate::journal::Call;
use crate::world::{World, failure_alert};

/// Observe every active print once, as one round of the driver does, and
/// answer the turns it claimed.
fn observe(world: &World) -> Vec<PendingTurn> {
    let observed = block_on(world.core.observe_active_prints()).expect("the open prints are read");
    assert!(
        observed.refused.is_empty(),
        "an observation was refused: {:?}",
        observed.refused
    );
    observed.turns
}

/// What the store says when it has failed.
fn disk_full() -> StoreError {
    StoreError::Io {
        detail: "the disk is full".to_owned(),
    }
}

/// Observe once and run every turn it claimed.
fn observe_and_supervise(world: &World) -> usize {
    let turns = observe(world);
    let claimed = turns.len();
    for turn in turns {
        block_on(world.core.run_supervision(turn)).expect("the turn is supervised");
    }
    claimed
}

/// The kind an observation is written under.
fn observation_kind() -> EventKind {
    EventKind::new(PeriodicObservationPayload::KIND).expect("a kind")
}

/// The observations one print's history holds, oldest first.
fn observations(world: &World, print_id: PrintId) -> Vec<EventRecord> {
    world
        .store
        .events_of(print_id)
        .into_iter()
        .filter(|event| event.payload_as::<PeriodicObservationPayload>().is_some())
        .collect()
}

/// One observation's payload.
fn payload(event: &EventRecord) -> PeriodicObservationPayload {
    event
        .payload_as::<PeriodicObservationPayload>()
        .expect("an observation")
        .expect("of its own type")
}

/// The port failures recorded against one event, by where each happened.
fn failures_against(world: &World, print_id: PrintId, event: &EventRecord) -> Vec<PortFailureSite> {
    world
        .store
        .events_of(print_id)
        .iter()
        .filter_map(|record| record.payload_as::<PortFailurePayload>()?.ok())
        .filter(|failure| failure.event_id == event.id)
        .map(|failure| failure.site)
        .collect()
}

/// A job the printer is running that no print records is adopted, observed
/// with the printer's and the job's telemetry and the camera's frame, and its
/// agent handed a turn on it with no detection behind it.
#[test]
fn a_running_job_nobody_opened_is_adopted_and_observed() {
    let world = World::with_camera();

    assert_eq!(observe_and_supervise(&world), 1);

    let prints = world.store.prints();
    assert_eq!(prints.len(), 1, "the running job was not adopted");
    let adopted = &prints[0];
    assert_eq!(adopted.file_name.as_deref(), Some("benchy.gcode"));
    let turns = world.agent.turns();
    assert_eq!(turns.len(), 1);
    let turn = &turns[0];
    assert_eq!(turn.print_id, adopted.id);
    let observed = payload(&turn.event);
    assert_eq!(
        observed.printer.map(|printer| printer.connection),
        Some(PrinterState::Printing)
    );
    assert_eq!(
        observed.job.and_then(|job| job.file_name).as_deref(),
        Some("benchy.gcode")
    );
    assert_eq!(observed.interval_s, 120);
    assert!(
        turn.event.image.is_some(),
        "the frame is not the event's image"
    );
    assert!(
        turn.image_path.is_some(),
        "the turn is not handed the frame"
    );
    assert_eq!(turn.situation.detector_warned, None);
    assert_eq!(turn.situation.detector_paused_the_print, None);
    assert_eq!(turn.situation.printer_state.as_deref(), Some("printing"));
}

/// Where the printer's job is known, only the print it is is observed: an open
/// print of another file is not printing, and an ended print is over.
#[test]
fn only_the_print_the_job_is_is_observed() {
    let world = World::new();
    let running = world.open_print(7);
    let other = block_on(printobserver_core::store::PrintStore::open_print(
        world.store.as_ref(),
        Some(8),
        Some("other.gcode".to_owned()),
    ))
    .expect("a print opens");

    assert_eq!(observe_and_supervise(&world), 1);

    assert_eq!(observations(&world, running.id).len(), 1);
    assert!(observations(&world, other.id).is_empty());
    assert!(
        world
            .agent
            .turns()
            .iter()
            .all(|turn| turn.print_id == running.id)
    );
}

/// A printer whose state and job cannot be read leaves every open print a
/// candidate: each is observed with no telemetry, each read's failure is
/// recorded against its observation, and each agent still gets its turn.
#[test]
fn an_unreadable_printer_observes_every_open_print_without_telemetry() {
    let world = World::new();
    let first = world.open_print(7);
    let second = block_on(printobserver_core::store::PrintStore::open_print(
        world.store.as_ref(),
        Some(8),
        Some("other.gcode".to_owned()),
    ))
    .expect("a print opens");
    let unreachable = PrinterError::Unreachable {
        detail: "the machine is switched off".to_owned(),
    };
    world
        .printer
        .fails(PrinterMethod::Snapshot, unreachable.clone());
    world.printer.fails(PrinterMethod::Job, unreachable);

    let claimed = observe(&world);

    for print in [first.id, second.id] {
        let held = observations(&world, print);
        assert_eq!(held.len(), 1);
        let observed = payload(&held[0]);
        assert!(observed.printer.is_none() && observed.job.is_none());
        assert_eq!(
            failures_against(&world, print, &held[0]),
            [
                PortFailureSite::PrinterSnapshot,
                PortFailureSite::PrinterJob
            ]
        );
    }
    assert_eq!(claimed.len(), 2);
    for turn in claimed {
        block_on(world.core.run_supervision(turn)).expect("the turn is supervised");
    }
    assert_eq!(world.agent.turns().len(), 2);
}

/// A printer reporting a terminal state is printing nothing, so an open print
/// of its job is not observed, and the print is left free for the next event.
#[test]
fn a_printer_in_a_terminal_state_is_not_observed() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Error);

    assert!(observe(&world).is_empty());

    assert!(observations(&world, print.id).is_empty());
    assert!(world.agent.turns().is_empty());
    world.printer.reports_state(PrinterState::Printing);
    let _ = world
        .handle(failure_alert(7))
        .expect("the alert is handled");
    assert_eq!(world.agent.turns().len(), 1, "the print was left claimed");
}

/// A print whose turn is running is given no observation, written or queued.
#[test]
fn a_print_whose_turn_is_running_is_not_observed() {
    let world = World::new();
    let print = world.open_print(7);
    world.agent.hold();
    std::thread::scope(|scope| {
        let running = scope.spawn(|| world.handle(failure_alert(7)));
        world.wait_until("the alert's turn to start", || world.agent.entered() == 1);

        assert!(observe(&world).is_empty());

        assert!(observations(&world, print.id).is_empty());
        assert!(
            world
                .core
                .await_arrivals(print.id, std::time::Duration::ZERO)
                .is_empty(),
            "an observation was handed to the running turn"
        );
        world.agent.release();
        running.join().expect("the turn ends").expect("handled");
    });
    assert_eq!(world.agent.turns().len(), 1);
    assert_eq!(
        observe_and_supervise(&world),
        1,
        "the print was left claimed"
    );
}

/// No camera configured is an observation with no frame, and the vision port
/// is never asked for one.
#[test]
fn with_no_camera_an_observation_carries_no_frame() {
    let world = World::new();
    let _ = world.open_print(7);

    assert_eq!(observe_and_supervise(&world), 1);

    let turn = &world.agent.turns()[0];
    assert!(turn.event.image.is_none());
    assert!(turn.image_path.is_none());
    assert!(
        !world.journal.calls().contains(&Call::FetchImage),
        "the vision port was asked for a frame nobody configured"
    );
}

/// A frame the camera will not give, or the store will not keep, is recorded
/// against the observation, and the turn runs on the telemetry alone.
#[test]
fn a_frame_that_is_not_had_is_recorded_against_the_observation() {
    for failing in ["camera", "store"] {
        let world = World::with_camera();
        let print = world.open_print(7);
        if failing == "camera" {
            world.vision.fails(VisionError::TimedOut);
        } else {
            world.store.fails(StoreMethod::PutImage, disk_full());
        }

        assert_eq!(observe_and_supervise(&world), 1, "{failing}");

        let turn = &world.agent.turns()[0];
        assert!(turn.event.image.is_none(), "{failing}");
        assert_eq!(
            failures_against(&world, print.id, &turn.event),
            [PortFailureSite::CameraLook],
            "{failing}"
        );
    }
}

/// An observation the store will not write down claims no turn and is
/// answered with the store's own error; the print is released, so the next
/// round observes it once the store has recovered.
#[test]
fn an_observation_that_cannot_be_written_down_claims_nothing() {
    let world = World::new();
    let print = world.open_print(7);
    world.store.fails(StoreMethod::AppendEvent, disk_full());

    let observed = block_on(world.core.observe_active_prints()).expect("the prints are read");

    assert!(observed.turns.is_empty());
    assert_eq!(
        observed.refused,
        [(print.id, printobserver_core::CoreError::Store(disk_full()))]
    );
    world.store.heals();
    assert_eq!(
        observe_and_supervise(&world),
        1,
        "the print was left claimed"
    );
    assert_eq!(observations(&world, print.id).len(), 1);
}

/// One print's observation refused stops no other: the round answers the turn
/// it claimed for the print it could write down, and leaves neither print
/// claimed once that turn has run.
#[test]
fn one_refused_observation_stops_no_other() {
    let world = World::new();
    let written = world.open_print(7);
    let refused = block_on(printobserver_core::store::PrintStore::open_print(
        world.store.as_ref(),
        Some(8),
        Some("other.gcode".to_owned()),
    ))
    .expect("a print opens");
    // An unreadable job leaves both open prints candidates.
    world.printer.fails(
        PrinterMethod::Job,
        PrinterError::Unreachable {
            detail: "the machine is switched off".to_owned(),
        },
    );
    world.store.refuses_appends_for(refused.id, disk_full());

    let observed = block_on(world.core.observe_active_prints()).expect("the prints are read");

    assert_eq!(
        observed.refused,
        [(
            refused.id,
            printobserver_core::CoreError::Store(disk_full())
        )]
    );
    assert_eq!(observed.turns.len(), 1);
    for turn in observed.turns {
        block_on(world.core.run_supervision(turn)).expect("the turn is supervised");
    }
    assert_eq!(world.agent.turns()[0].print_id, written.id);
    world.store.heals();
    assert_eq!(observe_and_supervise(&world), 2, "a print was left claimed");
}

/// Open prints the store will not read are a round that claims nothing and
/// answers the store's own error; the next round, once it has recovered,
/// observes them.
#[test]
fn a_round_whose_prints_cannot_be_read_claims_nothing() {
    let world = World::new();
    let print = world.open_print(7);
    world.store.fails(StoreMethod::OpenPrints, disk_full());

    let refused = block_on(world.core.observe_active_prints());

    assert_eq!(
        refused.err(),
        Some(printobserver_core::CoreError::Store(disk_full()))
    );
    assert!(observations(&world, print.id).is_empty());
    world.store.heals();
    assert_eq!(
        observe_and_supervise(&world),
        1,
        "the print was left claimed"
    );
}

/// Alerts handed to the print while its observation was being written, and
/// lost with that observation, are not lost with it: the newest is answered as
/// a turn of its own, carrying the earlier one as what arrived while busy.
#[test]
fn alerts_handed_over_while_an_observation_failed_are_given_their_turn() {
    let world = World::new();
    let print = world.open_print(7);
    world.store.holds_appends_of(observation_kind());
    let (alerts, observed) = std::thread::scope(|scope| {
        let observing = scope.spawn(|| block_on(world.core.observe_active_prints()));
        world.store.await_a_held_append();
        let mut alerts = Vec::new();
        for _ in 0..2 {
            let received = block_on(world.core.receive_event(failure_alert(7)))
                .expect("the alert is written down");
            assert!(received.turn.is_none(), "the alert claimed a claimed print");
            alerts.push(received.event.id);
        }
        world.store.fails(StoreMethod::AppendEvent, disk_full());
        world.store.lets_appends_through();
        let observed = observing.join().expect("the observation ends");
        world.store.heals();
        (alerts, observed.expect("the prints are read"))
    });
    assert!(observed.refused.is_empty(), "{:?}", observed.refused);
    assert_eq!(observed.turns.len(), 1);
    for turn in observed.turns {
        block_on(world.core.run_supervision(turn)).expect("the turn is supervised");
    }

    let handed = world.agent.turns();
    assert_eq!(handed.len(), 1);
    assert_eq!(handed[0].event.id, alerts[1]);
    assert_eq!(
        handed[0]
            .situation
            .arrived_while_busy
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        [alerts[0]]
    );
    assert!(observations(&world, print.id).is_empty());
}
