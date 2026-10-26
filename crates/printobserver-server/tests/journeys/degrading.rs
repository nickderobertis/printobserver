//! A camera that gives nothing worth looking at, through the real look and
//! observation paths.
//!
//! The camera is a real HTTP host serving a script of frames, one per fetch,
//! and counting the fetches: a generated near-uniform grey frame
//! (`tests/fixtures/grey-frame.jpg`, 1280×720, grey level 128 with σ 4 noise,
//! written once with Pillow), a recorded frame of a real print cut part-way —
//! which a decoder that pads a short frame would pass — and the recorded frame
//! whole. What is asserted is what reached the camera, what the look answered,
//! what the history holds and what the agent was handed.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use printobserver_types::serde_json::Value;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

use crate::agent::StandInAgent;
use crate::ingress::snapshot_bytes;
use crate::printer::RecordingPrinter;
use crate::world::{World, set};

/// The generated near-uniform grey frame.
fn grey() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grey-frame.jpg");
    std::fs::read(path).expect("the grey frame reads")
}

/// The recorded frame, cut part-way.
fn truncated() -> Vec<u8> {
    let whole = snapshot_bytes();
    whole[..whole.len() / 2].to_vec()
}

/// A camera serving one frame per fetch from a script, the last one again once
/// the script runs out, and counting the fetches.
struct Camera {
    /// Where it answers.
    url: String,
    /// How many fetches reached it.
    fetched: Arc<AtomicUsize>,
    /// The serving task.
    serving: tokio::task::JoinHandle<()>,
}

impl Camera {
    /// A camera serving these frames in turn.
    async fn serving(frames: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let address = listener.local_addr().expect("the bound address");
        let fetched = Arc::new(AtomicUsize::new(0));
        let script = Arc::new(Mutex::new(frames));
        let counting = Arc::clone(&fetched);
        let serving = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let index = counting.fetch_add(1, Ordering::SeqCst);
                let body = {
                    let frames = script.lock().expect("the script holds");
                    frames[index.min(frames.len() - 1)].clone()
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            }
        });
        Self {
            url: format!("http://{address}/frame.jpeg"),
            fetched,
            serving,
        }
    }

    /// How many fetches have reached it.
    fn fetches(&self) -> usize {
        self.fetched.load(Ordering::SeqCst)
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// A world whose camera is this one, its observation interval this long.
async fn with_camera(camera: &Camera, agent: Arc<StandInAgent>, interval_s: Option<i64>) -> World {
    let url = camera.url.clone();
    World::configured(RecordingPrinter::printing(), agent, move |document| {
        let mut table = toml::Table::new();
        table.insert("snapshot_url".to_owned(), toml::Value::String(url));
        set(document, "camera", toml::Value::Table(table));
        if let Some(seconds) = interval_s {
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(seconds),
            );
        }
    })
    .await
}

/// One look at a print, answered.
async fn look(world: &World, print_id: printobserver_types::PrintId) -> Value {
    let url = format!(
        "{}?wait_s=0",
        world.operation_url("/v1/prints/{print_id}/look", print_id)
    );
    let (status, look) = world.get(&url).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{look}");
    look
}

/// Every event of one print's history, through the real route.
async fn history(world: &World, print_id: printobserver_types::PrintId) -> Vec<Value> {
    let (_, answer) = world
        .get(&world.operation_url("/v1/prints/{print_id}/history", print_id))
        .await;
    answer["events"].as_array().cloned().unwrap_or_default()
}

/// A blank frame and a cut one are each fetched again, and the good frame
/// after them is the look's.
#[tokio::test(flavor = "multi_thread")]
async fn bad_frames_are_fetched_again_and_the_good_one_is_used() {
    let camera = Camera::serving(vec![grey(), truncated(), snapshot_bytes()]).await;
    let world = with_camera(&camera, StandInAgent::new(), None).await;
    let print_id = world.open_print().await;

    let answered = look(&world, print_id).await;

    assert_eq!(camera.fetches(), 3);
    assert_eq!(answered["event"]["kind"], "camera_look");
    let path = answered["image_path"]
        .as_str()
        .expect("the look has a frame");
    assert_eq!(
        std::fs::read(path).expect("the frame reads"),
        snapshot_bytes()
    );
    assert!(
        history(&world, print_id)
            .await
            .iter()
            .all(|event| event["kind"] != "camera_degraded"),
        "a look that found a good frame recorded the camera degraded"
    );
}

