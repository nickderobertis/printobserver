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
use crate::held_events::{self, HeldEvents};
use crate::http_host::{Host, image_host};
use crate::ingress::snapshot_bytes;
use crate::printer::RecordingPrinter;
use crate::refusing_prints::RefusingPrints;
use crate::world::{SECRET, World, failure_alert, set};

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

/// How many observations one print's history holds, read from the stores
/// alone, so that it can be read after the server has gone.
async fn observations_held(stores: &printobserver_core::Stores, print_id: PrintId) -> usize {
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

/// Whether the server goes by being stopped or by its handle being dropped.
#[derive(Debug, Clone, Copy)]
enum Going {
    /// `Running::stop`, which the service's own shutdown takes.
    Stopped,
    /// The last handle dropped, with nobody stopping it.
    Dropped,
}

/// A server that has gone — stopped, or its last handle dropped with nobody
/// stopping it — observes nothing more.
#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_has_gone_observes_nothing_more() {
    for going in [Going::Stopped, Going::Dropped] {
        let camera = image_host(snapshot_bytes()).await;
        let agent = StandInAgent::new();
        let world = observing(RecordingPrinter::printing(), Arc::clone(&agent), &camera).await;
        let print_id = world.open_print().await;
        observed(&agent, print_id, 1).await;

        let World {
            server,
            stores,
            root,
            ..
        } = world;
        match going {
            Going::Stopped => server.stop().await,
            Going::Dropped => drop(server),
        }
        // An observation already being written as the server went finishes.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let gone_at = observations_held(&stores, print_id).await;
        tokio::time::sleep(SEVERAL_INTERVALS).await;
        assert_eq!(
            observations_held(&stores, print_id).await,
            gone_at,
            "a server that was {going:?} went on observing"
        );
        drop(root);
    }
}

/// The interval journeys here observe at, as a configuration edit, with the
/// camera at `camera`.
fn every(seconds: i64, camera: &Host) -> impl FnOnce(&mut toml::Value) {
    let url = camera.url();
    move |document| {
        set(
            document,
            "supervisor.observation_interval_s",
            toml::Value::Integer(seconds),
        );
        let mut table = toml::Table::new();
        table.insert("snapshot_url".to_owned(), toml::Value::String(url));
        set(document, "camera", toml::Value::Table(table));
    }
}

/// A round slower than the interval is followed by a whole interval, not by
/// rounds making up for the ones it overran: each observation is written at
/// least the camera's delay and one interval after the one before it.
#[tokio::test(flavor = "multi_thread")]
async fn a_slow_round_is_followed_by_a_whole_interval() {
    let dwell = Duration::from_millis(1_500);
    let camera = Host::dwelling(dwell, "200 OK", "image/jpeg", snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let world = World::configured(
        RecordingPrinter::printing(),
        Arc::clone(&agent),
        every(1, &camera),
    )
    .await;
    let print_id = world.open_print().await;

    let turns = observed(&agent, print_id, 3).await;

    let interval = Duration::from_secs(1);
    for pair in turns.windows(2) {
        let apart = *pair[1].event.received_at.as_utc() - *pair[0].event.received_at.as_utc();
        let apart = apart.to_std().expect("observations are written in order");
        assert!(
            apart + Duration::from_millis(100) >= dwell + interval,
            "two observations were written {apart:?} apart, under a slow round and an interval"
        );
    }
    world.server.stop().await;
}

/// A round the store refuses claims nothing, and the running server observes
/// again on a later interval once the store answers.
#[tokio::test(flavor = "multi_thread")]
async fn a_round_the_store_refused_is_followed_by_one_that_observes() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let mut refusing = None;
    let world = World::configured_over(
        RecordingPrinter::printing(),
        Arc::clone(&agent),
        every(1, &camera),
        |mut stores| {
            let prints = RefusingPrints::over(Arc::clone(&stores.prints));
            stores.prints = Arc::clone(&prints) as Arc<dyn printobserver_core::store::PrintStore>;
            refusing = Some(prints);
            stores
        },
    )
    .await;
    let refusing = refusing.expect("the stores were composed");
    let print_id = world.open_print().await;
    // Refused once the server is up, because its start reads the open prints
    // too, and a start the store refuses is a server that never came up.
    refusing.refusing(true);

    tokio::time::sleep(SEVERAL_INTERVALS).await;
    assert!(
        observation_turns(&agent, print_id).is_empty(),
        "a round the store refused observed"
    );
    refusing.refusing(false);
    observed(&agent, print_id, 1).await;
    world.server.stop().await;
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

/// A world observing every second over `printer`, with the camera at
/// `camera` when there is one, whose event log and image store refuse what a
/// journey tells them to.
async fn observing_held(
    printer: Arc<RecordingPrinter>,
    agent: Arc<StandInAgent>,
    camera: Option<&Host>,
) -> (World, Arc<HeldEvents>) {
    let url = camera.map(Host::url);
    let mut held = None;
    let world = World::configured_over(
        printer,
        agent,
        |document| {
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(INTERVAL_S),
            );
            if let Some(url) = url {
                let mut table = toml::Table::new();
                table.insert("snapshot_url".to_owned(), toml::Value::String(url));
                set(document, "camera", toml::Value::Table(table));
            }
        },
        |stores| {
            let (stores, between) = HeldEvents::over(stores);
            held = Some(between);
            stores
        },
    )
    .await;
    (world, held.expect("the stores were composed"))
}

