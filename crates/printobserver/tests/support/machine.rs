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
//!
//! # A file it holds, and the bytes of it
//!
//! [`Machine::running`] starts one holding a file's bytes and running it: the
//! job names that file and the position a journey stores in
//! [`Machine::stands_at`], so the server can read the file and follow the job
//! through it. A journey clears [`Machine::reports_position`] for a job read
//! that names no position, as a machine does that cannot say where its job
//! stands, and stores other bytes in [`Machine::file_bytes`] for a file
//! sliced again under its name, which the next print reads. A request is about that file when its target's last segment is
//! the file's name, as a heater is told by the last segment; it is for the
//! bytes when its target is the link this host itself answered for them — a
//! path of its own minting, which no vendor spells. A machine started with
//! [`Machine::start`] holds no file, reports no position and answers both as
//! before.
//!
//! The position, whether it is reported and the bytes are fields rather than
//! setters because `tests/integration.rs`
//! compiles this module and never moves a job: a field the serving thread
//! reads is live in both binaries, where a method only one calls is dead code
//! in the other.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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
    /// In a state of its own that the printer vocabulary does not name, and
    /// raising none of the flags that vocabulary reads.
    Unnamed,
}

/// The machine's own word for the state the printer vocabulary does not name.
pub const UNNAMED_STATE: &str = "Conditioning chamber";

impl Reports {
    /// The machine's own word for this state, and the flags it sets.
    const fn said(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Printing => ("Printing", &["operational", "printing"]),
            Self::Paused => ("Paused", &["operational", "paused"]),
            Self::Operational => ("Operational", &["operational"]),
            Self::Unnamed => (UNNAMED_STATE, &[]),
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
    /// Whether it is part-way through cancelling the job, whatever it
    /// otherwise reports.
    cancelling: bool,
}

impl Reporting {
    /// The machine's own word for what it is doing, and the flags it sets.
    const fn said(self) -> (&'static str, &'static [&'static str]) {
        if self.cancelling {
            ("Cancelling", &["operational", "printing", "cancelling"])
        } else {
            self.state.said()
        }
    }
}

impl Default for Reporting {
    fn default() -> Self {
        Self {
            state: Reports::Printing,
            tool_target_c: TOOL_TARGET_C,
            bed_target_c: BED_TARGET_C,
            cancelling: false,
        }
    }
}

/// What this machine refuses of what it is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Refusing {
    /// Nothing.
    #[default]
    Nothing,
    /// Everything.
    Everything,
    /// Everything it is asked to do, while it answers what it is asked to
    /// read.
    Actions,
}

/// How this machine answers what it is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Answering {
    /// What it refuses.
    refusing: Refusing,
    /// Whether it answers success and changes nothing.
    deaf: bool,
    /// Whether it drops every request unanswered, as a machine that is off.
    unreachable: bool,
}

/// A file this machine holds and runs, as one request reads it.
#[derive(Debug, Clone, Copy)]
struct Held<'a> {
    /// Its name.
    name: &'a str,
    /// Every byte of it.
    bytes: &'a [u8],
}

/// The path of the link this machine answers a held file's bytes at: its own,
/// so a request for it is told apart without any vendor's path.
const HELD_BYTES_AT: &str = "/the-bytes-this-machine-holds";

/// The host, for as long as this process runs.
#[derive(Debug, Clone)]
pub struct Machine {
    /// Where it answers.
    pub address: SocketAddr,
    /// The name of the file it holds and runs, when it was started holding
    /// one.
    held: Option<Arc<str>>,
    /// Every byte of the file it holds: what a journey stores here is what
    /// the next read of the file answers.
    pub file_bytes: Arc<Mutex<Arc<Vec<u8>>>>,
    /// How far into the file it holds its job has read, in bytes: what a
    /// journey stores here is the position the next job read reports. A
    /// machine holding no file reports none.
    pub stands_at: Arc<AtomicU64>,
    /// Whether a job read reports that position at all.
    pub reports_position: Arc<AtomicBool>,
    /// What it is reporting.
    reported: Arc<Mutex<Reporting>>,
    /// How it answers what it is asked.
    answering: Arc<Mutex<Answering>>,
    /// How many requests it has been sent that ask it to do something rather
    /// than to say something: every one whose body names what it asks.
    pub commanded: Arc<AtomicUsize>,
}

impl Machine {
    /// Start one, reporting a machine part-way through a print.
    pub fn start() -> Self {
        Self::holding(None, Vec::new())
    }

    /// Start one running the file given, by name, with its job at its first
    /// byte until a journey moves [`Self::stands_at`].
    pub fn running(name: &str, bytes: Vec<u8>) -> Self {
        Self::holding(Some(name.into()), bytes)
    }

