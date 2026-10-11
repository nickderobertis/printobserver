//! `extruded_to` reads a download as it arrives and stops at the position.
//!
//! The stand-in here is a real HTTP server on a real socket. It answers a
//! file's facts the way `OctoPrint` does, and then serves the file's download
//! with every byte up to a little past the requested position — and **holds
//! the rest unsent** until the test lets it go. An adapter that read the whole
//! download before answering would wait on those bytes until its timeout; this
//! one answers, correctly, while they are still held. The stand-in records
//! whether it had let go at the moment the answer came back, so the claim is
//! observed rather than inferred from timing.

use core::fmt::Write as _;
use std::io::Write as _;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter, extruded_to};
use printobserver_printer_api::{PrinterError, PrinterPort as _, ToolFilament};
use printobserver_types::FileName;

use crate::block_on::block_on;
use crate::received::{Recorded, read_request};

/// How far past the position the stand-in sends before it holds.
const BEYOND: usize = 64;

/// How long the stand-in holds the rest of the body at most.
const HOLD: Duration = Duration::from_secs(60);

/// How the stand-in frames the download.
#[derive(Debug, Clone, Copy)]
enum Framing {
    /// `Content-Length`.
    Length,
    /// `Transfer-Encoding: chunked`.
    Chunked,
    /// Neither: the body ends when the connection closes.
    Close,
}

/// What the stand-in answers the download with.
enum Download {
    /// The file, framed one way, sent up to a byte and then held.
    Held {
        /// How it is framed.
        framing: Framing,
        /// The whole file.
        bytes: Vec<u8>,
        /// How many of its bytes go out before the hold.
        sent_first: usize,
    },
    /// These bytes exactly, and then the connection closes.
    Raw(Vec<u8>),
}

/// A file's facts and its download, served by a real server.
struct StandIn {
    /// Where it answers.
    address: SocketAddr,
    /// Every request it received.
    recorded: Arc<Mutex<Vec<Recorded>>>,
    /// Set the moment the held rest of the body is let go.
    released: Arc<AtomicBool>,
    /// Lets the held rest go, when dropped.
    release: Option<Sender<()>>,
    /// The server.
    thread: Option<JoinHandle<()>>,
}

impl StandIn {
    /// Serve one file's facts, naming a download link, and then its download.
    fn start(facts_body: String, download: Download) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let released = Arc::new(AtomicBool::new(false));
        let (release, held): (Sender<()>, Receiver<()>) = mpsc::channel();
        let thread = {
            let recorded = Arc::clone(&recorded);
            let released = Arc::clone(&released);
            thread::spawn(move || {
                serve(
                    &listener,
                    &facts_body,
                    &download,
                    &recorded,
                    &released,
                    &held,
                );
            })
        };
        Self {
            address,
            recorded,
            released,
            release: Some(release),
            thread: Some(thread),
        }
    }

    /// A printer configured against this stand-in.
    fn printer(&self) -> OctoPrintPrinter {
        OctoPrintPrinter::new(
            OctoPrintConfig::new(&format!("http://{}", self.address), "test-key")
                .expect("a configuration")
                .with_timeout(Duration::from_secs(20)),
        )
    }

    /// Whether the held rest of the body has been let go.
    fn released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }

    /// Every request it received.
    fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().expect("not poisoned").clone()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        drop(self.release.take());
        // Unblock a server still waiting for a download nothing will request.
        let _ = std::net::TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The stand-in's two answers, in order.
fn serve(
    listener: &TcpListener,
    facts_body: &str,
    download: &Download,
    recorded: &Mutex<Vec<Recorded>>,
    released: &AtomicBool,
    held: &Receiver<()>,
) {
    for answer in 0..2 {
        let Ok((mut connection, _)) = listener.accept() else {
            return;
        };
        let Some(request) = read_request(&mut connection) else {
            return;
        };
        recorded.lock().expect("not poisoned").push(request);
        if answer == 0 {
            let _ = connection.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{facts_body}",
                    facts_body.len()
                )
                .as_bytes(),
            );
            continue;
        }
        match download {
            Download::Raw(bytes) => {
                let _ = connection.write_all(bytes);
            }
            Download::Held {
                framing,
                bytes,
                sent_first,
            } => {
                let (first, rest) = bytes.split_at((*sent_first).min(bytes.len()));
                let _ = connection.write_all(&head(*framing, bytes.len()));
                let _ = connection.write_all(&framed(*framing, first));
                let _ = connection.flush();
                // Hold the rest until the test lets it go.
                let _ = held.recv_timeout(HOLD);
                released.store(true, Ordering::SeqCst);
                let _ = connection.write_all(&framed(*framing, rest));
                if matches!(framing, Framing::Chunked) {
                    let _ = connection.write_all(b"0\r\n\r\n");
                }
            }
        }
    }
}

