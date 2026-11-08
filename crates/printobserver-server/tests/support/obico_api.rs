//! Real loopback hosts standing in for `Obico`'s REST API and for its
//! failure detector's `ml_api`.
//!
//! The REST host serves `GET /api/v1/printers/{id}/` from the printer answer
//! committed at `tests/fixtures/obico-printer.json` — every field the
//! check-costs spike read off a real `Obico` printer answer at revision
//! `49c0bc70`, its credential `auth_token` a sentinel — with the three fields
//! a journey moves between reads: `current_print`, `pic` and `normalized_p`.
//! It answers only a request carrying `Authorization: Bearer <access_token>`
//! and refuses any other `403`, as `Obico` does; it answers an
//! acknowledgement `200`, and anything else `404`. The `ml_api` host answers
//! its `/hc/` `200` or `503`. Both keep every request they received, with the
//! instant it arrived, so a journey can say what reached them and how often.
//!
//! In the style of the adapter's own image host: a real socket, so the client
//! under test is the server's own.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use printobserver_types::serde_json::{self, Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The printer answer the REST host serves, as committed.
pub const RECORDED_PRINTER: &str = include_str!("../fixtures/obico-printer.json");

/// The credential the recorded answer carries, which nothing the server
/// writes may carry.
pub const AUTH_TOKEN_SENTINEL: &str = "SENTINEL-obico-printer-auth-token-0b6f3c";

/// One request a host received: its method and path, its headers, and when.
#[derive(Debug, Clone)]
pub struct Request {
    /// The method and the path, as the request line names them.
    pub line: String,
    /// Its headers.
    pub headers: Vec<String>,
    /// When it arrived.
    pub at: Instant,
}

impl Request {
    /// Its path.
    pub fn path(&self) -> &str {
        self.line.split(' ').nth(1).unwrap_or_default()
    }
}

/// What one host holds between requests.
#[derive(Debug)]
struct Held {
    /// The answer it serves.
    answer: Value,
    /// The status `/hc/` answers.
    health: &'static str,
    /// Every request it received.
    received: Vec<Request>,
}

/// One loopback host.
#[derive(Debug)]
pub struct Host {
    /// Where it listens.
    pub address: std::net::SocketAddr,
    /// What it holds.
    held: Arc<Mutex<Held>>,
    /// Its accepting task.
    serving: JoinHandle<()>,
}

impl Host {
    /// `Obico`'s REST API, admitting `token`, serving the recorded printer.
    pub async fn rest(token: &str) -> Self {
        Self::start(Some(token.to_owned())).await
    }

    /// The detector's `ml_api`, answering its health check `200`.
    pub async fn ml_api() -> Self {
        Self::start(None).await
    }

    /// Start one host.
    async fn start(token: Option<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let held = Arc::new(Mutex::new(Held {
            answer: serde_json::from_str(RECORDED_PRINTER).expect("the recording is JSON"),
            health: "200 OK",
            received: Vec::new(),
        }));
        let serving_held = Arc::clone(&held);
        let serving_token = token;
        let serving = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let held = Arc::clone(&serving_held);
                let token = serving_token.clone();
                tokio::spawn(async move { respond(stream, &held, token.as_deref()).await });
            }
        });
        Self {
            address,
            held,
            serving,
        }
    }

    /// Its address, as the configuration names it.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Change the answer it serves from now on.
    pub fn answer(&self, change: impl FnOnce(&mut Value)) {
        change(&mut self.held.lock().expect("the host holds").answer);
    }

    /// A new snapshot, as `Obico` takes one from the camera, scored `score`.
    pub fn scores(&self, snapshot: u64, score: f64) {
        self.answer(|answer| {
            answer["pic"] = json!({
                "img_url": format!(
                    "http://127.0.0.1:3334/media/tsd-pics/snapshots/17/latest_unrotated.jpg?digest={snapshot}"
                ),
            });
            answer["normalized_p"] = json!(score);
        });
    }

    /// Answer `/hc/` with this status line from now on.
    pub fn health(&self, status: &'static str) {
        self.held.lock().expect("the host holds").health = status;
    }

    /// Every request it received.
    pub fn received(&self) -> Vec<Request> {
        self.held.lock().expect("the host holds").received.clone()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// Read one request, keep it, and answer it.
async fn respond(mut stream: TcpStream, held: &Mutex<Held>, token: Option<&str>) {
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
    let headers: Vec<String> = lines
        .filter(|header| !header.is_empty())
        .map(str::to_owned)
        .collect();
    let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
    let (status, body) = {
        let mut held = held.lock().expect("the host holds");
        held.received.push(Request {
            line: line.clone(),
            headers: headers.clone(),
            at: Instant::now(),
        });
        match token {
            None if path == "/hc/" => (held.health, b"ok".to_vec()),
            None => ("404 Not Found", Vec::new()),
            Some(token) => {
                let bearer = format!("authorization: bearer {token}");
                let admitted = headers
                    .iter()
                    .any(|header| header.eq_ignore_ascii_case(&bearer));
                if !admitted {
                    (
                        "403 Forbidden",
                        br#"{"detail":"Authentication credentials were not provided."}"#.to_vec(),
                    )
                } else if line.starts_with("GET ") && path.starts_with("/api/v1/printers/") {
                    let id = held.answer["id"].as_i64().unwrap_or_default();
                    if path == format!("/api/v1/printers/{id}/") {
                        ("200 OK", held.answer.to_string().into_bytes())
                    } else {
                        ("404 Not Found", br#"{"detail":"Not found."}"#.to_vec())
                    }
                } else if line.starts_with("POST ") && path.contains("/acknowledge_alert/") {
                    ("200 OK", b"{}".to_vec())
                } else {
                    ("404 Not Found", br#"{"detail":"Not found."}"#.to_vec())
                }
            }
        }
    };
    let preamble = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if stream.write_all(preamble.as_bytes()).await.is_err() {
        return;
    }
    let _ = stream.write_all(&body).await;
    let _ = stream.shutdown().await;
}
