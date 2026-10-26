//! The failure detector's own health, checked during a print, through the
//! real server.
//!
//! The detector is a stand-in for `Obico`'s `ml_api` serving its `/hc/` on a
//! real socket, which a journey moves through the four moods the observability
//! spike measured: answering `200`, answering `503`, holding the request past
//! the check's five seconds, and closed. Beside it a stand-in for `Obico`'s
//! credentialed web API answers `200` throughout — the call that stays up while
//! the detector is down, and so is no health check. What is asserted is what
//! the printer history, the status and context reads and the agent show, and
//! what the detector's stand-in received.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use printobserver_types::serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpSocket};

use crate::agent::StandInAgent;
use crate::http_host::Host;
use crate::printer::RecordingPrinter;
use crate::world::{World, set};

/// How a journey waits for what the check should find.
const PATIENCE: Duration = Duration::from_secs(30);

/// How the detector's stand-in answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mood {
    /// `200 ok`.
    Answering,
    /// `503`.
    Failing,
    /// Nothing, for longer than the check waits.
    Hanging,
}

/// One request the detector's stand-in received: its request line and its
/// headers.
type Received = (String, Vec<String>);

/// The detector's stand-in.
struct Detector {
    /// Where it answers.
    address: SocketAddr,
    /// How it answers.
    mood: Arc<Mutex<Mood>>,
    /// Every request it received.
    received: Arc<Mutex<Vec<Received>>>,
    /// Its serving task, absent while it is closed.
    serving: Option<tokio::task::JoinHandle<()>>,
}

impl Detector {
    /// A stand-in answering `200`.
    fn start() -> Self {
        let mut detector = Self {
            address: "127.0.0.1:0".parse().expect("an address"),
            mood: Arc::new(Mutex::new(Mood::Answering)),
            received: Arc::new(Mutex::new(Vec::new())),
            serving: None,
        };
        detector.open();
        detector
    }

    /// Listen on its port again, or on a fresh one the first time.
    fn open(&mut self) {
        let socket = TcpSocket::new_v4().expect("a socket");
        socket.set_reuseaddr(true).expect("the port is reusable");
        socket.bind(self.address).expect("the port binds");
        let listener: TcpListener = socket.listen(16).expect("it listens");
        self.address = listener.local_addr().expect("the bound address");
        let mood = Arc::clone(&self.mood);
        let received = Arc::clone(&self.received);
        self.serving = Some(tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mood = Arc::clone(&mood);
                let received = Arc::clone(&received);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 1024];
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => request.extend_from_slice(&buffer[..read]),
                        }
                    }
                    let text = String::from_utf8_lossy(&request).into_owned();
                    let mut lines = text.split("\r\n");
                    let line = lines.next().unwrap_or_default().to_owned();
                    let headers = lines
                        .filter(|header| !header.is_empty())
                        .map(str::to_owned)
                        .collect();
                    received
                        .lock()
                        .expect("the record holds")
                        .push((line, headers));
                    let current = *mood.lock().expect("the mood holds");
                    let answer: &[u8] = match current {
                        Mood::Answering => {
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                        }
                        Mood::Failing => {
                            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        }
                        Mood::Hanging => {
                            tokio::time::sleep(Duration::from_secs(8)).await;
                            return;
                        }
                    };
                    let _ = stream.write_all(answer).await;
                    let _ = stream.shutdown().await;
                });
            }
        }));
    }

    /// Stop listening, so a connection is refused.
    async fn close(&mut self) {
        if let Some(serving) = self.serving.take() {
            serving.abort();
            let _ = serving.await;
        }
    }

    /// Answer this way from now on.
    fn answers(&self, mood: Mood) {
        *self.mood.lock().expect("the mood holds") = mood;
    }

    /// Its address, as the configuration names it.
    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Every request it received.
    fn received(&self) -> Vec<Received> {
        self.received.lock().expect("the record holds").clone()
    }
}

impl Drop for Detector {
    fn drop(&mut self) {
        if let Some(serving) = self.serving.take() {
            serving.abort();
        }
    }
}

/// A world watching an active print, checking the detector at `ml_api_url`
/// once a second, with `Obico`'s web API answering at `web_url`.
async fn watching(
    ml_api_url: Option<String>,
    web_url: &str,
) -> (World, printobserver_types::PrintId) {
    let web_url = web_url.to_owned();
    let world = World::configured(
        RecordingPrinter::printing(),
        StandInAgent::new(),
        move |document| {
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(1),
            );
            let mut obico = toml::Table::new();
            obico.insert("url".to_owned(), toml::Value::String(web_url));
            obico.insert(
                "access_token".to_owned(),
                toml::Value::String("a-journeys-own-obico-token".to_owned()),
            );
            if let Some(url) = ml_api_url {
                obico.insert("ml_api_url".to_owned(), toml::Value::String(url));
            }
            set(document, "obico", toml::Value::Table(obico));
        },
    )
    .await;
    let print_id = world.open_print().await;
    (world, print_id)
}

