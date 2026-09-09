//! A real host at the boundary the printer port reaches a machine over.
//!
//! This is not a double of the port. It is a socket answering the documents a
//! machine answers, so the adapter the server composes, the transport under it
//! and the parsing above it are all the real ones — the only thing that is not
//! a printer is the printer.
//!
//! # It says what it is doing, because policy turns on that
//!
//! Pausing is valid from printing, resuming from paused and starting from
//! operational, so no one fixed state lets a walk drive every action of the
//! vocabulary. A journey tells this host what to report before it asks for
//! something, which is what a machine would be doing anyway.
//!
//! # Two documents, distinguished by the request rather than by its path
//!
//! The two reads the port makes disagree about the type of one field, so no
//! single document satisfies both. This host distinguishes them by whether the
//! request carries a query at all — a property of the request rather than of
//! any vendor's path vocabulary, which no crate but the adapter may know. A
//! journey asserts that both halves arrived, so a host that stopped
//! distinguishing them fails loudly rather than quietly answering half a
//! status.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use printobserver_types::serde_json::{Map as JsonMap, Value, json};

/// The map a document's own object is built as.
type Map = JsonMap<String, Value>;

/// What the machine reports it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reports {
    /// Part-way through a print.
    Printing,
    /// Stopped part-way through a print.
    Paused,
    /// Idle, with nothing running.
    Operational,
}

impl Reports {
    /// The machine's own word for this state, and the flags it sets.
    const fn said(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Printing => ("Printing", &["operational", "printing"]),
            Self::Paused => ("Paused", &["operational", "paused"]),
            Self::Operational => ("Operational", &["operational"]),
        }
    }
}

/// The file the machine reports it is running.
pub const RUNNING_FILE: &str = "benchy.gcode";

/// The host, for as long as this process runs.
#[derive(Debug, Clone)]
pub struct Machine {
    /// Where it answers.
    pub address: SocketAddr,
    /// What it reports it is doing.
    reported: Arc<Mutex<Reports>>,
    /// Whether it refuses everything it is asked.
    refusing: Arc<Mutex<bool>>,
}

impl Machine {
    /// Start one, reporting a machine part-way through a print.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let reported = Arc::new(Mutex::new(Reports::Printing));
        let refusing = Arc::new(Mutex::new(false));
        let answering = Arc::clone(&reported);
        let refusals = Arc::clone(&refusing);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let reported = Arc::clone(&answering);
                let refusing = Arc::clone(&refusals);
                std::thread::spawn(move || answer(stream, &reported, &refusing));
            }
        });
        Self {
            address,
            reported,
            refusing,
        }
    }

    /// Tell it to refuse everything it is asked, or to stop.
    ///
    /// A machine having a bad day, which is a different thing from a policy
    /// that would not have it: the request was made and the machine would not
    /// have it, and what the caller is owed is the record and the machine's own
    /// answer rather than silence.
    pub fn refusing(&self, refusing: bool) {
        *self.refusing.lock().expect("whether it refuses") = refusing;
    }

    /// Tell it what to report it is doing.
    pub fn reports(&self, state: Reports) {
        *self.reported.lock().expect("the reported state") = state;
    }

    /// Where the server is configured to find it.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

/// The document the connection read is answered with.
fn connection_document(reported: Reports) -> Value {
    let (text, flags) = reported.said();
    json!({
        "state": {
            "text": text,
            "flags": flags
                .iter()
                .map(|flag| ((*flag).to_owned(), Value::Bool(true)))
                .collect::<Map>(),
        },
        "temperature": {
            "tool0": { "actual": 209.5, "target": 210.0 },
            "bed": { "actual": 59.5, "target": 60.0 },
        },
    })
}

/// The document the job read is answered with.
fn job_document(reported: Reports) -> Value {
    let (text, _) = reported.said();
    json!({
        "state": text,
        "job": {
            "file": { "name": RUNNING_FILE, "origin": "local", "size": 4211 },
            "estimatedPrintTime": 3600.0,
        },
        "progress": { "completion": 42.0, "printTime": 900, "printTimeLeft": 1200 },
    })
}

/// Read one request and answer the document it is for.
fn answer(mut stream: TcpStream, reported: &Mutex<Reports>, refusing: &Mutex<bool>) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2048];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let head = String::from_utf8_lossy(&request).into_owned();
    let target = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_owned();
    if *refusing.lock().expect("whether it refuses") {
        let _ = stream.write_all(
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\n\
              Content-Length: 2\r\nConnection: close\r\n\r\n{}",
        );
        return;
    }
    let state = *reported.lock().expect("the reported state");
    let body = if target.contains('?') {
        connection_document(state).to_string()
    } else {
        job_document(state).to_string()
    };
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
}
