//! A real HTTP server on a real socket, for the journeys that need no printer.
//!
//! Nothing here stands in for the layer under test. The adapter's own HTTP
//! client opens a connection, writes the bytes it would write to `OctoPrint`,
//! and reads back the bytes a server wrote — the boundary being driven is the
//! socket, and both sides of it are real. What this server replaces is
//! `OctoPrint`'s behaviour, so that a journey can put the far side into a state
//! a healthy `OctoPrint` will not enter on demand: a 403, a body that is not
//! JSON, a connection that is refused.

use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::received::{Recorded, read_request};

/// What a server answers one request with.
#[derive(Debug, Clone)]
pub enum Reply {
    /// A status and no body, which is what every acting endpoint answers.
    Empty(u16),
    /// A status and a body, framed by its length.
    Body(u16, String),
    /// Bytes written to the socket exactly as given, so that a journey can put
    /// something on the wire that is not a response at all.
    Raw(Vec<u8>),
}

/// A server one journey drives the adapter against.
pub struct Stub {
    /// Where it answers.
    address: SocketAddr,
    /// Every request it received.
    recorded: Arc<Mutex<Vec<Recorded>>>,
    /// Set when the server should stop accepting.
    stopped: Arc<AtomicBool>,
    /// The accept loop.
    thread: Option<JoinHandle<()>>,
}

impl Stub {
    /// Start a server answering every request through one function.
    ///
    /// # Panics
    ///
    /// Panics when no loopback socket can be bound, which is a broken host.
    pub fn start<F>(answer: F) -> Self
    where
        F: Fn(&Recorded) -> Reply + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let thread = {
            let recorded = Arc::clone(&recorded);
            let stopped = Arc::clone(&stopped);
            thread::spawn(move || {
                for connection in listener.incoming() {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(mut connection) = connection else {
                        return;
                    };
                    let Some(request) = read_request(&mut connection) else {
                        continue;
                    };
                    let reply = answer(&request);
                    recorded
                        .lock()
                        .expect("the record is not poisoned")
                        .push(request);
                    let _ = connection.write_all(&render(&reply));
                    let _ = connection.flush();
                }
            })
        };
        Self {
            address,
            recorded,
            stopped,
            thread: Some(thread),
        }
    }

    /// A server answering every request the same way.
    pub fn always(reply: Reply) -> Self {
        Self::start(move |_| reply.clone())
    }

    /// The base URL a configuration reaches this server at.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Every request this server has received.
    ///
    /// # Panics
    ///
    /// Panics when the record is poisoned, which is a panic in the accept loop.
    pub fn requests(&self) -> Vec<Recorded> {
        self.recorded
            .lock()
            .expect("the record is not poisoned")
            .clone()
    }

    /// The one request this server received.
    ///
    /// # Panics
    ///
    /// Panics when it received any other number of them.
    pub fn only_request(&self) -> Recorded {
        let requests = self.requests();
        assert_eq!(
            requests.len(),
            1,
            "expected one request, found {requests:?}"
        );
        requests.into_iter().next().expect("the one request")
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        // Unblock the accept loop, which is parked on a connection that will
        // never arrive otherwise.
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A loopback address nothing is listening on.
///
/// Bound and then released, so the port is one this host handed out rather than
/// one this test hoped was free.
///
/// # Panics
///
/// Panics when no loopback socket can be bound, which is a broken host.
pub fn closed_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    drop(listener);
    format!("http://{address}")
}

/// The bytes one reply is written as.
fn render(reply: &Reply) -> Vec<u8> {
    match reply {
        Reply::Empty(status) => {
            format!("HTTP/1.1 {status} .\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .into_bytes()
        }
        Reply::Body(status, body) => format!(
            "HTTP/1.1 {status} .\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
        Reply::Raw(bytes) => bytes.clone(),
    }
}
