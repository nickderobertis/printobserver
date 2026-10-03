//! A fresh look at a print, through the real route.
//!
//! What the command-line tier proves of a look end to end — a frame, no camera,
//! a refused wait, an early return — this tier proves at the route beside the
//! two things only the server can show: a camera that fails is recorded
//! against the look that asked it, and a look that is waiting holds none of the
//! workers the rest of the API answers on.

use core::time::Duration;
use std::time::Instant;

use printobserver_core::store::HistoryQuery;
use printobserver_core::{CameraLookPayload, PortFailurePayload, PortFailureSite};
use printobserver_types::serde_json::Value;

use crate::agent::StandInAgent;
use crate::http_host::{Host, image_host};
use crate::ingress::{post, snapshot_bytes};
use crate::printer::RecordingPrinter;
use crate::world::{SECRET, World, failure_alert, set};

/// A world whose camera answers at `url`.
async fn with_camera(agent: std::sync::Arc<StandInAgent>, url: String) -> World {
    World::configured(RecordingPrinter::printing(), agent, |document| {
        let mut camera = toml::Table::new();
        camera.insert("snapshot_url".to_owned(), toml::Value::String(url));
        set(document, "camera", toml::Value::Table(camera));
    })
    .await
}

/// The look route for one print, waiting this long.
fn look_url(world: &World, print_id: printobserver_types::PrintId, wait_s: u32) -> String {
    format!(
        "{}?wait_s={wait_s}",
        world.operation_url("/v1/prints/{print_id}/look", print_id)
    )
}

/// A camera that will not give a frame is recorded as a port failure at the
/// camera against the look, which is already in the history; the look answers
/// with no frame rather than failing.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_that_gives_no_frame_is_recorded_against_the_look() {
    for camera in [
        Host::answering("500 Internal Server Error", "text/plain", b"no".to_vec()).await,
        Host::serving("text/html", b"<html>a login page</html>".to_vec()).await,
    ] {
        let world = with_camera(StandInAgent::new(), camera.url()).await;
        let print_id = world.open_print().await;

        let (status, look) = world.get(&look_url(&world, print_id, 0)).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{look}");
        assert!(look.get("frame").is_none(), "{look}");
        assert!(look.get("image_path").is_none(), "{look}");
        let look_id = look["event"]["id"]
            .as_str()
            .expect("the look is an event")
            .to_owned();

        let history = world
            .stores
            .events
            .history(HistoryQuery {
                print_id,
                kinds: Vec::new(),
                since: None,
                until: None,
                limit: Some(10),
            })
            .await
            .expect("the history reads");
        let failure = history
            .iter()
            .find_map(printobserver_types::EventRecord::payload_as::<PortFailurePayload>)
            .expect("the camera's failure is recorded")
            .expect("of its own type");
        assert_eq!(failure.site, PortFailureSite::CameraLook);
        assert_eq!(failure.event_id.to_string(), look_id);
        assert!(
            history.iter().any(|event| event.id.to_string() == look_id
                && event.payload_as::<CameraLookPayload>().is_some()),
            "the look is not in the history it was recorded against"
        );
        world.server.stop().await;
    }
}

