//! A scripted `OctoPrint`, for the journeys a real one cannot be driven into.
//!
//! Compiled only under this crate's `standin` feature, which nothing but a
//! test enables. It is a real HTTP server on a real loopback socket, serving
//! the reads this adapter makes — the job, the printer, the connection — and
//! the push socket, so that a journey drives the real adapter, and everything
//! above it, through a reset landing idle, an error landing offline, or a
//! push socket that drops: the stops `OctoPrint`'s own virtual printer cannot
//! produce on demand. What it answers is the shape the
//! `spike-printer-observability` measurements tabulate for each of them.
//!
//! It lives here because this is the one crate that may spell an `OctoPrint`
//! path; a journey elsewhere scripts it through its methods alone.

use std::io::{Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use serde_json::{Value, json};

use crate::push::{SOCKET_ACCEPT, SOCKET_KEY};

/// A value no other run shares: `label`, then 32 hexadecimal digits drawn
/// from the standard library's per-process random hasher seed, so that no
/// key or session this stand-in admits is written into the tree.
#[must_use]
pub fn minted(label: &str) -> String {
    use core::hash::BuildHasher as _;
    use std::hash::RandomState;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let seeds = (std::process::id(), nanos, label);
    let high = RandomState::new().hash_one(seeds);
    let low = RandomState::new().hash_one(seeds);
    format!("{label}-{high:016x}{low:016x}")
}

/// What `GET /api/printer` answers a printer that is not operational with.
const NOT_OPERATIONAL: &str = r#"{"error":"Printer is not operational"}"#;

/// The job one script holds.
#[derive(Debug, Clone, PartialEq)]
pub struct JobScript {
    /// `OctoPrint`'s own word for the job's state.
    pub state: String,
    /// The error it reports.
    pub error: Option<String>,
    /// The file selected, and its size in bytes.
    pub file: Option<(String, i64)>,
    /// How far through the print is, as a percentage.
    pub completion: Option<f64>,
    /// How far into the file the printer has read, in bytes.
    pub filepos: Option<i64>,
    /// How long it has been printing, in seconds.
    pub print_time_s: Option<i64>,
}

impl JobScript {
    /// An idle printer, holding no file.
    #[must_use]
    pub fn idle() -> Self {
        Self {
            state: "Operational".to_owned(),
            error: None,
            file: None,
            completion: None,
            filepos: None,
            print_time_s: None,
        }
    }

    /// A job printing one file, this far in.
    #[must_use]
    pub fn printing(file: &str, size: i64, filepos: i64, print_time_s: i64) -> Self {
        let exact = |bytes: i64| f64::from(u32::try_from(bytes).unwrap_or(u32::MAX));
        let completion = 100.0 * exact(filepos) / exact(size);
        Self {
            state: "Printing".to_owned(),
            error: None,
            file: Some((file.to_owned(), size)),
            completion: Some(completion),
            filepos: Some(filepos),
            print_time_s: Some(print_time_s),
        }
    }
}

/// How the push socket's upgrade is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Upgrade {
    /// Upgraded, as `OctoPrint` upgrades it.
    Accepted,
    /// Refused with a 503.
    Refused,
    /// Upgraded with an accept that matches no key.
    WrongAccept,
    /// Upgraded leaving out its `Upgrade` and `Connection` headers.
    Bare,
    /// Answered with a head that never ends.
    Endless,
}

/// What the stand-in holds.
#[derive(Debug)]
struct Script {
    /// The connection's state, in `OctoPrint`'s own word.
    connection: String,
    /// The job.
    job: JobScript,
    /// Tool 0's target.
    tool_target: f64,
    /// The bed's target.
    bed_target: f64,
    /// Where the head is.
    position: [f64; 3],
    /// Whether an `M114` is answered on the push socket.
    answers_position: bool,
    /// Whether the command endpoint refuses every line.
    refuses_commands: bool,
    /// How the push socket's upgrade is answered.
    upgrade: Upgrade,
    /// Every line sent through the command endpoint, with whether a job was
    /// printing when it arrived.
    commands: Vec<(String, bool)>,
    /// Every job command received.
    job_commands: Vec<String>,
    /// Every print-head request received, as its body.
    printhead: Vec<Value>,
    /// Every request received, as its method and path.
    requests: Vec<String>,
    /// The key it admits.
    api_key: String,
    /// The session a successful passive login hands out.
    session: String,
    /// The status the passive login is answered with.
    login_status: u16,
    /// The body a successful passive login is answered with, in place of a
    /// session.
    login_body: Option<String>,

