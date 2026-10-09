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

/// Observe every active print once, as one tick of the driver does.
fn observe(world: &World) -> Vec<PendingTurn> {
    block_on(world.core.observe_active_prints()).expect("the observation is written down")
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
            world.store.fails(
                StoreMethod::PutImage,
                StoreError::Io {
                    detail: "the disk is full".to_owned(),
                },
            );
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

/// An observation the store will not write down claims no turn and answers
/// the store's own error; the print is released, so the next interval
/// observes it once the store has recovered.
#[test]
fn an_observation_that_cannot_be_written_down_claims_nothing() {
    let world = World::new();
    let print = world.open_print(7);
    world.store.fails(
        StoreMethod::AppendEvent,
        StoreError::Io {
            detail: "the disk is full".to_owned(),
        },
    );

    let refused = block_on(world.core.observe_active_prints());

    assert!(
        refused.is_err(),
        "a lost observation was answered as written"
    );
    assert!(world.agent.turns().is_empty());
    world.store.heals();
    assert_eq!(
        observe_and_supervise(&world),
        1,
        "the print was left claimed"
    );
    assert_eq!(observations(&world, print.id).len(), 1);
}

/// An alert handed to the print while its observation was being written, and
/// lost with that observation, is not lost with it: it is answered as a turn
/// of its own.
#[test]
fn an_alert_handed_over_while_an_observation_failed_is_given_its_turn() {
    let world = World::new();
    let print = world.open_print(7);
    world.store.holds_appends_of(observation_kind());
    let turns = std::thread::scope(|scope| {
        let observing = scope.spawn(|| block_on(world.core.observe_active_prints()));
        world.store.await_a_held_append();
        let received = block_on(world.core.receive_event(failure_alert(7)))
            .expect("the alert is written down");
        assert!(received.turn.is_none(), "the alert claimed a claimed print");
        world.store.fails(
            StoreMethod::AppendEvent,
            StoreError::Io {
                detail: "the disk is full".to_owned(),
            },
        );
        world.store.lets_appends_through();
        let turns = observing.join().expect("the observation ends");
        world.store.heals();
        (
            received.event.id,
            turns.expect("the alert is answered as a turn"),
        )
    });
    let (alert, claimed) = turns;
    assert_eq!(claimed.len(), 1);
    for turn in claimed {
        block_on(world.core.run_supervision(turn)).expect("the turn is supervised");
    }

    let handed = world.agent.turns();
    assert_eq!(handed.len(), 1);
    assert_eq!(handed[0].event.id, alert);
    assert!(observations(&world, print.id).is_empty());
}
