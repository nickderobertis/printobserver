//! A recording proxy in front of the scripted instance.
//!
//! Three of the actions this server issues have no read-back at all — the
//! machine reports neither an applied feedrate factor nor an applied flowrate
//! factor nor a fan setting — so the only observable those three have is what
//! the instance received. This is that observable: every request is recorded and
//! then forwarded verbatim to the real instance, which answers it, so what was
//! recorded is what the machine ruled on rather than what this server believed
//! it sent.
//!
//! Nothing here reads what it forwards. It carries no path, header or body of
//! the printer's own vocabulary, because only the adapter crate may construct
//! one of those — and a proxy that forwards bytes needs to know none of it.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::received::{Recorded, read_request};

/// A proxy that records what it forwards.
pub struct Proxy {
    /// Where it answers.
    address: SocketAddr,
    /// Everything it forwarded.
    recorded: Arc<Mutex<Vec<Recorded>>>,
    /// Set when it should stop accepting.
    stopped: Arc<AtomicBool>,
    /// The accept loop.
    thread: Option<JoinHandle<()>>,
}

impl Proxy {
    /// A proxy in front of the instance one base URL names.
    ///
    /// # Panics
    ///
    /// Panics when the upstream cannot be resolved or no loopback port is free,
    /// either of which is a broken environment.
    pub fn in_front_of(base_url: &str) -> Self {
        let authority = base_url
            .strip_prefix("http://")
            .expect("the instance answers over http");
        let upstream = authority
            .to_socket_addrs()
            .unwrap_or_else(|error| panic!("resolving {authority}: {error}"))
            .next()
            .unwrap_or_else(|| panic!("{authority} resolves to no address"));
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
                    let answer = forward(upstream, &request);
                    recorded
                        .lock()
                        .expect("the record is not poisoned")
                        .push(request);
                    if let Some(answer) = answer {
                        let _ = connection.write_all(&answer);
                        let _ = connection.flush();
                    }
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

    /// The base URL a configuration reaches this proxy at.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Everything the instance has received through this proxy.
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

    /// Every body the instance has received that carries one.
    pub fn bodies(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|request| !request.body.is_empty())
            .map(|request| request.body)
            .collect()
    }

    /// Forget everything recorded so far, so the next assertion is about the
    /// next action alone.
    ///
    /// # Panics
    ///
    /// Panics when the record is poisoned, which is a panic in the accept loop.
    pub fn forget(&self) {
        self.recorded
            .lock()
            .expect("the record is not poisoned")
            .clear();
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Send one recorded request upstream and read the whole answer back.
fn forward(upstream: SocketAddr, request: &Recorded) -> Option<Vec<u8>> {
    let mut stream = TcpStream::connect(upstream).ok()?;
    let mut head = format!("{} {} HTTP/1.1\r\n", request.method, request.path);
    for (name, value) in &request.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut raw = head.into_bytes();
    raw.extend_from_slice(request.body.as_bytes());
    stream.write_all(&raw).ok()?;
    stream.flush().ok()?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).ok()?;
    Some(answer)
}
