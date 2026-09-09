//! Two real hosts on the loopback address these journeys need.
//!
//! [`ImageHost`] serves the snapshot an alert names, so that the vision adapter
//! this server composes does its real fetch rather than a mocked one.
//!
//! [`SilentHost`] answers every request with an empty JSON document. It is what
//! the configuration walk points the `OctoPrint` address at: that walk drives
//! the real composition root, which asks the machine whether the address and
//! the key are the ones it answers to, and what it needs is an address that
//! *answers* — not an `OctoPrint`. It carries no `OctoPrint` path, header or
//! response shape, because only the adapter crate may construct one.

use core::fmt::Write as _;
use core::time::Duration;
use std::net::SocketAddr;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A host serving one body to every request, for as long as it is held.
#[derive(Debug)]
pub struct Host {
    /// Where it is listening.
    address: SocketAddr,
    /// The task accepting connections, aborted when this host is dropped.
    serving: JoinHandle<()>,
}

impl Host {
    /// Start a host serving this body under this content type.
    pub async fn serving(content_type: &'static str, body: Vec<u8>, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let serving = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let body = body.clone();
                tokio::spawn(async move { respond(stream, content_type, body, delay).await });
            }
        });
        Self { address, serving }
    }

    /// Where it is answering.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// The URL of the one thing it serves.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/snapshot.jpg", self.address)
    }

    /// The base URL a caller reaches it at.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// A host serving one snapshot as an image.
pub async fn image_host(body: Vec<u8>) -> Host {
    Host::serving("image/jpeg", body, Duration::ZERO).await
}

/// A host serving one snapshot slowly, so a journey can watch handling run on.
pub async fn slow_image_host(body: Vec<u8>, delay: Duration) -> Host {
    Host::serving("image/jpeg", body, delay).await
}

/// A host that answers, and says nothing else.
pub async fn silent_host() -> Host {
    Host::serving("application/json", b"{}".to_vec(), Duration::ZERO).await
}

/// An address nothing is listening on.
///
/// A port is bound to learn one that is free and then released, so what a
/// caller meets is a refused connection rather than a served refusal.
pub async fn unreachable_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    drop(listener);
    address
}

/// Read one request and write the answer.
async fn respond(
    mut stream: TcpStream,
    content_type: &'static str,
    body: Vec<u8>,
    delay: Duration,
) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    tokio::time::sleep(delay).await;
    let mut head = String::from("HTTP/1.1 200 OK\r\n");
    let _ = write!(head, "Content-Type: {content_type}\r\n");
    let _ = write!(head, "Content-Length: {}\r\n", body.len());
    head.push_str("Connection: close\r\n\r\n");
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    if stream.write_all(&body).await.is_err() {
        return;
    }
    let _ = stream.shutdown().await;
}
