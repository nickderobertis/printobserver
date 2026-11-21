//! A machine running a real slice: the stood-in machine, with the file it
//! runs in front of it.
//!
//! The stood-in machine reports a job and answers every action, but holds no
//! file, so a print over it has no plan anybody can read. This host stands in
//! front of it at the boundary the printer port reaches, and adds the file: it
//! answers a read naming the running file with that file's record, the
//! record's own download link with its bytes — only the last ones when a suffix
//! range asks for those — and the machine's job read with the job reading as
//! far into the file as it was told. Every other request goes to the machine
//! as it arrived, and its answer comes back as the machine gave it.
//!
//! It knows the file by its name in a request and its download by the link it
//! gave out itself, so it spells no vendor's path, as the machine does not.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use printobserver_types::serde_json::{Value, json};

use crate::machine::RUNNING_FILE;

/// Where this host hands out the file's bytes: a link of its own, given in the
/// file's record.
const HELD: &str = "/held/";

/// The host in front of one machine, for as long as this process runs.
#[derive(Debug, Clone)]
pub struct Slicing {
    /// Where it answers, which is what the server is configured with.
    pub address: SocketAddr,
    /// How far into the file the job has read.
    position: Arc<AtomicI64>,
}

impl Slicing {
    /// Start one in front of the machine at `machine`, running `bytes` read as
    /// far as `position`.
    pub fn in_front_of(machine: &str, bytes: Vec<u8>, position: i64) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let behind: SocketAddr = machine
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .parse()
            .expect("the machine's address");
        let bytes = Arc::new(bytes);
        let reading = Arc::new(AtomicI64::new(position));
        let read_to = Arc::clone(&reading);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let bytes = Arc::clone(&bytes);
                let position = read_to.load(Ordering::SeqCst);
                std::thread::spawn(move || answer(stream, behind, &bytes, position));
            }
        });
        Self {
            address,
            position: reading,
        }
    }

    /// Where the server is configured to find the machine.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Tell it the job has read as far as `position` into the file.
    pub fn reads_to(&self, position: i64) {
        self.position.store(position, Ordering::SeqCst);
    }
}

/// Read one request and answer it: the file's record or bytes, the machine's
/// job read with the job's place in the file, or the machine's own answer.
fn answer(mut stream: TcpStream, behind: SocketAddr, bytes: &[u8], position: i64) {
    let Some(request) = read_whole(&mut stream) else {
        return;
    };
    let head = String::from_utf8_lossy(&request).into_owned();
    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default();
    let target = first.next().unwrap_or_default();
    if method == "GET" && target.contains(RUNNING_FILE) {
        let answered = if target.starts_with(HELD) {
            file_answer(bytes, &head)
        } else {
            json_answer(&file_record(bytes))
        };
        let _ = stream.write_all(&answered);
        return;
    }
    let Some(relayed) = relay(behind, &request) else {
        return;
    };
    if method == "GET"
        && !target.contains('?')
        && let Some(job) = with_the_file(&relayed, bytes.len(), position)
    {
        let _ = stream.write_all(&json_answer(&job));
        return;
    }
    let _ = stream.write_all(&relayed);
}

/// The machine's job read, the job reading `position` into a file of `size`.
fn with_the_file(relayed: &[u8], size: usize, position: i64) -> Option<Value> {
    let at = relayed
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let mut job: Value = printobserver_types::serde_json::from_slice(&relayed[at + 4..]).ok()?;
    job.pointer_mut("/job/file")?["size"] = json!(size);
    let progress = job.get_mut("progress")?;
    progress["filepos"] = json!(position);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a slice is a few megabytes, which a double holds exactly"
    )]
    let completion = position as f64 * 100.0 / size as f64;
    progress["completion"] = json!(completion);
    Some(job)
}

/// The file's record: its name, size and the link its bytes are downloaded
/// from, which is this host's own.
fn file_record(bytes: &[u8]) -> Value {
    json!({
        "name": RUNNING_FILE,
        "path": RUNNING_FILE,
        "origin": "local",
        "type": "machinecode",
        "size": bytes.len(),
        "refs": { "download": format!("{HELD}{RUNNING_FILE}") },
    })
}

/// One document as a whole answer.
fn json_answer(document: &Value) -> Vec<u8> {
    let body = document.to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// The file's bytes, or only its last ones when a suffix range asks for those.
fn file_answer(bytes: &[u8], head: &str) -> Vec<u8> {
    let total = bytes.len();
    let suffix = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("range"))
        .and_then(|(_, value)| value.trim().strip_prefix("bytes=-"))
        .and_then(|count| count.parse::<usize>().ok())
        .filter(|count| *count > 0 && total > 0);
    let (status, from, range) = suffix.map_or_else(
        || ("200 OK", 0, String::new()),
        |count| {
            let from = total.saturating_sub(count);
            (
                "206 Partial Content",
                from,
                format!("Content-Range: bytes {from}-{}/{total}\r\n", total - 1),
            )
        },
    );
    let body = &bytes[from..];
    let mut answered = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\n{range}\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answered.extend_from_slice(body);
    answered
}

/// Send one request to the machine as it arrived, and read its whole answer:
/// the machine closes every connection once it has answered.
fn relay(behind: SocketAddr, request: &[u8]) -> Option<Vec<u8>> {
    let mut machine = TcpStream::connect(behind).ok()?;
    machine.write_all(request).ok()?;
    let mut answered = Vec::new();
    machine.read_to_end(&mut answered).ok()?;
    Some(answered)
}

/// One whole request: its head, and as many body bytes as it declared.
fn read_whole(stream: &mut TcpStream) -> Option<Vec<u8>> {
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
