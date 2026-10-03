//! An image host these journeys control.
//!
//! A real HTTP server on the loopback address, so that the fetch under test is
//! the real one: a mocked client would be exactly the layer whose bounds these
//! journeys exist to prove. It serves one response, and each way of serving it
//! makes the fetch cross one of the bounds the adapter declares. It writes down
//! the head of every request it was sent, which is what a journey about a
//! request this adapter makes — rather than a fetch — reads back.

use core::fmt::Write as _;
use core::time::Duration;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// What the host answers with.
#[derive(Debug, Clone)]
pub struct Answer {
    /// The status line's code and reason.
    pub status: &'static str,
    /// The content type it declares, or none at all.
    pub content_type: Option<String>,
    /// The body.
    pub body: Vec<u8>,
    /// How long it waits before answering anything.
    pub delay: Duration,
    /// Whether it declares the body's length, or ends the body by closing.
    pub declare_length: bool,
}

impl Answer {
    /// An ordinary snapshot, served as an image and declaring its length.
    pub fn image(body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            content_type: Some("image/jpeg".to_owned()),
            body,
            delay: Duration::ZERO,
            declare_length: true,
        }
    }
}

/// A host serving one answer for as long as it is held.
#[derive(Debug)]
pub struct ImageHost {
    /// Where it is listening.
    address: SocketAddr,
    /// The task accepting connections, aborted when this host is dropped.
    serving: JoinHandle<()>,
    /// The head of every request it was sent, in the order they arrived.
    received: Arc<Mutex<Vec<String>>>,
}

impl ImageHost {
    /// Start a host serving this answer to every request.
    pub async fn serving(answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let received = Arc::new(Mutex::new(Vec::new()));
        let recording = Arc::clone(&received);
        let serving = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let answer = answer.clone();
                let recording = Arc::clone(&recording);
                tokio::spawn(async move { respond(stream, answer, &recording).await });
            }
        });
        Self {
            address,
            serving,
            received,
        }
    }

    /// The address it answers at, with no path.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The head of every request it was sent, in the order they arrived.
    pub fn received(&self) -> Vec<String> {
        self.received
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The URL of the snapshot this host serves.
    pub fn snapshot_url(&self) -> String {
        format!("http://{}/snapshot.jpg", self.address)
    }
}

impl Drop for ImageHost {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// An address nothing is listening on.
///
/// A port is bound to learn one that is free and then released, so what the
/// fetch meets is a refused connection rather than a served refusal — which is
/// the one failure no response a host can write will produce.
pub async fn unreachable_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    drop(listener);
    format!("http://{address}/snapshot.jpg")
}

/// Read one request and write the answer.
async fn respond(mut stream: TcpStream, answer: Answer, received: &Mutex<Vec<String>>) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    received
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(String::from_utf8_lossy(&request).into_owned());
    tokio::time::sleep(answer.delay).await;
    let mut head = format!("HTTP/1.1 {}\r\n", answer.status);
    if let Some(content_type) = &answer.content_type {
        let _ = write!(head, "Content-Type: {content_type}\r\n");
    }
    if answer.declare_length {
        let _ = write!(head, "Content-Length: {}\r\n", answer.body.len());
    }
    head.push_str("Connection: close\r\n\r\n");
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    if stream.write_all(&answer.body).await.is_err() {
        return;
    }
    let _ = stream.shutdown().await;
}