    /// The reply an `M114` is answered with in place of the head's position.
    position_reply: Option<String>,
    /// The opcode of every frame the client sent on a push socket.
    client_opcodes: Vec<u8>,
    /// The file whose last print the instance records as a success, its size
    /// and when that print ended, in seconds since the epoch.
    succeeded: Option<(String, i64, f64)>,
}

/// A scripted `OctoPrint` on a loopback socket.
pub struct ScriptedOctoPrint {
    /// Where it answers.
    address: SocketAddr,
    /// What it holds.
    script: Arc<Mutex<Script>>,
    /// The push sockets open now.
    sockets: Arc<Mutex<Vec<TcpStream>>>,
    /// Set when it should stop accepting.
    stopped: Arc<AtomicBool>,
    /// The accept loop.
    thread: Option<JoinHandle<()>>,
}

impl core::fmt::Debug for ScriptedOctoPrint {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ScriptedOctoPrint")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl ScriptedOctoPrint {
    /// Start one, idle and operational.
    ///
    /// # Panics
    ///
    /// Panics when no loopback socket can be bound, which is a broken host.
    #[must_use]
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let script = Arc::new(Mutex::new(Script {
            connection: "Operational".to_owned(),
            job: JobScript::idle(),
            tool_target: 0.0,
            bed_target: 0.0,
            position: [0.0; 3],
            answers_position: true,
            refuses_commands: false,
            upgrade: Upgrade::Accepted,
            commands: Vec::new(),
            job_commands: Vec::new(),
            printhead: Vec::new(),
            requests: Vec::new(),
            api_key: minted("octoprint-key"),
            session: minted("octoprint-session"),
            login_status: 200,
            login_body: None,

            position_reply: None,
            client_opcodes: Vec::new(),
            succeeded: None,
        }));
        let sockets = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let thread = {
            let script = Arc::clone(&script);
            let sockets = Arc::clone(&sockets);
            let stopped = Arc::clone(&stopped);
            thread::spawn(move || {
                for connection in listener.incoming() {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(connection) = connection else {
                        return;
                    };
                    let script = Arc::clone(&script);
                    let sockets = Arc::clone(&sockets);
                    thread::spawn(move || serve(connection, &script, &sockets));
                }
            })
        };
        Self {
            address,
            script,
            sockets,
            stopped,
            thread: Some(thread),
        }
    }

    /// The base URL an adapter reaches it at.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The key it admits, drawn when it started.
    #[must_use]
    pub fn api_key(&self) -> String {
        self.held().api_key.clone()
    }

    /// What it holds, held.
    fn held(&self) -> MutexGuard<'_, Script> {
        lock(&self.script)
    }

    /// Report this connection state and this job from now on.
    pub fn reports(&self, connection: &str, job: JobScript) {
        let mut held = self.held();
        connection.clone_into(&mut held.connection);
        held.job = job;
    }

    /// Record a print of `file`, of `size` bytes, as having just succeeded:
    /// what `OctoPrint`'s file history says once it ran a print of it to the
    /// end, answered at `GET /api/files/local/<file>`.
    ///
    /// # Panics
    ///
    /// Panics when the host's clock reads before the epoch, which is a broken
    /// host.
    pub fn records_a_success(&self, file: &str, size: i64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock reads after the epoch")
            .as_secs_f64();
        self.held().succeeded = Some((file.to_owned(), size, now));
    }

    /// How many times a file's history was read, at
    /// `GET /api/files/local/<file>`.
    #[must_use]
    pub fn history_reads(&self, file: &str) -> usize {
        let asked = format!("GET /api/files/local/{file}");
        self.held()
            .requests
            .iter()
            .filter(|request| request.starts_with(&asked))
            .count()
    }

    /// Report these heater targets from now on.
    pub fn targets(&self, tool_c: f64, bed_c: f64) {
        let mut held = self.held();
        held.tool_target = tool_c;
        held.bed_target = bed_c;
    }

    /// Put the head here.
    pub fn moves_head_to(&self, position: [f64; 3]) {
        self.held().position = position;
    }

    /// Refuse every line sent through the command endpoint from now on, or
    /// take them again.
    pub fn refuses_commands(&self, refuses: bool) {
        self.held().refuses_commands = refuses;
    }

    /// Answer `M114` on the push socket, or not.
    pub fn answers_position(&self, answers: bool) {
        self.held().answers_position = answers;
    }

    /// Answer the passive login with this status from now on.
    pub fn answers_login_with(&self, status: u16) {
        self.held().login_status = status;
    }

    /// Answer a successful passive login with this body rather than a session.
    pub fn answers_login_body(&self, body: &str) {
        self.held().login_body = Some(body.to_owned());
    }

    /// Answer the upgrade leaving out its `Upgrade` and `Connection` headers,
    /// or with them.
    pub fn answers_a_bare_upgrade(&self, bare: bool) {
        self.held().upgrade = if bare {
            Upgrade::Bare
        } else {
            Upgrade::Accepted
        };
    }

    /// Answer the upgrade with a head that never ends — one header line far
    /// longer than any upgrade answer, never followed by the blank line — or
    /// as `OctoPrint` answers it.
    pub fn answers_an_endless_upgrade(&self, endless: bool) {
        self.held().upgrade = if endless {
            Upgrade::Endless
        } else {
            Upgrade::Accepted
        };
    }

    /// Answer the upgrade with an accept matching no key, or with the right one.
    pub fn answers_a_wrong_accept(&self, wrong: bool) {
        self.held().upgrade = if wrong {
            Upgrade::WrongAccept
        } else {
            Upgrade::Accepted
        };
    }

    /// Answer `M114` with this terminal line rather than the head's position.
    pub fn answers_position_with(&self, line: &str) {
        self.held().position_reply = Some(line.to_owned());
    }

    /// Write these bytes, exactly, on every open push socket: a frame of the
    /// journey's own making, well-formed or not.
    pub fn pushes_raw(&self, bytes: &[u8]) {
        lock(&self.sockets).retain_mut(|socket| socket.write_all(bytes).is_ok());
    }

    /// The opcode of every frame a client sent on a push socket, in order.
    #[must_use]
    pub fn client_opcodes(&self) -> Vec<u8> {
        self.held().client_opcodes.clone()
    }

    /// Refuse the push socket's upgrade from now on, or admit it again.
    pub fn refuses_sockets(&self, refuses: bool) {
        self.held().upgrade = if refuses {
            Upgrade::Refused
        } else {
            Upgrade::Accepted
        };
    }

    /// Push one event, of `OctoPrint`'s own type and payload, on every open
    /// push socket.
    pub fn pushes_event(&self, kind: &str, payload: &Value) {
        self.broadcast(&json!({ "event": { "type": kind, "payload": payload } }));
    }

    /// Push these terminal lines on every open push socket.
    pub fn pushes_logs(&self, lines: &[&str]) {
        broadcast_to(&self.sockets, &current_with(lines));
    }

    /// One message, on every open push socket.
    fn broadcast(&self, message: &Value) {
        broadcast_to(&self.sockets, message);
    }

    /// Close every open push socket.
    pub fn drops_sockets(&self) {
        let mut sockets = lock(&self.sockets);
        for socket in sockets.drain(..) {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    /// How many push sockets are open now.
    #[must_use]
    pub fn open_sockets(&self) -> usize {
        lock(&self.sockets).len()
    }

    /// Every line sent through the command endpoint, with whether a job was
    /// printing as it arrived.
    #[must_use]
    pub fn commands(&self) -> Vec<(String, bool)> {
        self.held().commands.clone()
    }

    /// Every job command received.
    #[must_use]
    pub fn job_commands(&self) -> Vec<String> {
        self.held().job_commands.clone()
    }

    /// Every print-head request received — each move and home — as its body.
    #[must_use]
    pub fn printhead_requests(&self) -> Vec<Value> {
        self.held().printhead.clone()
    }

    /// Where the head is now.
    #[must_use]
    pub fn head(&self) -> [f64; 3] {
        self.held().position
    }

    /// The tool's and the bed's targets now.
    #[must_use]
    pub fn reported_targets(&self) -> (f64, f64) {
        let held = self.held();
        (held.tool_target, held.bed_target)
    }

    /// Every request received, as `METHOD path`.
    #[must_use]
    pub fn requests(&self) -> Vec<String> {
        self.held().requests.clone()
    }
}

impl Drop for ScriptedOctoPrint {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.drops_sockets();
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One lock, whatever a panicking holder left.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A `current` message carrying these terminal lines.
fn current_with(lines: &[&str]) -> Value {
    json!({ "current": { "logs": lines, "messages": [] } })
}

/// One message on every open push socket, dropping the ones that will not take
/// it.
fn broadcast_to(sockets: &Mutex<Vec<TcpStream>>, message: &Value) {
    let frame = text_frame(&message.to_string());
    lock(sockets).retain_mut(|socket| socket.write_all(&frame).is_ok());
}

/// One unmasked, final text frame, as a server sends it.
fn text_frame(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut frame = vec![0x81_u8];
    match u16::try_from(bytes.len()) {
        Ok(length) if length < 126 => frame.push(u8::try_from(length).unwrap_or(0)),
        Ok(length) => {
            frame.push(126);
            frame.extend_from_slice(&length.to_be_bytes());
        }
        Err(_) => {
            frame.push(127);
            frame.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(bytes);
    frame
}

/// One request read off a connection: its method, path, headers and body.
struct Request {
    /// The method.
    method: String,
    /// The path, query included.
    path: String,
    /// Each header, its name lowercased.
    headers: Vec<(String, String)>,
    /// The body.
    body: Vec<u8>,
}

impl Request {
    /// One header's value.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(held, _)| held == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Read one request, or nothing when the connection carries none.
fn read_request(connection: &mut TcpStream) -> Option<Request> {
    let mut buffered = Vec::new();
    let split = loop {
        if let Some(split) = buffered.windows(4).position(|window| window == b"\r\n\r\n") {
            break split;
        }
        let mut piece = [0_u8; 4096];
        let read = connection.read(&mut piece).ok()?;
        if read == 0 {
            return None;
        }
        buffered.extend_from_slice(&piece[..read]);
    };
    let head = String::from_utf8_lossy(&buffered[..split]).into_owned();
    let mut body = buffered[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let mut words = lines.next()?.split(' ');
    let method = words.next()?.to_owned();
    let path = words.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    while body.len() < length {
        let mut piece = [0_u8; 4096];
        let read = connection.read(&mut piece).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&piece[..read]);
    }
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

/// Answer one connection: one request, or the push socket it upgrades to.
fn serve(mut connection: TcpStream, script: &Mutex<Script>, sockets: &Mutex<Vec<TcpStream>>) {
    let Some(request) = read_request(&mut connection) else {
        return;
    };
    lock(script)
        .requests
        .push(format!("{} {}", request.method, request.path));
    if request.path.ends_with(crate::push::SOCKET_PATH) {
        upgrade(connection, &request, script, sockets);
        return;
    }
    let admitted = request.header("x-api-key") == Some(lock(script).api_key.as_str());
    let (status, body) = if admitted {
        answer(&request, script, sockets)
    } else {
        (403, r#"{"error":"Forbidden"}"#.to_owned())
    };
    let reply = format!(
        "HTTP/1.1 {status} .\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = connection.write_all(reply.as_bytes());
    let _ = connection.flush();
}

/// Whether a connection state is one `GET /api/printer` refuses.
fn not_operational(connection: &str) -> bool {
    connection.starts_with("Closed") || connection.starts_with("Offline") || connection == "Error"
}

/// The status and body one REST request is answered with.
fn answer(
    request: &Request,
    script: &Mutex<Script>,
    sockets: &Mutex<Vec<TcpStream>>,
) -> (u16, String) {
    let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
    let mut held = lock(script);
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/api/login") => match held.login_status {
            200 => (
                200,
                held.login_body.clone().unwrap_or_else(|| {
                    json!({ "name": "standin", "session": held.session }).to_string()
                }),
            ),
            status => (status, r#"{"error":"refused"}"#.to_owned()),
        },
        ("GET", "/api/connection") => (
            200,
            json!({ "current": { "state": held.connection, "port": "VIRTUAL" } }).to_string(),
        ),
        ("GET", path) if path.starts_with("/api/printer") => printer_body(&held),
        ("GET", "/api/job") => (200, job_body(&held.job).to_string()),
        ("POST", "/api/job") => {
            let command = body["command"].as_str().unwrap_or_default().to_owned();
            if command == "cancel" {
                "Operational".clone_into(&mut held.job.state);
                held.job.completion = None;
                held.job.filepos = None;
                "Operational".clone_into(&mut held.connection);
            }
            held.job_commands.push(command);
            (204, String::new())
        }
        ("POST", "/api/printer/command") if held.refuses_commands => {
            (409, r#"{"error":"Printer is not operational"}"#.to_owned())
        }
        ("POST", "/api/printer/command") => commanded(held, sockets, &body),
        // A move or a home, which a printing job refuses, as `OctoPrint`
        // does.
        ("POST", "/api/printer/printhead") if body["command"] == "feedrate" => {
            held.printhead.push(body);
            (204, String::new())
        }
        ("POST", "/api/printer/printhead") if held.job.state == "Printing" => (
            409,
            r#"{"error":"Printer is currently printing"}"#.to_owned(),
        ),
        ("POST", "/api/printer/printhead") => moved(&mut held, body),
        ("POST", "/api/printer/tool") => {
            if let Some(target) = body["targets"]["tool0"].as_f64() {
                held.tool_target = target;
            }
            (204, String::new())
        }
        ("POST", "/api/printer/bed") => {
            if let Some(target) = body["target"].as_f64() {
                held.bed_target = target;
            }
            (204, String::new())
        }
        ("GET", path) if path.starts_with("/api/files?") => {
            (200, json!({ "files": [] }).to_string())
        }
        ("GET", path) if path.starts_with("/api/files/local/") => {
            let asked = &path["/api/files/local/".len()..];
            match &held.succeeded {
                Some((file, size, at)) if file == asked => (
                    200,
                    json!({
                        "name": file,
                        "origin": "local",
                        "type": "machinecode",
                        "size": size,
                        "prints": {
                            "success": 1,
                            "failure": 0,
                            "last": { "date": at, "success": true },
                        },
                    })
                    .to_string(),
                ),
                _ => (404, r#"{"error":"Not found"}"#.to_owned()),
            }
        }
        _ => (404, r#"{"error":"Not found"}"#.to_owned()),
    }
}

/// Lines sent through the command endpoint, each recorded with whether a job
/// was printing, and an `M114` answered on the push socket.
fn commanded(
    mut held: MutexGuard<'_, Script>,
    sockets: &Mutex<Vec<TcpStream>>,
    body: &Value,
) -> (u16, String) {
    let printing = held.job.state == "Printing";
    let lines: Vec<String> = body["commands"]
        .as_array()
        .map(|lines| {
            lines
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let answer = lines.iter().any(|line| line == "M114") && held.answers_position;
    let [x, y, z] = held.position;
    let reply = held
        .position_reply
        .clone()
        .unwrap_or_else(|| format!("Recv: X:{x:.2} Y:{y:.2} Z:{z:.2} E:0.00 Count X:0 Y:0 Z:0"));
    for line in lines {
        held.commands.push((line, printing));
    }
    drop(held);
    if answer {
        broadcast_to(sockets, &current_with(&["Send: M114", &reply]));
    }
    (204, String::new())
}

/// One move or home of the print head: a jog moves Z from where it is and X
/// and Y to a place when it says so, and a home puts the axes it names at zero.
fn moved(held: &mut Script, body: Value) -> (u16, String) {
    let mut at = held.position;
    match body["command"].as_str() {
        Some("jog") => {
            let absolute = body["absolute"].as_bool().unwrap_or(false);
            for (index, axis) in ["x", "y", "z"].iter().enumerate() {
                if let Some(value) = body[*axis].as_f64() {
                    at[index] = if absolute { value } else { at[index] + value };
                }
            }
        }
        Some("home") => {
            for (index, axis) in ["x", "y", "z"].iter().enumerate() {
                let named = body["axes"]
                    .as_array()
                    .is_some_and(|axes| axes.iter().any(|named| named == axis));
                if named {
                    at[index] = 0.0;
                }
            }
        }
        _ => return (400, r#"{"error":"Unknown command"}"#.to_owned()),
    }
    held.position = at;
    held.printhead.push(body);
    (204, String::new())
}

/// What `GET /api/printer` answers for what the stand-in holds: `409` for a
/// printer that is not operational, as `OctoPrint` answers it.
fn printer_body(held: &Script) -> (u16, String) {
    if not_operational(&held.connection) {
        return (409, NOT_OPERATIONAL.to_owned());
    }
    let printing = held.job.state == "Printing";
    (
        200,
        json!({
            "state": {
                "text": held.connection,
                "flags": {
                    "operational": true,
                    "printing": printing,
                    "paused": held.job.state == "Paused",
                    "cancelling": held.job.state == "Cancelling",
                    "error": false,
                    "closedOrError": false,
                },
            },
            "temperature": {
                "tool0": { "actual": 25.0, "target": held.tool_target, "offset": 0 },
                "bed": { "actual": 25.0, "target": held.bed_target, "offset": 0 },
            },
        })
        .to_string(),
    )
}

/// What `GET /api/job` answers for one job.
fn job_body(job: &JobScript) -> Value {
    json!({
        "job": {
            "file": job.file.as_ref().map(|(name, size)| json!({
                "name": name,
                "origin": "local",
                "size": size,
            })),
        },
        "progress": {
            "completion": job.completion,
            "filepos": job.filepos,
            "printTime": job.print_time_s,
            "printTimeLeft": Value::Null,
        },
        "state": job.state,
        "error": job.error,
    })
}

/// Upgrade one connection to a push socket, and keep it open for broadcasts.
fn upgrade(
    mut connection: TcpStream,
    request: &Request,
    script: &Mutex<Script>,
    sockets: &Mutex<Vec<TcpStream>>,
) {
    let answer = lock(script).upgrade;
    if answer == Upgrade::Refused {
        let _ = connection.write_all(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    }
    if request.header("sec-websocket-key") != Some(SOCKET_KEY) {
        let _ = connection.write_all(
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    }
    if answer == Upgrade::Endless {
        let mut head = b"HTTP/1.1 101 Switching Protocols\r\nX-Filler: ".to_vec();
        head.extend(std::iter::repeat_n(b'a', 128 * 1024));
        let _ = connection.write_all(&head);
        return;
    }
    let accept = if answer == Upgrade::WrongAccept {
        "bm90IHRoZSBhY2NlcHQgZm9yIHRoaXMga2V5"
    } else {
        SOCKET_ACCEPT
    };
    let headers = if answer == Upgrade::Bare {
        ""
    } else {
        "Upgrade: websocket\r\nConnection: Upgrade\r\n"
    };
    let accepted = format!(
        "HTTP/1.1 101 Switching Protocols\r\n{headers}Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if connection.write_all(accepted.as_bytes()).is_err() {
        return;
    }
    let connected = json!({ "connected": { "version": "1.11.8" } });
    if connection
        .write_all(&text_frame(&connected.to_string()))
        .is_err()
    {
        return;
    }
    let Ok(held) = connection.try_clone() else {
        return;
    };
    let peer = connection.peer_addr().ok();
    lock(sockets).push(held);
    // What the client sends — its authentication, a pong — is read frame by
    // frame and its opcode kept; the read ends when the socket does, and the
    // socket is no longer one of the open ones.
    while let Some(opcode) = client_frame(&mut connection) {
        lock(script).client_opcodes.push(opcode);
    }
    lock(sockets).retain(|socket| socket.peer_addr().ok() != peer);
}

/// The opcode of one masked frame a client sent, its payload read and set
/// aside; nothing once the socket ends.
fn client_frame(connection: &mut TcpStream) -> Option<u8> {
    let mut header = [0_u8; 2];
    connection.read_exact(&mut header).ok()?;
    let length = match header[1] & 0x7F {
        126 => {
            let mut extended = [0_u8; 2];
            connection.read_exact(&mut extended).ok()?;
            u64::from(u16::from_be_bytes(extended))
        }
        127 => {
            let mut extended = [0_u8; 8];
            connection.read_exact(&mut extended).ok()?;
            u64::from_be_bytes(extended)
        }
        short => u64::from(short),
    };
    let mut mask = [0_u8; 4];
    connection.read_exact(&mut mask).ok()?;
    let mut payload = vec![0_u8; usize::try_from(length).ok()?];
    connection.read_exact(&mut payload).ok()?;
    Some(header[0] & 0x0F)
}

/// A real `OctoPrint`'s terminal and serial connection, for the live tiers
/// that put its virtual printer into a stop: a line typed at its terminal —
/// one of the virtual printer's own `!!DEBUG:` triggers, or a move made while
/// no job runs — and the reconnect a stop that went offline needs.
///
/// Compiled under `standin` alone, like the scripted printer: nothing but a
/// test reaches a printer with a line of its own choosing, and the port's own
/// [`PrinterCommand`](printobserver_printer_api::PrinterCommand) vocabulary is
/// closed.
#[derive(Debug, Clone)]
pub struct Terminal {
    /// The instance.
    config: crate::OctoPrintConfig,
}

impl Terminal {
    /// The terminal of the instance one configuration names.
    #[must_use]
    pub const fn new(config: crate::OctoPrintConfig) -> Self {
        Self { config }
    }

    /// One request, answered with its status, or why it was not.
    fn post(&self, path: &str, body: &Value) -> Result<u16, String> {
        let encoded = body.to_string();
        crate::http::send(
            self.config.endpoint(),
            self.config.api_key(),
            crate::http::Method::Post,
            &self.config.endpoint().path(path),
            Some(encoded.as_bytes()),
            self.config.timeout(),
        )
        .map(|response| response.status)
        .map_err(|transport| format!("{transport:?}"))
    }

    /// Type one line at the terminal.
    ///
    /// # Errors
    ///
    /// Answers why the instance did not take it.
    pub fn send(&self, line: &str) -> Result<(), String> {
        match self.post("/api/printer/command", &json!({ "commands": [line] }))? {
            204 => Ok(()),
            status => Err(format!("the terminal answered {status}")),
        }
    }

    /// Connect the printer again, on the connection the instance is
    /// configured with.
    ///
    /// # Errors
    ///
    /// Answers why the instance did not take it.
    pub fn connect(&self) -> Result<(), String> {
        match self.post(
            "/api/connection",
            &json!({ "command": "connect", "autoconnect": true }),
        )? {
            204 => Ok(()),
            status => Err(format!("the connection answered {status}")),
        }
    }
}
