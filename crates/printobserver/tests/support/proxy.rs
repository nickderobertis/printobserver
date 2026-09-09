//! What the server received, read at the wire rather than inferred from it.
//!
//! Every command in the walk is configured to reach the supervisor through
//! this, which forwards each request to the real server and keeps a copy. So
//! "the request the server received" is the bytes that arrived rather than a
//! guess made from what came back — which is what makes a command that
//! substituted a value for the one its caller gave fail here, whatever the
//! answer looked like.
//!
//! # It can also make a file go before the answer arrives
//!
//! The one failure this program declares that a colocated tier cannot otherwise
//! produce is a materialized image path that names no file **here**: the server
//! answers a path only when the file is there, and on one host the two are the
//! same filesystem. [`Proxy::losing_the_file`] hands the answer on only once
//! the file it names has gone, which is exactly what the program meets when it
//! runs somewhere that path does not resolve.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// One request, as it arrived, and the answer that went back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The method it was made with.
    pub method: String,
    /// The target it was made to, query included.
    pub target: String,
    /// The whole body it carried.
    pub body: String,
    /// The whole body the server answered with.
    ///
    /// Kept because what a client's own output may carry is held to what its
    /// server's answer carried, and that is a comparison against the bytes the
    /// server sent rather than against what the client says they were.
    pub answered: String,
}

/// What the proxy does with an answer before handing it on.
#[derive(Debug, Clone, Default)]
struct Losing {
    /// The file to remove once the server has answered.
    file: Option<PathBuf>,
}

/// A recording forward proxy in front of one server.
#[derive(Debug, Clone)]
pub struct Proxy {
    /// Where it answers, which is what a command is configured with.
    pub address: SocketAddr,
    /// Every request that has come through it.
    received: Arc<Mutex<Vec<Received>>>,
    /// What to do to the filesystem before handing an answer on.
    losing: Arc<Mutex<Losing>>,
}

impl Proxy {
    /// Start one in front of a server.
    pub fn in_front_of(server: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let received = Arc::new(Mutex::new(Vec::new()));
        let losing = Arc::new(Mutex::new(Losing::default()));
        let keeping = Arc::clone(&received);
        let doing = Arc::clone(&losing);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let keeping = Arc::clone(&keeping);
                let doing = Arc::clone(&doing);
                std::thread::spawn(move || forward(stream, server, &keeping, &doing));
            }
        });
        Self {
            address,
            received,
            losing,
        }
    }

    /// Where a command is configured to find the supervisor.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Every request that has come through since it was last forgotten.
    pub fn received(&self) -> Vec<Received> {
        self.received.lock().expect("what was received").clone()
    }

    /// The one request that has come through since it was last forgotten.
    ///
    /// # Panics
    ///
    /// Panics when none or more than one came through, which is a command that
    /// made no request or made several.
    pub fn the_one_request(&self) -> Received {
        let received = self.received();
        assert_eq!(
            received.len(),
            1,
            "one command made {} requests: {received:#?}",
            received.len()
        );
        received.into_iter().next().expect("one request")
    }

    /// Forget every request that has come through.
    pub fn forget(&self) {
        self.received.lock().expect("what was received").clear();
    }

    /// Remove this file once the server has answered, before the answer is
    /// handed on.
    pub fn losing_the_file(&self, file: Option<PathBuf>) {
        self.losing.lock().expect("what to lose").file = file;
    }
}

/// Read one whole request, record it, forward it, and answer what came back.
fn forward(
    mut client: TcpStream,
    server: SocketAddr,
    keeping: &Mutex<Vec<Received>>,
    doing: &Mutex<Losing>,
) {
    let Some(request) = read_request(&mut client) else {
        return;
    };
    let Ok(mut upstream) = TcpStream::connect(server) else {
        return;
    };
    if upstream.write_all(&request).is_err() {
        return;
    }
    let mut answer = Vec::new();
    if upstream.read_to_end(&mut answer).is_err() {
        return;
    }
    keeping
        .lock()
        .expect("what was received")
        .push(exchange(&request, &answer));
    if let Some(file) = doing.lock().expect("what to lose").file.clone() {
        let _ = std::fs::remove_file(file);
    }
    let _ = client.write_all(&answer);
}

/// One request and the answer it got, as text.
fn exchange(request: &[u8], answer: &[u8]) -> Received {
    let head = String::from_utf8_lossy(request).into_owned();
    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
    Received {
        method: first.next().unwrap_or_default().to_owned(),
        target: first.next().unwrap_or_default().to_owned(),
        body: after_the_head(&head),
        answered: after_the_head(&String::from_utf8_lossy(answer)),
    }
}

/// Everything after the head of one message.
fn after_the_head(message: &str) -> String {
    message
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default()
        .to_owned()
}

/// One whole request: its head, and as many body bytes as it declared.
fn read_request(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let separator = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|at| at + 4);
        if let Some(separator) = separator {
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
