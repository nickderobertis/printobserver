//! A real HTTP host on the loopback address, answering what a journey says.
//!
//! The camera a look fetches a frame from and the `Obico` API a handled
//! detection is acknowledged to are both on the far side of the supervisor's
//! own HTTP client, so what they are here is a socket answering the way they
//! would — the client, its bounds and everything above it are the real ones.
//! It writes down the head of every request it was sent, which is what a
//! journey about a request the supervisor made reads back.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// What the host answers every request with.
#[derive(Debug, Clone)]
pub struct Answer {
    /// The status line's code and reason.
    pub status: &'static str,
    /// The content type it declares.
    pub content_type: &'static str,
    /// The body.
    pub body: Vec<u8>,
}

impl Answer {
    /// One still image.
    pub fn image(body: &[u8]) -> Self {
        Self {
            status: "200 OK",
            content_type: "image/jpeg",
            body: body.to_vec(),
        }
    }
}

/// What the host holds between requests.
#[derive(Debug)]
struct Held {
    /// What it answers.
    answer: Answer,
    /// The head of every request it was sent, in the order they arrived.
    received: Mutex<Vec<String>>,
    /// Set when the host is dropped, so its thread stops accepting.
    stopping: AtomicBool,
}

/// A host answering every request, for as long as it is held.
#[derive(Debug)]
pub struct Host {
    /// Where it is listening.
    address: SocketAddr,
    /// What it answers and what it was sent.
    held: Arc<Held>,
}

impl Host {
    /// Start a host answering this to every request.
    pub fn answering(answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let held = Arc::new(Held {
            answer,
            received: Mutex::new(Vec::new()),
            stopping: AtomicBool::new(false),
        });
        let serving = Arc::clone(&held);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if serving.stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    respond(stream, &serving);
                }
            }
        });
        Self { address, held }
    }

    /// The URL of one path on this host.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    /// The head of every request it was sent, in the order they arrived.
    pub fn received(&self) -> Vec<String> {
        self.held
            .received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.held.stopping.store(true, Ordering::SeqCst);
        // Wake the accepting thread so it sees the flag and stops.
        let _ = TcpStream::connect(self.address);
    }
}

/// Read one request's head and write the answer.
fn respond(mut stream: TcpStream, held: &Held) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    held.received
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(String::from_utf8_lossy(&request).into_owned());
    let answer = &held.answer;
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        answer.status,
        answer.content_type,
        answer.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&answer.body);
    let _ = stream.flush();
}