/// The port failures one print's history records against one event.
async fn failures_against(
    world: &World,
    print_id: PrintId,
    event: printobserver_types::EventId,
) -> Vec<PortFailurePayload> {
    history(world, print_id)
        .await
        .iter()
        .filter_map(EventRecord::payload_as::<PortFailurePayload>)
        .map(|payload| payload.expect("of its own type"))
        .filter(|failure| failure.event_id == event)
        .collect()
}

/// A second print open beside the one [`World::open_print`] opens.
async fn open_another(world: &World) -> PrintId {
    world
        .stores
        .prints
        .open_print(Some(4300), Some("bracket.gcode".to_owned()))
        .await
        .expect("a print opens")
        .id
}

/// A printer that answers no read says nothing about which print stopped, so
/// every open print is observed, each observation carrying no telemetry and
/// recording both reads that failed against itself.
#[tokio::test(flavor = "multi_thread")]
async fn a_printer_nothing_can_read_leaves_every_open_print_observed() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let printer = RecordingPrinter::printing();
    let world = observing(Arc::clone(&printer), Arc::clone(&agent), &camera).await;
    let first = world.open_print().await;
    let second = open_another(&world).await;
    printer.unreadable(true);

    for print_id in [first, second] {
        let turn = observed(&agent, print_id, 1).await.remove(0);
        let payload = turn
            .event
            .payload_as::<PeriodicObservationPayload>()
            .expect("the turn is on an observation")
            .expect("of its own type");
        assert_eq!(payload.printer, None, "telemetry nobody read was carried");
        assert_eq!(payload.job, None, "a job nobody read was carried");
        let sites: Vec<PortFailureSite> = failures_against(&world, print_id, turn.event.id)
            .await
            .into_iter()
            .map(|failure| failure.site)
            .collect();
        assert!(
            sites.contains(&PortFailureSite::PrinterSnapshot)
                && sites.contains(&PortFailureSite::PrinterJob),
            "the reads that failed were not recorded against the observation: {sites:?}"
        );
    }
    world.server.stop().await;
}

/// A printer reporting a state no print carries on from is printing nothing,
/// even while its job still reports itself running: the print is left open and
/// not observed, and is observed again once the printer reports it printing.
#[tokio::test(flavor = "multi_thread")]
async fn a_printer_in_a_terminal_state_under_a_running_job_is_not_observed() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let printer = RecordingPrinter::printing();
    let world = observing(Arc::clone(&printer), Arc::clone(&agent), &camera).await;
    let print_id = world.open_print().await;
    printer.reports_connection(PrinterState::Error);

    tokio::time::sleep(SEVERAL_INTERVALS).await;
    assert!(
        observations(&world, print_id).await.is_empty(),
        "a printer in error was observed"
    );
    assert!(agent.turns().is_empty(), "{:?}", agent.turns());
    let held = world
        .stores
        .prints
        .print(print_id)
        .await
        .expect("the print reads")
        .expect("the print is held");
    assert_eq!(held.ended_at, None, "a print whose job runs was closed");

    printer.reports_connection(PrinterState::Printing);
    observed(&agent, print_id, 1).await;
    world.server.stop().await;
}

