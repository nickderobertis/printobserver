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
//! # It does what it is asked, because a wall proves nothing
//!
//! A machine that answered every action with success and never moved is a
//! successful no-op, and a tier driven against one could not tell that from a
//! command that worked. So this host **honours** what it is asked: a job
//! command moves what it reports it is doing, and a heater target moves the
//! temperature it reports. It reads that off the request without knowing any
//! vendor's shape — the words it looks for are this system's own action
//! vocabulary, and which heater is which is the last segment of the request
//! target rather than a path it spells.
//!
//! [`Machine::deaf`] is the other half of that: a host that answers success and
//! changes nothing, which is the violation the tier's own effect assertions
//! have to refuse.
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

/// The tool temperature it reports before anything has changed it.
pub const TOOL_TARGET_C: f64 = 210.0;

/// The bed temperature it reports before anything has changed it.
pub const BED_TARGET_C: f64 = 60.0;

/// What this machine is reporting at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reporting {
    /// What it is doing.
    state: Reports,
    /// The tool temperature it is holding.
    tool_target_c: f64,
    /// The bed temperature it is holding.
    bed_target_c: f64,
}

impl Default for Reporting {
    fn default() -> Self {
        Self {
            state: Reports::Printing,
            tool_target_c: TOOL_TARGET_C,
            bed_target_c: BED_TARGET_C,
        }
    }
}

/// How this machine answers what it is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Answering {
    /// Whether it refuses everything.
    refusing: bool,
    /// Whether it answers success and changes nothing.
    deaf: bool,
}

/// The host, for as long as this process runs.
#[derive(Debug, Clone)]
pub struct Machine {
    /// Where it answers.
    pub address: SocketAddr,
    /// What it is reporting.
    reported: Arc<Mutex<Reporting>>,
    /// How it answers what it is asked.
    answering: Arc<Mutex<Answering>>,
}

impl Machine {
    /// Start one, reporting a machine part-way through a print.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let reported = Arc::new(Mutex::new(Reporting::default()));
        let answering = Arc::new(Mutex::new(Answering::default()));
        let reporting = Arc::clone(&reported);
        let answers = Arc::clone(&answering);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let reported = Arc::clone(&reporting);
                let answering = Arc::clone(&answers);
                std::thread::spawn(move || answer(stream, &reported, &answering));
            }
        });
        Self {
            address,
            reported,
            answering,
        }
    }

    /// Tell it to refuse everything it is asked, or to stop.
    ///
    /// A machine having a bad day, which is a different thing from a policy
    /// that would not have it: the request was made and the machine would not
    /// have it, and what the caller is owed is the record and the machine's own
    /// answer rather than silence.
    pub fn refusing(&self, refusing: bool) {
        self.answering.lock().expect("how it answers").refusing = refusing;
    }

    /// Tell it to answer success and change nothing, or to stop.
    ///
    /// The successful no-op: every action reaches it, every action is taken,
    /// and nothing moves. It is what the tier's own effect assertions are
    /// driven over, because an assertion that cannot tell this from a command
    /// that worked is not an assertion about anything.
    pub fn deaf(&self, deaf: bool) {
        self.answering.lock().expect("how it answers").deaf = deaf;
    }

    /// Tell it what to report it is doing.
    pub fn reports(&self, state: Reports) {
        self.reported.lock().expect("what it reports").state = state;
    }

    /// Put the temperatures it reports back where it started.
    pub fn holds_its_starting_temperatures(&self) {
        let mut reported = self.reported.lock().expect("what it reports");
        reported.tool_target_c = TOOL_TARGET_C;
        reported.bed_target_c = BED_TARGET_C;
    }

    /// Where the server is configured to find it.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