/// The header block of a download framed one way.
fn head(framing: Framing, length: usize) -> Vec<u8> {
    let framing = match framing {
        Framing::Length => format!("Content-Length: {length}\r\n"),
        Framing::Chunked => "Transfer-Encoding: chunked\r\n".to_owned(),
        Framing::Close => String::new(),
    };
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n{framing}Connection: close\r\n\r\n")
        .into_bytes()
}

/// Some bytes of a body, framed one way.
fn framed(framing: Framing, bytes: &[u8]) -> Vec<u8> {
    match framing {
        Framing::Chunked if !bytes.is_empty() => {
            let mut chunk = format!("{:x}\r\n", bytes.len()).into_bytes();
            chunk.extend_from_slice(bytes);
            chunk.extend_from_slice(b"\r\n");
            chunk
        }
        Framing::Chunked => Vec::new(),
        Framing::Length | Framing::Close => bytes.to_vec(),
    }
}

/// The facts of `print.gcode`, naming its download on some other host, the way
/// `OctoPrint` spells it absolute.
fn facts_naming(download: &str) -> String {
    format!(
        r#"{{"name": "print.gcode", "origin": "local", "type": "machinecode", "size": 1,
            "refs": {{"download": "{download}",
                      "resource": "http://elsewhere.invalid:9/api/files/local/print.gcode"}}}}"#
    )
}

/// The facts of `print.gcode`, downloadable at its usual path.
fn facts() -> String {
    facts_naming("http://elsewhere.invalid:9/downloads/files/local/print.gcode")
}

/// A file long enough that the position lands well inside it.
fn program() -> Vec<u8> {
    let mut text = String::from("M83\nT0\n");
    for line in 0..2_000 {
        writeln!(text, "G1 X{} Y{} E0.{}", line % 50, line % 30, line % 7 + 1)
            .expect("a String never fails to be written to");
        if line == 300 {
            text.push_str("T1\n");
        }
    }
    text.into_bytes()
}

/// A file name.
fn print_gcode() -> FileName {
    FileName::new("print.gcode").expect("a file name")
}

/// The correct figure is answered while the rest of the body is still held,
/// whichever way the download is framed.
#[test]
fn extruded_to_answers_while_the_rest_of_the_download_is_held() {
    let bytes = program();
    for (framing, position) in [
        (Framing::Length, bytes.len() * 2 / 3),
        (Framing::Chunked, bytes.len() / 2 + 3),
        (Framing::Close, bytes.len() / 4),
    ] {
        let stand_in = StandIn::start(
            facts(),
            Download::Held {
                framing,
                bytes: bytes.clone(),
                sent_first: position + BEYOND,
            },
        );
        let position = i64::try_from(position).expect("a position");

        let fed = block_on(stand_in.printer().extruded_to(print_gcode(), position))
            .expect("the filament fed");
        let held = !stand_in.released();

        assert!(
            held,
            "{framing:?}: the answer came back only after the rest was sent"
        );
        assert_eq!(fed, extruded_to(&bytes, position), "{framing:?}");
        assert_eq!(fed.len(), 2, "{framing:?}: both tools fed by then: {fed:?}");

        let requests = stand_in.requests();
        assert_eq!(requests[0].path, "/api/files/local/print.gcode");
        assert_eq!(
            requests[1].path, "/downloads/files/local/print.gcode",
            "the link's path, sent to the configured instance rather than its host"
        );
        assert_eq!(requests[1].header("x-api-key"), Some("test-key"));
    }
}

/// A position past the end is the whole file, whichever way it is framed.
#[test]
fn a_position_past_the_end_reads_the_whole_download() {
    let bytes = program();
    let whole = extruded_to(&bytes, i64::MAX);
    for framing in [Framing::Length, Framing::Chunked, Framing::Close] {
        let stand_in = StandIn::start(
            facts(),
            Download::Held {
                framing,
                bytes: bytes.clone(),
                sent_first: bytes.len(),
            },
        );
        let reading = thread::spawn({
            let printer = stand_in.printer();
            move || block_on(printer.extruded_to(print_gcode(), 1_000_000_000))
        });
        if !matches!(framing, Framing::Length) {
            // Nothing but the end of the body says where it ends, so the answer
            // arrives once the rest is let go.
            thread::sleep(Duration::from_millis(200));
            assert!(
                !reading.is_finished(),
                "{framing:?}: answered before the end"
            );
        }
        let mut stand_in = stand_in;
        drop(stand_in.release.take());

        let fed: Vec<ToolFilament> = reading
            .join()
            .expect("the reader")
            .expect("the filament fed");
        assert_eq!(fed, whole, "{framing:?}");
    }
}