/// One print whose observation the store refuses stops no other print's, holds
/// no claim on its own print, and is observed once the store answers again.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_observation_stops_no_other_print_and_is_taken_once_the_store_answers() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let printer = RecordingPrinter::printing();
    let (world, held) =
        observing_held(Arc::clone(&printer), Arc::clone(&agent), Some(&camera)).await;
    let refused = world.open_print().await;
    let other = open_another(&world).await;
    // Unread, the printer names neither print the active one, so both are owed
    // an observation every round.
    printer.unreadable(true);
    held.refusing_observations_of(refused, true);

    observed(&agent, other, 2).await;
    assert!(
        observation_turns(&agent, refused).is_empty(),
        "a refused observation was handed to the agent"
    );
    assert!(
        observations(&world, refused).await.is_empty(),
        "a refused observation was written down"
    );

    held.refusing_observations_of(refused, false);
    observed(&agent, refused, 1).await;
    world.server.stop().await;
}

/// An alert that arrives for a print while its observation is being written
/// is handed to the claim that observation holds; when the store then refuses
/// the observation, the alert is given the turn rather than dropped.
#[tokio::test(flavor = "multi_thread")]
async fn an_alert_handed_to_a_refused_observation_is_given_its_turn() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let (world, held) = observing_held(
        RecordingPrinter::printing(),
        Arc::clone(&agent),
        Some(&camera),
    )
    .await;
    let print_id = world.open_print().await;
    let mut completions = world.server.completions();

    held.hold_the_next_observation().await;
    let accepted = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(failure_alert(4211, &camera.url()).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(accepted, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");
    assert!(
        agent.turns().is_empty(),
        "the alert took a turn of its own while the observation held the print"
    );
    held.release();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let first = loop {
        if let Some(turn) = agent
            .turns()
            .into_iter()
            .find(|turn| turn.print_id == print_id)
        {
            break turn;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the alert handed to the refused observation was never given a turn"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        first.event.kind().as_str(),
        "obico_failure_alert",
        "the first turn was not the alert's"
    );
    world.server.stop().await;
}

/// With no camera configured, an observation runs on the printer's telemetry
/// alone, and no camera's failure is recorded against it.
#[tokio::test(flavor = "multi_thread")]
async fn an_observation_with_no_camera_runs_on_telemetry_alone() {
    let agent = StandInAgent::new();
    let (world, _held) =
        observing_held(RecordingPrinter::printing(), Arc::clone(&agent), None).await;
    let print_id = world.open_print().await;

    let turn = observed(&agent, print_id, 1).await.remove(0);

    assert!(
        turn.event.image.is_none(),
        "a frame nobody took was carried"
    );
    assert!(turn.image_path.is_none());
    let payload = turn
        .event
        .payload_as::<PeriodicObservationPayload>()
        .expect("the turn is on an observation")
        .expect("of its own type");
    assert!(
        payload.printer.is_some(),
        "the printer's telemetry is carried"
    );
    assert!(
        failures_against(&world, print_id, turn.event.id)
            .await
            .is_empty(),
        "a failure was recorded against an observation with no camera"
    );
    world.server.stop().await;
}

/// A frame the image store refuses is recorded against the observation, in the
/// store's own words, and the turn still runs on the printer's telemetry.
#[tokio::test(flavor = "multi_thread")]
async fn a_frame_the_store_refuses_is_recorded_against_the_observation() {
    let camera = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    let (world, held) = observing_held(
        RecordingPrinter::printing(),
        Arc::clone(&agent),
        Some(&camera),
    )
    .await;
    held.refusing_images(true);
    let print_id = world.open_print().await;

    let turn = observed(&agent, print_id, 1).await.remove(0);

    assert!(
        turn.event.image.is_none(),
        "a frame the store refused was carried"
    );
    assert!(turn.image_path.is_none());
    let failures = failures_against(&world, print_id, turn.event.id).await;
    assert!(
        failures
            .iter()
            .any(|failure| failure.site == PortFailureSite::CameraLook
                && failure.detail.contains(held_events::DETAIL)),
        "the refused frame was not recorded against the observation: {failures:?}"
    );
    world.server.stop().await;
}