/// A look that is waiting holds no worker the API answers on: while several
/// wait at once, a read is answered at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waiting_look_holds_no_worker_the_api_answers_on() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let url = look_url(&world, print_id, 3);
    let looks: Vec<_> = (0..6)
        .map(|_| {
            let client = world.client.clone();
            let url = url.clone();
            tokio::spawn(async move { client.get(url).send().await.map(|answer| answer.status()) })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = Instant::now();
    let (status, _) = world
        .get(&world.operation_url("/v1/prints/{print_id}/status", print_id))
        .await;
    let answered = started.elapsed();
    assert_eq!(status, reqwest::StatusCode::OK);
    assert!(
        answered < Duration::from_secs(2),
        "a status read waited {answered:?} behind looks that were waiting"
    );
    for look in looks {
        let status = look
            .await
            .expect("the look ends")
            .expect("the look answers");
        assert_eq!(status, reqwest::StatusCode::OK);
    }
    world.server.stop().await;
}

/// A look inside a running turn returns the moment an event for its print
/// arrives, carrying it, and the look's own record names what it delivered.
#[tokio::test(flavor = "multi_thread")]
async fn a_look_returns_early_with_what_arrived() {
    let host = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    agent.taking(Duration::from_secs(20));
    let world = with_camera(agent, host.url()).await;
    let alert = failure_alert(4211, &host.url()).to_string();

    assert_eq!(
        post(&world, &alert, Some(SECRET)).await,
        reqwest::StatusCode::ACCEPTED
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while world.agent.turns().is_empty() {
        assert!(Instant::now() < deadline, "no turn started");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let print_id = world.agent.turns()[0].print_id;

    let started = Instant::now();
    let looking = {
        let client = world.client.clone();
        let url = look_url(&world, print_id, 60);
        tokio::spawn(async move {
            client
                .get(url)
                .send()
                .await
                .expect("the look answers")
                .json::<Value>()
                .await
                .expect("the look is JSON")
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        post(&world, &alert, Some(SECRET)).await,
        reqwest::StatusCode::ACCEPTED
    );
    let look = looking.await.expect("the look ends");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the look waited out its whole wait: {:?}",
        started.elapsed()
    );
    let arrived = look["arrived"]
        .as_array()
        .expect("the look carries what arrived");
    assert_eq!(arrived.len(), 1, "{look}");
    assert_eq!(arrived[0]["kind"], "obico_failure_alert");
    assert_eq!(
        look["event"]["payload"]["delivered"][0], arrived[0]["id"],
        "the look's record does not name what it delivered: {look}"
    );
    assert!(look["frame"].is_object(), "{look}");
    assert_eq!(
        world.agent.turns().len(),
        1,
        "the arrival started a second turn"
    );
    world.server.stop().await;
}

/// A wait longer than the supervisor allows is refused rather than shortened,
/// and a print nothing is held under is refused before any wait.
#[tokio::test(flavor = "multi_thread")]
async fn a_wait_too_long_or_a_print_unknown_is_refused_at_once() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let started = Instant::now();
    let (status, refused) = world.get(&look_url(&world, print_id, 91)).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(refused.to_string().contains("90"), "{refused}");
    let (status, _) = world
        .get(&look_url(&world, printobserver_types::PrintId::new(), 60))
        .await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert!(started.elapsed() < Duration::from_secs(5));
    world.server.stop().await;
}

/// A printer that cannot be read leaves the look's printer and job absent, and
/// the look is still taken, framed and written down.
#[tokio::test(flavor = "multi_thread")]
async fn a_printer_that_cannot_be_read_leaves_the_looks_reads_absent() {
    let host = image_host(snapshot_bytes()).await;
    let world = with_camera(StandInAgent::new(), host.url()).await;
    let print_id = world.open_print().await;
    world.printer.unreadable(true);

    let (status, look) = world.get(&look_url(&world, print_id, 0)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{look}");
    assert!(look.get("printer").is_none(), "{look}");
    assert!(look.get("job").is_none(), "{look}");
    assert_eq!(look["detector_paused"], false);
    assert!(look["frame"].is_object(), "{look}");
    assert_eq!(look["event"]["kind"], "camera_look");
    world.server.stop().await;
}

/// A frame the store will not keep is recorded as a port failure at the camera
/// against the look, and the look answers with no frame.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_frame_the_store_will_not_keep_is_recorded_against_the_look() {
    use std::os::unix::fs::PermissionsExt as _;

    let host = image_host(snapshot_bytes()).await;
    let world = with_camera(StandInAgent::new(), host.url()).await;
    let print_id = world.open_print().await;
    let images = world.state_dir().join("images");
    std::fs::create_dir_all(&images).expect("the images directory");
    std::fs::set_permissions(&images, std::fs::Permissions::from_mode(0o500))
        .expect("the images directory can be made read-only");

    let (status, look) = world.get(&look_url(&world, print_id, 0)).await;
    std::fs::set_permissions(&images, std::fs::Permissions::from_mode(0o700))
        .expect("the images directory can be made writable again");
    assert_eq!(status, reqwest::StatusCode::OK, "{look}");
    assert!(look.get("frame").is_none(), "{look}");
    assert!(look.get("image_path").is_none(), "{look}");
    let history = world
        .stores
        .events
        .history(HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: Some(10),
        })
        .await
        .expect("the history reads");
    let failure = history
        .iter()
        .find_map(printobserver_types::EventRecord::payload_as::<PortFailurePayload>)
        .expect("the store's refusal is recorded")
        .expect("of its own type");
    assert_eq!(failure.site, PortFailureSite::CameraLook);
    assert_eq!(
        failure.event_id.to_string(),
        look["event"]["id"].as_str().expect("the look's own event")
    );
    world.server.stop().await;
}