/// A download the instance refuses is ruled on like every other request.
#[test]
fn a_refused_download_is_ruled_on_like_any_request() {
    let body = r#"{"error": "You don't have the permission to access the requested resource."}"#;
    let stand_in = StandIn::start(
        facts(),
        Download::Raw(
            format!(
                "HTTP/1.1 403 FORBIDDEN\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        ),
    );

    let error = block_on(stand_in.printer().extruded_to(print_gcode(), 10)).expect_err("a refusal");

    match error {
        PrinterError::Unauthorized { detail } => assert!(detail.contains("permission"), "{detail}"),
        other => panic!("expected the key refused, found {other:?}"),
    }
}

/// A file whose facts name no download link is malformed, and so is a link
/// that would change the request it lands in; neither is followed.
#[test]
fn a_missing_or_unfollowable_download_link_is_malformed() {
    let stub_facts = [
        r#"{"name": "print.gcode", "origin": "local", "type": "machinecode"}"#.to_owned(),
        facts_naming(r"http://elsewhere.invalid:9/downloads/a b.gcode"),
        facts_naming("downloads/files/local/print.gcode"),
    ];
    for body in stub_facts {
        let stand_in = StandIn::start(body.clone(), Download::Raw(Vec::new()));

        let error =
            block_on(stand_in.printer().extruded_to(print_gcode(), 10)).expect_err("a refusal");

        assert!(
            matches!(error, PrinterError::Malformed { .. }),
            "{body}: {error:?}"
        );
        assert_eq!(stand_in.requests().len(), 1, "{body}: a download was sent");
    }
}

/// A link with no path at all is the root, and a link that is already a path
/// is followed as it stands.
#[test]
fn a_link_spelled_as_a_path_is_followed_as_it_stands() {
    let bytes = b"M83\nG1 E2\n".to_vec();
    let stand_in = StandIn::start(
        facts_naming("/prefix/downloads/files/local/print.gcode"),
        Download::Raw(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                bytes.len(),
                String::from_utf8_lossy(&bytes)
            )
            .into_bytes(),
        ),
    );

    let fed = block_on(stand_in.printer().extruded_to(print_gcode(), 100)).expect("the feed");

    assert_eq!(fed, extruded_to(&bytes, 100));
    assert_eq!(
        stand_in.requests()[1].path,
        "/prefix/downloads/files/local/print.gcode"
    );

    let rooted = StandIn::start(
        facts_naming("http://elsewhere.invalid:9"),
        Download::Raw(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()),
    );
    let fed = block_on(rooted.printer().extruded_to(print_gcode(), 100)).expect("the feed");
    assert!(fed.is_empty(), "{fed:?}");
    assert_eq!(rooted.requests()[1].path, "/");
}

/// A broken download, what it is, and what it should be refused as.
type BrokenDownload = (&'static str, &'static [u8], fn(&PrinterError) -> bool);

/// A download that stops short of what it promised, or that is not framed as
/// it says, is not answered as though it were the file.
#[test]
fn a_broken_download_is_not_read_as_the_file() {
    let cases: [BrokenDownload; 5] = [
        (
            "a length it does not deliver",
            b"HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\nM83\nG1 E1\n",
            |error| matches!(error, PrinterError::Unreachable { .. }),
        ),
        (
            "a chunk size that is no number",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nM83\r\n0\r\n\r\n",
            |error| matches!(error, PrinterError::Malformed { .. }),
        ),
        (
            "a chunk longer than it declared",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nM83\r\n0\r\n\r\n",
            |error| matches!(error, PrinterError::Malformed { .. }),
        ),
        (
            "chunks that stop without a last one",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nM83\n\r\n",
            |error| matches!(error, PrinterError::Malformed { .. }),
        ),
        (
            "no header block",
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n",
            |error| matches!(error, PrinterError::Malformed { .. }),
        ),
    ];
    for (describing, raw, expected) in cases {
        let stand_in = StandIn::start(facts(), Download::Raw(raw.to_vec()));

        let error =
            block_on(stand_in.printer().extruded_to(print_gcode(), 1_000)).expect_err("a refusal");

        assert!(expected(&error), "{describing}: {error:?}");
    }
}
