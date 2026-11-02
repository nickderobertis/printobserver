//! A camera on the loopback address that answers each request with the next
//! answer of a script, and records when each request arrived and was answered.
//!
//! The scheduled check's retries are a claim about the requests a real camera
//! receives and when, so what a journey reads is this host's own record of the
//! exchanges — the vision adapter, its HTTP client and the supervisor's waits
//! between attempts are the real ones.

use core::fmt::Write as _;
use core::time::Duration;
use std::net::SocketAddr;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// One answer a scripted host gives one request, after holding it this long.
#[derive(Debug, Clone)]
pub struct Scripted {
    /// How long the request is held before it is answered; `None` holds it
    /// until the connection is dropped, answering nothing.
    pub hold: Option<Duration>,
    /// The status line it is answered with.
    pub status: &'static str,
    /// The content type it is answered under.
    pub content_type: &'static str,
    /// The body it is answered with.
    pub body: Vec<u8>,
}

impl Scripted {
    /// A frame, answered at once.
    #[must_use]
    pub const fn frame(body: Vec<u8>) -> Self {
        Self {
            hold: Some(Duration::ZERO),
            status: "200 OK",
            content_type: "image/jpeg",
            body,
        }
    }

    /// A refusal under one status, saying `said`, answered at once.
    #[must_use]
    pub fn refusing(status: &'static str, said: &str) -> Self {
        Self {
            hold: Some(Duration::ZERO),
            status,
            content_type: "text/plain",
            body: said.as_bytes().to_vec(),
        }
    }

    /// go2rtc's own answer when it could not take a frame.
    #[must_use]
    pub fn exit_status_69() -> Self {
        Self::refusing("500 Internal Server Error", GO2RTC_FAILURE)
    }

    /// A request taken and never answered.
    #[must_use]
    pub const fn silent() -> Self {
        Self {
            hold: None,
            status: "200 OK",
            content_type: "image/jpeg",
            body: Vec::new(),
        }
    }

    /// The same answer, held this long first.
    #[must_use]
    pub const fn held(mut self, hold: Duration) -> Self {
        self.hold = Some(hold);
        self
    }
}

/// What go2rtc answers a frame it could not take with, under `500`.
pub const GO2RTC_FAILURE: &str = "exit status 69";

/// One request a scripted host received: when it arrived whole, and when its
/// answer was written, if it was.
#[derive(Debug, Clone, Copy)]
pub struct Exchange {
    /// When the request's head had arrived.
    pub received: std::time::Instant,
    /// When the answer had been written, or `None` when it never was.
    pub answered: Option<std::time::Instant>,
}

/// A host answering each request with the next answer of a script, in the
/// order the requests arrive — the last answer for every request after the
/// script runs out — and recording when each arrived and was answered.
#[derive(Debug)]
pub struct ScriptedHost {
    /// Where it is listening.
    pub address: SocketAddr,
    /// Every exchange, in the order the requests arrived.
    exchanges: std::sync::Arc<std::sync::Mutex<Vec<Exchange>>>,
    /// The task accepting connections, aborted when this host is dropped.
    serving: JoinHandle<()>,
}

impl ScriptedHost {
    /// Start a host answering `script`, in order.
    ///
    /// # Panics
    ///
    /// Panics when the script is empty: a host with nothing to answer is not
    /// one any journey means.
    pub async fn answering(script: Vec<Scripted>) -> Self {
        assert!(!script.is_empty(), "a script answers something");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let exchanges: std::sync::Arc<std::sync::Mutex<Vec<Exchange>>> = std::sync::Arc::default();
        let recorded = std::sync::Arc::clone(&exchanges);
        let serving = tokio::spawn(async move {
            let mut next = 0_usize;
            while let Ok((stream, _)) = listener.accept().await {
                let answer = script[next.min(script.len() - 1)].clone();
                next += 1;
                let recorded = std::sync::Arc::clone(&recorded);
                tokio::spawn(async move {
                    exchange(stream, answer, &recorded).await;
                });
            }
        });
        Self {
            address,
            exchanges,
            serving,
        }
    }

    /// The URL of the one thing it serves.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/api/frame.jpeg?src=camera", self.address)
    }

    /// Every exchange so far, in the order the requests arrived.
    #[must_use]
    pub fn exchanges(&self) -> Vec<Exchange> {
        self.exchanges
            .lock()
            .expect("the record is not poisoned")
            .clone()
    }
}

impl Drop for ScriptedHost {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// Read one request, record it, hold it, answer it, and record the answer.
async fn exchange(
    mut stream: TcpStream,
    answer: Scripted,
    recorded: &std::sync::Mutex<Vec<Exchange>>,
) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let index = {
        let mut held = recorded.lock().expect("the record is not poisoned");
        held.push(Exchange {
            received: std::time::Instant::now(),
            answered: None,
        });
        held.len() - 1
    };
    let Some(hold) = answer.hold else {
        // Held until the client gives up and drops the connection.
        let _ = stream.read(&mut buffer).await;
        return;
    };
    tokio::time::sleep(hold).await;
    let mut head = format!("HTTP/1.1 {}\r\n", answer.status);
    let _ = write!(head, "Content-Type: {}\r\n", answer.content_type);
    let _ = write!(head, "Content-Length: {}\r\n", answer.body.len());
    head.push_str("Connection: close\r\n\r\n");
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    if stream.write_all(&answer.body).await.is_err() {
        return;
    }
    let _ = stream.flush().await;
    recorded.lock().expect("the record is not poisoned")[index].answered =
        Some(std::time::Instant::now());
    let _ = stream.shutdown().await;
}

/// The URL of a camera nothing listens for: a loopback port bound and let go,
/// so a connection to it is refused.
pub async fn refusing_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    drop(listener);
    format!("http://{address}/api/frame.jpeg?src=camera")
}