/// The document the connection read is answered with.
fn connection_document(reported: Reporting) -> Value {
    let (text, flags) = reported.state.said();
    json!({
        "state": {
            "text": text,
            "flags": flags
                .iter()
                .map(|flag| ((*flag).to_owned(), Value::Bool(true)))
                .collect::<Map>(),
        },
        "temperature": {
            "tool0": { "actual": 209.5, "target": reported.tool_target_c },
            "bed": { "actual": 59.5, "target": reported.bed_target_c },
        },
    })
}

/// The document the job read is answered with.
fn job_document(reported: Reporting) -> Value {
    let (text, _) = reported.state.said();
    json!({
        "state": text,
        "job": {
            "file": { "name": RUNNING_FILE, "origin": "local", "size": 4211 },
            "estimatedPrintTime": 3600.0,
        },
        "progress": { "completion": 42.0, "printTime": 900, "printTimeLeft": 1200 },
    })
}

/// Read one request, do what it asks, and answer the document it is for.
fn answer(mut stream: TcpStream, reported: &Mutex<Reporting>, answering: &Mutex<Answering>) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let head = String::from_utf8_lossy(&request).into_owned();
    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let target = first.next().unwrap_or_default().to_owned();

    let how = *answering.lock().expect("how it answers");
    if how.refusing {
        let _ = stream.write_all(
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\n\
              Content-Length: 2\r\nConnection: close\r\n\r\n{}",
        );
        return;
    }
    if method == "POST" && !how.deaf {
        let body = head.split_once("\r\n\r\n").map(|(_, body)| body);
        do_what_it_asks(&target, body.unwrap_or_default(), reported);
    }

    let reporting = *reported.lock().expect("what it reports");
    let body = if target.contains('?') {
        connection_document(reporting).to_string()
    } else {
        job_document(reporting).to_string()
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

/// One whole request: its head, and as many body bytes as it declared.
fn read_request(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        if let Some(separator) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|at| at + 4)
        {
            let head = String::from_utf8_lossy(&request[..separator]).into_owned();
            let declared = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or_default();
            if request.len() >= separator + declared {
                return Some(request);
            }
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return None,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
}

/// Move what this machine reports, according to what it was just asked.
///
/// Read off the request without knowing any vendor's shape: the words looked
/// for are this system's own action vocabulary — which a machine's own
/// vocabulary spells the same way — and which heater a temperature is for is
/// the last segment of the request target. Nothing here spells a path.
fn do_what_it_asks(target: &str, body: &str, reported: &Mutex<Reporting>) {
    let Ok(asked) = printobserver_types::serde_json::from_str::<Value>(body) else {
        return;
    };
    let said = words_in(&asked);
    let mut held = reported.lock().expect("what it reports");
    if said.iter().any(|word| word == "target") {
        if let Some(degrees) = numbers_in(&asked).first() {
            match target.rsplit('/').next().unwrap_or_default() {
                "tool" => held.tool_target_c = *degrees,
                "bed" => held.bed_target_c = *degrees,
                _ => {}
            }
        }
        return;
    }
    // Resuming is asked for as a resume of a pause, so the later word wins.
    for (word, state) in [
        ("resume", Reports::Printing),
        ("pause", Reports::Paused),
        ("cancel", Reports::Operational),
        ("start", Reports::Printing),
    ] {
        if said.iter().any(|said| said == word) {
            held.state = state;
            return;
        }
    }
}

/// Every string one document carries, at any depth.
fn words_in(asked: &Value) -> Vec<String> {
    match asked {
        Value::String(word) => vec![word.clone()],
        Value::Array(held) => held.iter().flat_map(words_in).collect(),
        Value::Object(held) => held.values().flat_map(words_in).collect(),
        _ => Vec::new(),
    }
}

/// Every number one document carries, at any depth.
fn numbers_in(asked: &Value) -> Vec<f64> {
    match asked {
        Value::Number(held) => held.as_f64().into_iter().collect(),
        Value::Array(held) => held.iter().flat_map(numbers_in).collect(),
        Value::Object(held) => held.values().flat_map(numbers_in).collect(),
        _ => Vec::new(),
    }
}
