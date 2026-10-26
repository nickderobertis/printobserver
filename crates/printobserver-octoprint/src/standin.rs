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

/// The key this stand-in admits.
pub const STANDIN_API_KEY: &str = "a-scripted-octoprint-key";

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
        #[expect(clippy::cast_precision_loss, reason = "a file position fits a double")]
        let completion = 100.0 * filepos as f64 / size as f64;
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
    /// Whether the push socket's upgrade is refused.
    refuses_sockets: bool,
    /// Every line sent through the command endpoint, with whether a job was
    /// printing when it arrived.
    commands: Vec<(String, bool)>,
    /// Every job command received.
    job_commands: Vec<String>,
    /// Every request received, as its method and path.
    requests: Vec<String>,
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
            refuses_sockets: false,
            commands: Vec::new(),
            job_commands: Vec::new(),
            requests: Vec::new(),
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

    /// Answer `M114` on the push socket, or not.
    pub fn answers_position(&self, answers: bool) {
        self.held().answers_position = answers;
    }

    /// Refuse the push socket's upgrade from now on, or admit it again.
    pub fn refuses_sockets(&self, refuses: bool) {
        self.held().refuses_sockets = refuses;
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
    let (status, body) = if request.header("x-api-key") == Some(STANDIN_API_KEY) {
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
        ("POST", "/api/login") => (
            200,
            json!({ "name": "standin", "session": "a-scripted-session" }).to_string(),
        ),
        ("GET", "/api/connection") => (
            200,
            json!({ "current": { "state": held.connection, "port": "VIRTUAL" } }).to_string(),
        ),
        ("GET", path) if path.starts_with("/api/printer") => {
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
        ("POST", "/api/printer/command") => {
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
            for line in lines {
                held.commands.push((line, printing));
            }
            drop(held);
            if answer {
                let reply = format!("Recv: X:{x:.2} Y:{y:.2} Z:{z:.2} E:0.00 Count X:0 Y:0 Z:0");
                broadcast_to(sockets, &current_with(&["Send: M114", &reply]));
            }
            (204, String::new())
        }
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
        _ => (404, r#"{"error":"Not found"}"#.to_owned()),
    }
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
    if lock(script).refuses_sockets {
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
    let accepted = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {SOCKET_ACCEPT}\r\n\r\n"
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
    lock(sockets).push(held);
    // What the client sends — its authentication — is read and set aside;
    // the read ends when the socket does.
    let mut sink = [0_u8; 1024];
    while matches!(connection.read(&mut sink), Ok(read) if read > 0) {}
}
