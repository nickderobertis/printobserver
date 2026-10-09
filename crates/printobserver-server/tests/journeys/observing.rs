//! The periodic observation, through the real server.
//!
//! Every journey here starts the real composition root with the interval
//! shortened to one second, a machine part-way through a print, a camera
//! serving a frame, and no `Obico` alert at all. What it reads is what the
//! supervising agent was handed — the turn request the server ran — and what
//! the history recorded.

use core::time::Duration;
use std::sync::Arc;

use printobserver_core::store::HistoryQuery;
use printobserver_core::{PeriodicObservationPayload, PortFailurePayload, PortFailureSite};
use printobserver_printer_api::PrinterState;
use printobserver_supervisor_api::TurnRequest;
use printobserver_types::{EventRecord, PrintId, Timestamp};

use crate::agent::StandInAgent;
use crate::http_host::{Host, image_host};
use crate::ingress::snapshot_bytes;
use crate::printer::RecordingPrinter;
use crate::world::{World, set};

/// The interval every journey here observes at, in seconds.
const INTERVAL_S: i64 = 1;

/// Long enough for several observations at [`INTERVAL_S`].
const SEVERAL_INTERVALS: Duration = Duration::from_millis(3_500);

/// A world observing every second, whose camera answers at `camera`.
async fn observing(
    printer: Arc<RecordingPrinter>,
    agent: Arc<StandInAgent>,
    camera: &Host,
) -> World {
    let url = camera.url();
    World::configured(printer, agent, |document| {
        set(
            document,
            "supervisor.observation_interval_s",
            toml::Value::Integer(INTERVAL_S),
        );
        let mut table = toml::Table::new();
        table.insert("snapshot_url".to_owned(), toml::Value::String(url));
        set(document, "camera", toml::Value::Table(table));
    })
    .await
}

/// The turns the agent was handed on a periodic observation of one print.
fn observation_turns(agent: &StandInAgent, print_id: PrintId) -> Vec<TurnRequest> {
    agent
        .turns()
        .into_iter()
        .filter(|turn| {
            turn.print_id == print_id
                && turn
                    .event
                    .payload_as::<PeriodicObservationPayload>()
                    .is_some()
        })
        .collect()
}

/// Wait until the agent has been handed `count` observation turns of one
/// print, and answer them.
async fn observed(agent: &StandInAgent, print_id: PrintId, count: usize) -> Vec<TurnRequest> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let turns = observation_turns(agent, print_id);
        if turns.len() >= count {
            return turns;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "only {} of {count} observation turns were run",
            turns.len()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every event one print's history holds, newest first.
async fn history(world: &World, print_id: PrintId) -> Vec<EventRecord> {
    world
        .stores
        .events
        .history(HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: Some(500),
        })
        .await
        .expect("the history reads")
}

/// The periodic observations one print's history holds.
async fn observations(world: &World, print_id: PrintId) -> Vec<EventRecord> {
    history(world, print_id)
        .await
        .into_iter()
        .filter(|event| event.payload_as::<PeriodicObservationPayload>().is_some())
        .collect()
}

/// An active print no detector alerted on is given a periodic observation
/// carrying the printer's and the job's telemetry, with the camera's frame as
/// its image, and its agent is handed a turn on it; a print that has ended is
/// given none.
#[tokio::test(flavor = "multi_thread")]
async fn an_active_print_is_observed_and_an_ended_one_is_not() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let world = observing(RecordingPrinter::printing(), Arc::clone(&agent), &camera).await;
    let ended = world
        .stores
        .prints
        .open_print(Some(4100), Some("earlier.gcode".to_owned()))
        .await
        .expect("a print opens")
        .id;
    world
        .stores
        .prints
        .end_print(
            ended,
            PrinterState::Operational,
            Timestamp::now(),
            "it finished".to_owned(),
        )
        .await
        .expect("the print ends");
    let active = world.open_print().await;

    let turn = observed(&agent, active, 1).await.remove(0);

    let payload = turn
        .event
        .payload_as::<PeriodicObservationPayload>()
        .expect("the turn is on an observation")
        .expect("of its own type");
    assert_eq!(payload.interval_s, 1);
    let printer = payload.printer.expect("the printer's telemetry is carried");
    assert_eq!(printer.connection, PrinterState::Printing);
    assert_eq!(printer.tools[0].target_c.map(|c| c.value()), Some(215.0));
    let job = payload.job.expect("the job's telemetry is carried");
    assert_eq!(job.file_name.as_deref(), Some("benchy.gcode"));
    assert_eq!(job.completion.map(|c| c.value()), Some(0.25));
    assert_eq!(turn.event.print_id, Some(active));
    assert!(
        turn.event.image.is_some(),
        "the observation carries no frame"
    );
    let frame = turn
        .image_path
        .expect("the turn is handed the frame's path");
    assert_eq!(
        std::fs::read(&frame).expect("the frame is on disk"),
        snapshot_bytes(),
        "the frame handed over is not the camera's"
    );
    assert_eq!(
        turn.situation.detector_warned, None,
        "an observation is not a detection"
    );

    tokio::time::sleep(SEVERAL_INTERVALS).await;
    assert!(
        observations(&world, ended).await.is_empty(),
        "an ended print was observed"
    );
    assert!(
        agent.turns().iter().all(|turn| turn.print_id != ended),
        "an ended print's agent was handed a turn"
    );
    world.server.stop().await;
}