    /// Start one, holding the file given.
    fn holding(held: Option<Arc<str>>, bytes: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let machine = Self {
            address: listener.local_addr().expect("the bound address"),
            held,
            file_bytes: Arc::new(Mutex::new(Arc::new(bytes))),
            stands_at: Arc::new(AtomicU64::new(0)),
            reports_position: Arc::new(AtomicBool::new(true)),
            reported: Arc::new(Mutex::new(Reporting::default())),
            answering: Arc::new(Mutex::new(Answering::default())),
            commanded: Arc::new(AtomicUsize::new(0)),
        };
        let serving = machine.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let machine = serving.clone();
                std::thread::spawn(move || {
                    let bytes = Arc::clone(&machine.file_bytes.lock().expect("the file's bytes"));
                    let position = machine
                        .reports_position
                        .load(Ordering::SeqCst)
                        .then(|| machine.stands_at.load(Ordering::SeqCst));
                    let running = machine.held.as_deref().map(|name| {
                        (
                            Held {
                                name,
                                bytes: &bytes,
                            },
                            position,
                        )
                    });
                    answer(
                        stream,
                        &machine.reported,
                        &machine.answering,
                        running,
                        machine.address,
                        &machine.commanded,
                    );
                });
            }
        });
        machine
    }

    /// Tell it to refuse everything it is asked, or to stop.
    ///
    /// A machine having a bad day, which is a different thing from a policy
    /// that would not have it: the request was made and the machine would not
    /// have it, and what the caller is owed is the record and the machine's own
    /// answer rather than silence.
    pub fn refusing(&self, refusing: bool) {
        self.answering.lock().expect("how it answers").refusing = if refusing {
            Refusing::Everything
        } else {
            Refusing::Nothing
        };
    }

    /// Tell it to refuse everything it is asked to do while it goes on
    /// answering what it is asked to read, or to stop.
    pub fn refusing_actions(&self, refusing: bool) {
        self.answering.lock().expect("how it answers").refusing = if refusing {
            Refusing::Actions
        } else {
            Refusing::Nothing
        };
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

    /// Tell it to drop every request unanswered, as a machine switched off
    /// does, or to stop.
    pub fn unreachable(&self, unreachable: bool) {
        self.answering.lock().expect("how it answers").unreachable = unreachable;
    }

    /// Tell it to report itself part-way through cancelling its job, or to
    /// stop and report what it otherwise does.
    pub fn cancelling(&self, cancelling: bool) {
        self.reported.lock().expect("what it reports").cancelling = cancelling;
    }

    /// Tell it what to report it is doing.
    pub fn reports(&self, state: Reports) {
        self.reported.lock().expect("what it reports").state = state;
    }

    /// The file it runs: the one it holds, or [`RUNNING_FILE`].
    pub fn running_file(&self) -> String {
        self.held
            .as_ref()
            .map_or_else(|| RUNNING_FILE.to_owned(), |held| (**held).to_owned())
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
            "tool0": { "actual": 209.5, "target": reported.tool_target_c },
            "bed": { "actual": 59.5, "target": reported.bed_target_c },
        },
    })
}

/// The document the job read is answered with, naming the file it holds and
/// where in it the job stands when it holds one and reports it.
fn job_document(reported: Reporting, running: Option<(Held<'_>, Option<u64>)>) -> Value {
    let (text, _) = reported.said();
    let (name, size) = running.map_or((RUNNING_FILE, 4211), |(held, _)| {
        (held.name, held.bytes.len())
    });
    let mut document = json!({
        "state": text,
        "job": {
            "file": { "name": name, "origin": "local", "size": size },
            "estimatedPrintTime": 3600.0,
        },
        "progress": { "completion": 42.0, "printTime": 900, "printTimeLeft": 1200 },
    });
    if let Some((_, Some(position))) = running {
        document["progress"]["filepos"] = json!(position);
    }
    document
}

/// The document a read of the file it holds is answered with: what the file
/// is, and the link its bytes are answered at.
fn file_document(held: Held<'_>, address: SocketAddr) -> Value {
    json!({
        "name": held.name,
        "origin": "local",
        "type": "machinecode",
        "size": held.bytes.len(),
        "refs": { "download": format!("http://{address}{HELD_BYTES_AT}") },
    })
}

/// Read one request, do what it asks, and answer the document it is for.
fn answer(
    mut stream: TcpStream,
    reported: &Mutex<Reporting>,
    answering: &Mutex<Answering>,
    running: Option<(Held<'_>, Option<u64>)>,
    address: SocketAddr,
    commanded: &AtomicUsize,
) {
    let file = running.map(|(file, _)| file);
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let head = String::from_utf8_lossy(&request).into_owned();
    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let target = first.next().unwrap_or_default().to_owned();
    // A request asking for something names it in its body: a session's
    // sign-in, which names nothing, asks the machine to do nothing.
    if method == "POST"
        && head
            .split_once("\r\n\r\n")
            .and_then(|(_, body)| printobserver_types::serde_json::from_str::<Value>(body).ok())
            .is_some_and(|asked| !words_in(&asked).is_empty())
    {
        commanded.fetch_add(1, Ordering::SeqCst);
    }

    let how = *answering.lock().expect("how it answers");
    if how.unreachable {
        return;
    }
    if how.refusing == Refusing::Everything {
        let _ = stream.write_all(
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\n\
              Content-Length: 2\r\nConnection: close\r\n\r\n{}",
        );
        return;
    }
    if method == "POST" && how.refusing == Refusing::Actions {
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

    if let Some(held) = file.filter(|_| target == HELD_BYTES_AT) {
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                held.bytes.len()
            )
            .as_bytes(),
        );
        let _ = stream.write_all(held.bytes);
        return;
    }
    let reporting = *reported.lock().expect("what it reports");
    let body = if target.contains('?') {
        connection_document(reporting).to_string()
    } else if let Some(held) = file.filter(|held| target.rsplit('/').next() == Some(held.name)) {
        file_document(held, address).to_string()
    } else {
        job_document(reporting, running).to_string()
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
