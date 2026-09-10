//! What the walk against a real supervisor stands on.
//!
//! Two things, and the second is what makes the first worth having.
//!
//! [`Proxy`] sits between the client and the **real** supervisor and forwards
//! every request to it, recording what went and what came back. That is where
//! the body a comparison is against comes from: the answer this supervisor
//! actually sent, rather than a document a test wrote for it.
//!
//! [`matches`] is the comparison. What a method answered is rendered back to
//! JSON and held against that captured body, field for field — so a client
//! that put anything of its own into an answer fails on the equality rather
//! than needing a probe that guesses what it put there. `live.rs`'s own
//! falsifying fixtures drive two such clients through it.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

/// One request that went to the supervisor, and the answer it sent back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    /// The method the call was made by.
    pub method: String,
    /// The whole request target, question mark and all.
    pub target: String,
    /// The body the call carried, empty where it carried none.
    pub body: String,
    /// The status the supervisor answered under.
    pub status: u16,
    /// The whole document the supervisor answered with.
    pub answer: String,
}

/// A recording proxy in front of one supervisor.
///
/// The client is pointed at this rather than at the supervisor, so nothing
/// about the client changes and everything it sends and is sent is seen.
pub struct Proxy {
    /// Where it answers.
    address: String,
    /// Everything that went through it, in order.
    seen: Arc<Mutex<Vec<Exchange>>>,
}

impl Proxy {
    /// A proxy forwarding to the supervisor at `server`.
    ///
    /// # Panics
    ///
    /// Panics when no loopback port can be taken, which is a machine nothing
    /// could be driven on.
    pub fn in_front_of(server: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener
            .local_addr()
            .expect("the bound address")
            .to_string();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let onward = server
            .trim()
            .trim_end_matches('/')
            .strip_prefix("http://")
            .unwrap_or(server)
            .to_owned();
        let recording = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                forward(stream, &onward, &recording);
            }
        });
        Self { address, seen }
    }

    /// Where this proxy answers, as a client's own configuration writes it.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The last exchange that went through it.
    ///
    /// # Panics
    ///
    /// Panics when nothing has, which is a call that was never made.
    pub fn last(&self) -> Exchange {
        self.seen
            .lock()
            .expect("the recording is takeable")
            .last()
            .cloned()
            .expect("a call reached the supervisor through this proxy")
    }

    /// How many calls have gone through it.
    pub fn calls(&self) -> usize {
        self.seen.lock().expect("the recording is takeable").len()
    }
}

/// Forward one request to the supervisor and record both halves.
///
/// The exchange is recorded **before** the answer goes back to the client, and
/// the order is the whole of why this is reliable: a caller reads `last()` the
/// moment its own call returns, so an exchange recorded after that answer is
/// one the caller can be looking for before it is there — and what it would
/// find instead is the call before it, which is a walk asserting the wrong
/// step and blaming the client.
fn forward(client: TcpStream, onward: &str, recording: &Arc<Mutex<Vec<Exchange>>>) -> Option<()> {
    let mut reading = BufReader::new(client);
    let mut request = String::new();
    reading.read_line(&mut request).ok()?;
    let mut words = request.split_whitespace();
    let method = words.next()?.to_owned();
    let target = words.next()?.to_owned();

    let mut head = String::new();
    let mut length = 0_usize;
    loop {
        let mut line = String::new();
        if reading.read_line(&mut line).ok()? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
        head.push_str(&line);
    }
    let mut carried = vec![0_u8; length];
    reading.read_exact(&mut carried).ok()?;
    let body = String::from_utf8_lossy(&carried).into_owned();

    let mut upstream = TcpStream::connect(onward).ok()?;
    upstream
        .write_all(format!("{method} {target} HTTP/1.1\r\n{head}\r\n{body}").as_bytes())
        .ok()?;
    let mut answered = Vec::new();
    upstream.read_to_end(&mut answered).ok()?;

    let separator = answered
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let answer_head = String::from_utf8_lossy(&answered[..separator]).into_owned();
    let status = answer_head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())?;
    recording
        .lock()
        .expect("the recording is takeable")
        .push(Exchange {
            method,
            target,
            body,
            status,
            answer: String::from_utf8_lossy(&answered[separator + 4..]).into_owned(),
        });

    let mut client = reading.into_inner();
    client.write_all(&answered).ok()?;
    client.flush().ok()?;
    Some(())
}

/// Whether what a method answered carries, field for field, what the
/// supervisor sent.
///
/// # Panics
///
/// Panics when the captured body is not a document, which is a supervisor that
/// answered something no client could read.
pub fn matches<T: Serialize>(answered: &T, captured: &str) -> bool {
    let sent: Value = serde_json::from_str(captured).expect("the supervisor answered a document");
    serde_json::to_value(answered).expect("an answer renders back as a document") == sent
}

/// Assert that what a method answered is what the supervisor sent.
///
/// # Panics
///
/// Panics naming the operation when it is not.
pub fn same<T: Serialize>(operation: &str, answered: &T, captured: &str) {
    assert!(
        matches(answered, captured),
        "`{operation}` answered something other than what the supervisor sent.\n\
         it answered: {}\nthe supervisor sent: {captured}",
        serde_json::to_string(answered).unwrap_or_default()
    );
}