/// A camera giving only blank frames is asked three times and no more, and
/// the look is recorded as the camera degraded in its place, with no frame.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_giving_only_blank_frames_degrades_the_look_after_three_fetches() {
    let camera = Camera::serving(vec![grey()]).await;
    let world = with_camera(&camera, StandInAgent::new(), None).await;
    let print_id = world.open_print().await;

    let answered = look(&world, print_id).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(
        camera.fetches(),
        3,
        "not exactly the first fetch and two more"
    );
    assert_eq!(answered["event"]["kind"], "camera_degraded");
    assert_eq!(answered["event"]["payload"]["attempts"], 3);
    assert_eq!(answered["event"]["payload"]["cause"], "blank");
    assert!(answered.get("frame").is_none(), "{answered}");
    assert!(answered.get("image_path").is_none(), "{answered}");
    let events = history(&world, print_id).await;
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "camera_degraded")
    );
    assert!(
        events.iter().all(|event| event["kind"] != "camera_look"),
        "a degraded look was also recorded as a look"
    );
}

/// A camera giving only cut frames degrades the look as undecodable.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_giving_only_cut_frames_degrades_the_look_as_undecodable() {
    let camera = Camera::serving(vec![truncated()]).await;
    let world = with_camera(&camera, StandInAgent::new(), None).await;
    let print_id = world.open_print().await;

    let answered = look(&world, print_id).await;

    assert_eq!(camera.fetches(), 3);
    assert_eq!(answered["event"]["kind"], "camera_degraded");
    assert_eq!(answered["event"]["payload"]["cause"], "undecodable");
    assert!(answered.get("image_path").is_none(), "{answered}");
}

/// A periodic observation whose camera gives only bad frames gives the
/// print's agent its turn with the camera's failure in its situation, and no
/// frame.
#[tokio::test(flavor = "multi_thread")]
async fn an_observation_with_only_bad_frames_turns_on_the_cameras_failure() {
    let camera = Camera::serving(vec![truncated(), grey()]).await;
    let agent = StandInAgent::new();
    let world = with_camera(&camera, Arc::clone(&agent), Some(1)).await;
    let print_id = world.open_print().await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let turn = loop {
        if let Some(turn) = agent
            .turns()
            .into_iter()
            .find(|turn| turn.print_id == print_id)
        {
            break turn;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no observation turn ran"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    assert_eq!(turn.event.kind().as_str(), "camera_degraded");
    assert!(
        turn.image_path.is_none(),
        "a frame was handed over: {turn:?}"
    );
    let noticed = turn.situation.noticed.as_deref().unwrap_or_default();
    assert!(noticed.starts_with("camera_degraded"), "{noticed}");
    let events = history(&world, print_id).await;
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "periodic_observation")
    );
}

/// The blank-frame bound the configuration names is the one frames are judged
/// by, through the production start: under a bound of one grey level the
/// generated grey frame is a picture, and the look keeps it.
#[tokio::test(flavor = "multi_thread")]
async fn the_configured_blank_frame_bound_is_the_one_frames_are_judged_by() {
    use printobserver_octoprint::standin::{JobScript, STANDIN_API_KEY, ScriptedOctoPrint};
    let camera = Camera::serving(vec![grey()]).await;
    let octoprint = ScriptedOctoPrint::start();
    octoprint.reports("Printing", JobScript::printing("tower.gcode", 816, 400, 60));
    let root = tempfile::TempDir::new().expect("a root");
    let mut document = crate::world::document(root.path(), &octoprint.base_url());
    set(
        &mut document,
        "octoprint.api_key",
        toml::Value::String(STANDIN_API_KEY.to_owned()),
    );
    let mut table = toml::Table::new();
    table.insert(
        "snapshot_url".to_owned(),
        toml::Value::String(camera.url.clone()),
    );
    set(&mut document, "camera", toml::Value::Table(table));
    let mut vision = toml::Table::new();
    vision.insert("blank_frame_max_stddev".to_owned(), toml::Value::Float(1.0));
    set(&mut document, "vision", toml::Value::Table(vision));
    let path = crate::world::write(root.path(), &document);
    let server = printobserver_server::Server::start(&path)
        .await
        .expect("the server starts");
    let client = crate::world::presenting(crate::world::OPERATOR);
    let listed: Value = client
        .get(format!("{}/prints", server.base_url()))
        .send()
        .await
        .expect("the server answers")
        .json()
        .await
        .expect("a listing");
    let print_id = listed["active"]
        .as_str()
        .expect("the running job is adopted")
        .to_owned();

    let answered: Value = client
        .get(format!(
            "{}/prints/{print_id}/look?wait_s=0",
            server.base_url()
        ))
        .send()
        .await
        .expect("the server answers")
        .json()
        .await
        .expect("a look");

    assert_eq!(camera.fetches(), 1);
    assert_eq!(answered["event"]["kind"], "camera_look");
    let path = answered["image_path"]
        .as_str()
        .expect("the look kept the frame");
    assert_eq!(std::fs::read(path).expect("the frame reads"), grey());
    server.stop().await;
}