/// Every `detector_health_changed` the printer history answers, oldest first.
async fn changes(world: &World) -> Vec<Value> {
    let (_, answer) = world
        .get(&world.at("/v1/history?kinds=detector_health_changed"))
        .await;
    let mut events = answer["events"].as_array().cloned().unwrap_or_default();
    events.reverse();
    events
}

/// Wait until the printer history holds `count` changes.
async fn until_changes(world: &World, count: usize) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let found = changes(world).await;
        if found.len() >= count {
            return found;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "only {} of {count} changes were recorded",
            found.len()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The detector's health, as the status read answers it.
async fn status_health(world: &World, print_id: printobserver_types::PrintId) -> Value {
    let (_, status) = world
        .get(&world.operation_url("/v1/prints/{print_id}/status", print_id))
        .await;
    status["detector"].clone()
}

/// Wait until the status read answers the detector as `status`.
async fn until_status(
    world: &World,
    print_id: printobserver_types::PrintId,
    status: &str,
) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let health = status_health(world, print_id).await;
        if health["status"] == status {
            return health;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the detector never read {status}: {health}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The detector going down — refusing, then holding its check past the bound,
/// then closed — is recorded once, with the outage's start answered by the
/// status and context reads and a turn for the agent; its coming back is
/// recorded once; and the web API answering throughout changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_detector_outage_is_recorded_once_each_way_and_read_with_its_start() {
    let web = Host::serving("application/json", br#"{"version":"1.0.0"}"#.to_vec()).await;
    let mut detector = Detector::start();
    let (world, print_id) = watching(Some(detector.url()), &web.url()).await;

    until_status(&world, print_id, "available").await;
    assert!(
        changes(&world).await.is_empty(),
        "the first finding was recorded as a change"
    );

    detector.answers(Mood::Failing);
    let down = until_changes(&world, 1).await;
    assert_eq!(down[0]["payload"]["status"], "unavailable");
    assert_eq!(down[0]["print_id"], Value::Null);
    let health = until_status(&world, print_id, "unavailable").await;
    assert!(health["since"].is_string(), "{health}");
    let (_, context) = world
        .get(&world.operation_url("/v1/prints/{print_id}/context", print_id))
        .await;
    assert_eq!(context["context"]["detector"], health);
    assert!(
        context["context"]["recent_events"]
            .as_array()
            .expect("recent events")
            .iter()
            .any(|event| event["kind"] == "detector_health_changed"),
        "the outage is not among the print's recent events: {context}"
    );
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let turn = loop {
        if let Some(turn) = world
            .agent
            .turns()
            .into_iter()
            .find(|turn| turn.event.kind().as_str() == "detector_health_changed")
        {
            break turn;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no turn on the outage"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(turn.print_id, print_id);
    let noticed = turn.situation.noticed.as_deref().unwrap_or_default();
    assert!(noticed.starts_with("detector_health_changed"), "{noticed}");

    detector.answers(Mood::Hanging);
    let before = detector.received().len();
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while detector.received().len() < before + 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the hanging check was not made"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    detector.close().await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        status_health(&world, print_id).await,
        health,
        "the outage's start moved"
    );

    detector.answers(Mood::Answering);
    detector.open();
    let back = until_changes(&world, 2).await;
    assert_eq!(back[1]["payload"]["status"], "available");
    until_status(&world, print_id, "available").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        changes(&world).await.len(),
        2,
        "{:#}",
        json!(changes(&world).await)
    );

    let received = detector.received();
    assert!(!received.is_empty());
    for (line, headers) in received {
        assert!(line.starts_with("GET /hc/ "), "{line}");
        assert!(
            headers
                .iter()
                .all(|header| !header.to_lowercase().starts_with("authorization")),
            "a credential was sent to the detector's health check: {headers:?}"
        );
    }
}

/// With no detector address configured nothing checks it: the status read
/// answers it unchecked, and no change is ever recorded.
#[tokio::test(flavor = "multi_thread")]
async fn with_no_detector_address_the_detector_reads_unchecked() {
    let web = Host::serving("application/json", br#"{"version":"1.0.0"}"#.to_vec()).await;
    let (world, print_id) = watching(None, &web.url()).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    assert_eq!(
        status_health(&world, print_id).await,
        json!({ "status": "unchecked", "since": null })
    );
    assert!(changes(&world).await.is_empty());
}
