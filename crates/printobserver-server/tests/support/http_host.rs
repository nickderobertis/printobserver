//! A real HTTP host on the loopback address, serving one answer.
//!
//! The snapshot an alert names is fetched by the `Obico` adapter this server
//! composes, so what it fetches from has to be a real host: a mocked client
//! would be exactly the layer these journeys exist to drive.

use core::fmt::Write as _;
use std::net::SocketAddr;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A host serving one body to every request, for as long as it is held.
#[derive(Debug)]
pub struct Host {
    /// Where it is listening.
    pub address: SocketAddr,
    /// The task accepting connections, aborted when this host is dropped.
    serving: JoinHandle<()>,
}

impl Host {
    /// Start a host serving this body under this content type.
    pub async fn serving(content_type: &'static str, body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let serving = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let body = body.clone();
                tokio::spawn(async move { respond(stream, content_type, body).await });
            }
        });
        Self { address, serving }
    }

    /// The URL of the one thing it serves.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/snapshot.jpg", self.address)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// A host serving one snapshot as an image.
pub async fn image_host(body: Vec<u8>) -> Host {
    Host::serving("image/jpeg", body).await
}

/// Read one request and write the answer.
async fn respond(mut stream: TcpStream, content_type: &'static str, body: Vec<u8>) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
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