/// A printer that has finished its job is printing nothing: the open print is
/// closed by the read the observation makes, and nothing is observed.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_whose_printer_has_finished_is_closed_and_not_observed() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let printer = RecordingPrinter::printing();
    printer.in_state(PrinterState::Operational);
    let world = observing(Arc::clone(&printer), Arc::clone(&agent), &camera).await;
    let print_id = world.open_print().await;

    tokio::time::sleep(SEVERAL_INTERVALS).await;

    assert!(observations(&world, print_id).await.is_empty());
    assert!(agent.turns().is_empty(), "{:?}", agent.turns());
    let held = world
        .stores
        .prints
        .print(print_id)
        .await
        .expect("the print reads")
        .expect("the print is held");
    assert!(held.ended_at.is_some(), "the finished print was left open");
    world.server.stop().await;
}

/// While a print's turn is running nothing further is observed for it and
/// nothing is left waiting for it; once the turn returns, the next interval
/// observes it again.
#[tokio::test(flavor = "multi_thread")]
async fn no_observation_piles_up_behind_a_running_turn() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    agent.hold_turns();
    let world = observing(RecordingPrinter::printing(), Arc::clone(&agent), &camera).await;
    let print_id = world.open_print().await;

    observed(&agent, print_id, 1).await;
    tokio::time::sleep(SEVERAL_INTERVALS).await;

    assert_eq!(
        observations(&world, print_id).await.len(),
        1,
        "an observation was written while the print's turn was running"
    );
    assert_eq!(observation_turns(&agent, print_id).len(), 1);
    assert!(
        world
            .server
            .supervisor()
            .await_arrivals(print_id, Duration::ZERO)
            .is_empty(),
        "an observation was left waiting for the running turn"
    );

    agent.release_turns();
    observed(&agent, print_id, 2).await;
    world.server.stop().await;
}

/// The first observation comes one interval after the server starts, not at
/// the start: a restart is not itself a reason to look.
#[tokio::test(flavor = "multi_thread")]
async fn the_first_observation_waits_one_interval() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let world = World::configured(
        RecordingPrinter::printing(),
        Arc::clone(&agent),
        |document| {
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(3),
            );
            let mut table = toml::Table::new();
            table.insert("snapshot_url".to_owned(), toml::Value::String(camera.url()));
            set(document, "camera", toml::Value::Table(table));
        },
    )
    .await;
    let print_id = world.open_print().await;

    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert!(
        observation_turns(&agent, print_id).is_empty(),
        "an observation was taken before one interval had passed"
    );
    observed(&agent, print_id, 1).await;
    world.server.stop().await;
}

/// A server that has stopped observes nothing more.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_server_observes_nothing_more() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let world = observing(RecordingPrinter::printing(), Arc::clone(&agent), &camera).await;
    let print_id = world.open_print().await;
    observed(&agent, print_id, 1).await;

    let World { server, stores, .. } = world;
    server.stop().await;
    // An observation already being written as the server stopped finishes.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let count = |stores: &printobserver_core::Stores| {
        let stores = stores.clone();
        async move {
            stores
                .events
                .history(HistoryQuery {
                    print_id,
                    kinds: Vec::new(),
                    since: None,
                    until: None,
                    limit: Some(500),
                })
                .await
                .expect("the history reads")
                .iter()
                .filter(|event| event.payload_as::<PeriodicObservationPayload>().is_some())
                .count()
        }
    };
    let stopped_at = count(&stores).await;
    tokio::time::sleep(SEVERAL_INTERVALS).await;
    assert_eq!(
        count(&stores).await,
        stopped_at,
        "a stopped server went on observing"
    );
}

/// A camera that gives no frame is recorded against the observation, and the
/// turn still runs on the printer's telemetry.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_that_gives_no_frame_is_recorded_against_the_observation() {
    let camera = Host::answering("500 Internal Server Error", "text/plain", b"no".to_vec()).await;
    let agent = StandInAgent::new();
    let world = observing(RecordingPrinter::printing(), Arc::clone(&agent), &camera).await;
    let print_id = world.open_print().await;

    let turn = observed(&agent, print_id, 1).await.remove(0);

    assert!(turn.event.image.is_none());
    assert!(turn.image_path.is_none());
    let failure = history(&world, print_id)
        .await
        .iter()
        .find_map(EventRecord::payload_as::<PortFailurePayload>)
        .expect("the camera's failure is recorded")
        .expect("of its own type");
    assert_eq!(failure.site, PortFailureSite::CameraLook);
    assert_eq!(failure.event_id, turn.event.id);
    world.server.stop().await;
}

/// Left out of the configuration, the interval is two minutes; configured, it
/// is what the configuration says, and it is what the supervisor runs under.
#[tokio::test(flavor = "multi_thread")]
async fn the_interval_defaults_to_two_minutes_and_a_configured_one_is_honoured() {
    let defaulted = World::open().await;
    assert_eq!(
        defaulted.server.config().observation_interval,
        Duration::from_secs(120)
    );
    assert_eq!(
        defaulted.server.supervisor().config().observation_interval,
        Duration::from_secs(120)
    );
    defaulted.server.stop().await;

    let configured = World::configured(RecordingPrinter::printing(), StandInAgent::new(), |doc| {
        set(
            doc,
            "supervisor.observation_interval_s",
            toml::Value::Integer(7),
        );
    })
    .await;
    assert_eq!(
        configured.server.config().observation_interval,
        Duration::from_secs(7)
    );
    assert_eq!(
        configured.server.supervisor().config().observation_interval,
        Duration::from_secs(7)
    );
    configured.server.stop().await;
}
