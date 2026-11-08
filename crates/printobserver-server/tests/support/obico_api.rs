//! Real loopback hosts standing in for `Obico`'s REST API and for its
//! failure detector's `ml_api`.
//!
//! The REST host serves `GET /api/v1/printers/{id}/` from [`RECORDED_PRINTER`]
//! — every field the check-costs spike read off a real `Obico` printer answer
//! at revision `49c0bc70`, its credential `auth_token` a sentinel — with the
//! three fields a journey moves between reads: `current_print`, `pic` and
//! `normalized_p`.
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

/// The printer answer the REST host serves: every field the check-costs spike
/// read off a real `Obico` printer answer at revision `49c0bc70`, its
/// credential `auth_token` a sentinel.
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
pub const RECORDED_PRINTER: &str = r#"{
  "id": 17,
  "name": "Prusa MK4",
  "created_at": "2026-02-27T09:14:02.118344Z",
  "archived_at": null,
  "action_on_failure": "PAUSE",
  "tools_off_on_pause": true,
  "bed_off_on_pause": false,
  "retract_on_pause": 6.5,
  "lift_z_on_pause": 2.5,
  "detective_sensitivity": 1.0,
  "watching_enabled": true,
  "not_watching_reason": "",
  "min_timelapse_secs_on_finish": 600,
  "min_timelapse_secs_on_cancel": 300,
  "auth_token": "SENTINEL-obico-printer-auth-token-0b6f3c",
  "agent_name": "octoprint_obico",
  "agent_version": "2.5.2",
  "settings": {
    "webcam_flipV": false,
    "webcam_flipH": false,
    "webcam_rotation": 0,
    "ratio169": false
  },
  "status": {
    "state": {
      "text": "Printing",
      "flags": {
        "operational": true,
        "printing": true,
        "paused": false,
        "cancelling": false,
        "error": false
      }
    },
    "job": {
      "file": {
        "name": "benchy.gcode",
        "display": "benchy.gcode"
      }
    },
    "progress": {
      "completion": 12.5,
      "printTime": 900,
      "printTimeLeft": 6300
    },
    "_ts": 1772367300
  },
  "pic": {
    "img_url": "http://127.0.0.1:3334/media/tsd-pics/snapshots/17/latest_unrotated.jpg?digest=1772367300"
  },
  "normalized_p": 0.0,
  "current_print": {
    "id": 4211,
    "printer": 17,
    "filename": "benchy.gcode",
    "started_at": "2026-03-01T12:00:00.500000Z",
    "finished_at": null,
    "cancelled_at": null,
    "ended_at": null,
    "uploaded_at": null,
    "alerted_at": null,
    "alert_acknowledged_at": null,
    "alert_muted_at": null,
    "paused_at": null,
    "alert_overwrite": null,
    "access_consented_at": null,
    "video_archived_at": null,
    "video_url": null,
    "tagged_video_url": null,
    "poster_url": null,
    "print_time": 900,
    "filament_used": null,
    "need_alert_overwrite": false,
    "need_print_shot_feedback": false,
    "g_code_file": null
  }
}"#;

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

/// How the REST host answers a printer read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// With the record.
    Answering,
    /// With nothing, for longer than the server's read waits.
    Hanging,
    /// With the start of an answer, and then nothing more.
    BreakingOff,
}

/// What one host holds between requests.
#[derive(Debug)]
struct Held {
    /// How it answers a printer read.
    mood: Mood,
    /// Whether it answers its one record at every printer's path, as a
    /// misrouted host would.
    any_printer: bool,
    /// The answer it serves.
    answer: Value,
    /// The status `/hc/` answers.
    health: &'static str,
    /// Every request it received.
    received: Vec<Request>,
}

/// One loopback host, which answers a printer read as its [`Mood`] says.
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
            mood: Mood::Answering,
            any_printer: false,
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

    /// The answer it serves now.
    pub fn served(&self) -> Value {
        self.held.lock().expect("the host holds").answer.clone()
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

    /// Answer a printer read this way from now on.
    pub fn answers(&self, mood: Mood) {
        self.held.lock().expect("the host holds").mood = mood;
    }

    /// Answer its one record at every printer's path from now on.
    pub fn answers_every_printer(&self) {
        self.held.lock().expect("the host holds").any_printer = true;
    }

    /// Stop listening, so a connection is refused.
    pub fn stop(&self) {
        self.serving.abort();
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
    let (status, body, mood) = {
        let mut held = held.lock().expect("the host holds");
        let mood = held.mood;
        held.received.push(Request {
            line: line.clone(),
            headers: headers.clone(),
            at: Instant::now(),
        });
        let (status, body) = match token {
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
                    if held.any_printer || path == format!("/api/v1/printers/{id}/") {
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
        };
        (status, body, mood)
    };
    let reading = line.starts_with("GET ") && path.starts_with("/api/v1/printers/");
    if reading && status.starts_with("200") {
        match mood {
            Mood::Answering => {}
            Mood::Hanging => {
                tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                return;
            }
            Mood::BreakingOff => {
                let cut = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(cut.as_bytes()).await;
                let _ = stream.write_all(&body[..body.len() / 2]).await;
                let _ = stream.shutdown().await;
                return;
            }
        }
    }
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
