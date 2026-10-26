//! `OctoPrint`'s push socket, read as the printer's push channel.
//!
//! `OctoPrint` pushes what the printer does as it happens — a reset, an error,
//! every terminal line — over a websocket at [`SOCKET_PATH`]. Opening it is
//! three steps: a passive login with the API key (`POST` [`LOGIN_PATH`]
//! `{"passive": true}`) answers a user and a session; the socket is upgraded;
//! and the first message sent on it is `{"auth": "<user>:<session>"}`.
//!
//! The socket is spoken with the standard library and `serde_json` alone,
//! frames held to RFC 6455: a server's frame is never masked and carries no
//! reserved bit; a control frame is final and at most 125 bytes; a message is
//! one text frame and the continuations after it, and is UTF-8; a ping is
//! answered with a pong carrying its own payload; a close ends the stream. A
//! frame or a message past [`FRAME_LIMIT`] is refused rather than held.
//!
//! One thread reads the socket and hands what it read to the
//! [`PushStream`] the port answers. The stream ends when the socket does —
//! closed, refused, or a frame this reader will not take — and dropping the
//! stream shuts the socket, which ends the thread.

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::net::{Shutdown, TcpStream};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use printobserver_printer_api::{PrinterError, PrinterPush, Stream};
use serde::Deserialize;

use crate::config::OctoPrintConfig;
use crate::http::{self, Method, Transport};

/// Where the push socket is upgraded.
pub const SOCKET_PATH: &str = "/sockjs/websocket";

/// Where the passive login that authenticates the socket is made.
pub const LOGIN_PATH: &str = "/api/login";

/// The largest frame, and the largest whole message across its frames, this
/// reader holds: `OctoPrint`'s biggest, the history sent on connect, is tens of
/// kilobytes.
pub const FRAME_LIMIT: u64 = 16 * 1024 * 1024;

/// The `Sec-WebSocket-Key` this reader opens the socket with.
///
/// Fixed rather than drawn per connection: it is a nonce against caching
/// proxies, of which there are none between this board and its printer, and
/// the standard library carries no SHA-1 to check a fresh one's answer with.
pub const SOCKET_KEY: &str = "c3Bpa2UtcG9zaXRpb24tcmQ=";

/// The `Sec-WebSocket-Accept` RFC 6455 requires for [`SOCKET_KEY`]: base64 of
/// the SHA-1 of the key and the protocol's GUID.
pub const SOCKET_ACCEPT: &str = "jiEa7Dg/7St4gOODROz+ZiSta7k=";

/// The longest upgrade answer this reader reads before refusing it.
const HEAD_LIMIT: usize = 64 * 1024;

/// The mask every frame this reader sends is masked with.
const MASK: [u8; 4] = [0x37, 0xFA, 0x21, 0x3D];

/// The event type `OctoPrint` pushes when the printer reset.
const RESET_EVENT: &str = "PrinterReset";

/// The event type `OctoPrint` pushes when the printer reported an error.
const ERROR_EVENT: &str = "Error";

/// What a passive login answers.
#[derive(Deserialize)]
struct Login {
    /// The user the key belongs to.
    name: String,
    /// The session the socket is authenticated with.
    session: String,
}

/// What the reading thread and the stream share.
#[derive(Default)]
struct Shared {
    /// What has been read and not yet taken.
    queue: VecDeque<PrinterPush>,
    /// Whether the socket has ended.
    ended: bool,
    /// Who to wake when something arrives.
    waker: Option<Waker>,
}

/// The printer's push channel, as the port answers it.
pub struct PushStream {
    /// What the reading thread hands over.
    shared: Arc<Mutex<Shared>>,
    /// The socket, held to shut it when the stream is dropped.
    socket: TcpStream,
}

impl Stream for PushStream {
    type Item = PrinterPush;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<PrinterPush>> {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(push) = shared.queue.pop_front() {
            return Poll::Ready(Some(push));
        }
        if shared.ended {
            return Poll::Ready(None);
        }
        shared.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

impl Drop for PushStream {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

/// Hand what the reading thread read to the stream, waking it.
fn hand_over(shared: &Mutex<Shared>, pushes: Vec<PrinterPush>, ended: bool) {
    let mut held = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.queue.extend(pushes);
    held.ended |= ended;
    if let Some(waker) = held.waker.take() {
        waker.wake();
    }
}

/// An unreachable socket, in the port's vocabulary.
fn unreachable(detail: impl Into<String>) -> PrinterError {
    let error = PrinterError::Unreachable {
        detail: detail.into(),
    };
    log::warn!("octoprint push socket: {error}");
    error
}

/// Open and authenticate the push socket, and start reading it.
///
/// # Errors
///
/// Answers the port's own error when the login is refused, the socket cannot
/// be reached, or the upgrade is not one RFC 6455 accepts.
pub(crate) fn open(config: &OctoPrintConfig) -> Result<PushStream, PrinterError> {
    let endpoint = config.endpoint();
    let login = http::send(
        endpoint,
        config.api_key(),
        Method::Post,
        &endpoint.path(LOGIN_PATH),
        Some(br#"{"passive": true}"#),
        config.timeout(),
    )
    .map_err(|transport| match transport {
        Transport::Unreachable(detail) => unreachable(detail),
        Transport::Malformed(detail) => PrinterError::Malformed { detail },
    })?;
    match login.status {
        200 => {}
        401 | 403 => {
            return Err(PrinterError::Unauthorized {
                detail: login.text(),
            });
        }
        status => {
            return Err(PrinterError::Refused {
                status,
                detail: login.text(),
            });
        }
    }
    let login: Login =
        serde_json::from_slice(&login.body).map_err(|error| PrinterError::Malformed {
            detail: format!("{LOGIN_PATH} answered no session: {error}"),
        })?;
    if login.name.is_empty() || login.session.is_empty() {
        return Err(PrinterError::Malformed {
            detail: format!("{LOGIN_PATH} answered an empty user or session"),
        });
    }
    let mut socket =
        http::connect(endpoint, config.timeout()).map_err(|transport| match transport {
            Transport::Unreachable(detail) | Transport::Malformed(detail) => unreachable(detail),
        })?;
    let upgrade = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: {SOCKET_KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n",
        endpoint.path(SOCKET_PATH),
        endpoint.host(),
        endpoint.port()
    );
    socket
        .write_all(upgrade.as_bytes())
        .map_err(|error| unreachable(format!("the upgrade could not be sent: {error}")))?;
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > HEAD_LIMIT {
            return Err(unreachable("the upgrade answer never ended"));
        }
        head.extend(exact(&mut socket, 1).map_err(unreachable)?);
    }
    upgraded(&String::from_utf8_lossy(&head)).map_err(unreachable)?;
    let auth = serde_json::json!({ "auth": format!("{}:{}", login.name, login.session) });
    send_frame(&mut socket, Outgoing::Text, auth.to_string().as_bytes()).map_err(unreachable)?;
    // The reader waits as long as the printer is quiet: an idle printer pushes
    // nothing for minutes, and a socket that died is a read that fails.
    socket
        .set_read_timeout(None)
        .map_err(|error| unreachable(error.to_string()))?;
    let reading = socket
        .try_clone()
        .map_err(|error| unreachable(error.to_string()))?;
    let shared = Arc::new(Mutex::new(Shared::default()));
    let handed = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("printobserver-octoprint-push".to_owned())
        .spawn(move || read_until_ended(reading, &handed))
        .map_err(|error| unreachable(format!("the reader could not start: {error}")))?;
    Ok(PushStream { shared, socket })
}

/// Read messages until the socket ends, handing each one's pushes over.
fn read_until_ended(mut socket: TcpStream, shared: &Mutex<Shared>) {
    loop {
        match message(&mut socket) {
            Ok(text) => hand_over(shared, pushes_of(&text), false),
            Err(detail) => {
                log::warn!("octoprint push socket ended: {detail}");
                hand_over(shared, Vec::new(), true);
                return;
            }
        }
    }
}

/// What one pushed message says the printer did.
///
/// A `current` message's `logs` are terminal lines; an `event` of type
/// [`RESET_EVENT`] is a reset, and one of type [`ERROR_EVENT`] an error,
/// carrying the reason `OctoPrint` gave. The `history` sent on connect is the
/// past, not something the printer did now, and is read as nothing.
#[must_use]
pub fn pushes_of(text: &str) -> Vec<PrinterPush> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut pushes = Vec::new();
    if let Some(event) = value.get("event") {
        match event.get("type").and_then(serde_json::Value::as_str) {
            Some(RESET_EVENT) => pushes.push(PrinterPush::Reset),
            Some(ERROR_EVENT) => pushes.push(PrinterPush::Error {
                reason: event
                    .get("payload")
                    .and_then(|payload| payload.get("reason"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            }),
            _ => {}
        }
    }
    if let Some(logs) = value
        .get("current")
        .and_then(|current| current.get("logs"))
        .and_then(serde_json::Value::as_array)
    {
        pushes.extend(
            logs.iter()
                .filter_map(serde_json::Value::as_str)
                .map(|line| PrinterPush::Log {
                    line: line.to_owned(),
                }),
        );
    }
    pushes
}

/// Refuse an upgrade answer RFC 6455 does not accept: a status other than 101,
/// or a missing `Upgrade`, `Connection` or matching `Sec-WebSocket-Accept`.
fn upgraded(head: &str) -> Result<(), String> {
    let status = head.split("\r\n").next().unwrap_or("");
    let mut words = status.splitn(3, ' ');
    let switching =
        matches!(words.next(), Some("HTTP/1.1" | "HTTP/1.0")) && words.next() == Some("101");
    if !switching {
        return Err(format!("the push socket refused the upgrade: `{status}`"));
    }
    let header = |name: &str| {
        head.split("\r\n").skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    let has = |name: &str, token: &str| {
        header(name).is_some_and(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
        })
    };
    if !has("Upgrade", "websocket") || !has("Connection", "upgrade") {
        return Err("the upgrade answer named no websocket upgrade".to_owned());
    }
    if header("Sec-WebSocket-Accept").as_deref() != Some(SOCKET_ACCEPT) {
        return Err("the upgrade answer's Sec-WebSocket-Accept does not match the key".to_owned());
    }
    Ok(())
}

/// Read exactly `count` bytes.
fn exact(stream: &mut TcpStream, count: usize) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0; count];
    stream
        .read_exact(&mut buffer)
        .map_err(|error| format!("the push socket closed: {error}"))?;
    Ok(buffer)
}

/// One whole text message off the socket, answering pings on the way.
fn message(stream: &mut TcpStream) -> Result<String, String> {
    let mut whole: Option<Vec<u8>> = None;
    loop {
        let header = exact(stream, 2)?;
        let (first, second) = (header[0], header[1]);
        if first & 0x70 != 0 || second & 0x80 != 0 {
            return Err("the push socket sent a frame a server may not send".to_owned());
        }
        let mut length = u64::from(second & 0x7F);
        if length == 126 {
            let extended = exact(stream, 2)?;
            length = u64::from(u16::from_be_bytes([extended[0], extended[1]]));
        } else if length == 127 {
            let extended: [u8; 8] = exact(stream, 8)?
                .try_into()
                .map_err(|_| "a frame length is eight bytes".to_owned())?;
            length = u64::from_be_bytes(extended);
        }
        if length > FRAME_LIMIT {
            return Err(format!(
                "the push socket sent a {length}-byte frame, past {FRAME_LIMIT}"
            ));
        }
        let opcode = first & 0x0F;
        let last = first & 0x80 != 0;
        if opcode >= 0x8 && (!last || length > 125) {
            return Err(format!(
                "the push socket sent a control frame (opcode {opcode:#x}) fragmented or over \
                 125 bytes"
            ));
        }
        let count = usize::try_from(length).map_err(|error| error.to_string())?;
        let payload = exact(stream, count)?;
        match (opcode, whole.as_mut()) {
            (0x9, _) => send_frame(stream, Outgoing::Pong, &payload)?,
            (0xA, _) => {}
            (0x8, _) => return Err("the push socket closed".to_owned()),
            (0x1, None) => whole = Some(payload),
            (0x0, Some(begun)) => {
                if u64::try_from(begun.len() + payload.len()).unwrap_or(u64::MAX) > FRAME_LIMIT {
                    return Err(format!(
                        "the push socket sent a message past {FRAME_LIMIT} bytes across its frames"
                    ));
                }
                begun.extend_from_slice(&payload);
            }
            (0x0, None) => {
                return Err(
                    "the push socket sent a continuation with no message to continue".to_owned(),
                );
            }
            (0x1, Some(_)) => {
                return Err("the push socket began a message inside an unfinished one".to_owned());
            }
            (other, _) => return Err(format!("the push socket sent opcode {other:#x}")),
        }
        // Only a data frame can end a message: a ping or pong between a
        // message's fragments is final in itself, not for the message.
        if last
            && opcode <= 0x1
            && let Some(done) = whole.take()
        {
            return String::from_utf8(done).map_err(|error| {
                format!("the push socket sent a message that is not UTF-8: {error}")
            });
        }
    }
}

/// The frames this reader sends: nothing else can be framed.
#[derive(Clone, Copy)]
enum Outgoing {
    /// A text message.
    Text,
    /// The answer to a ping.
    Pong,
}

impl Outgoing {
    /// The RFC 6455 opcode.
    const fn opcode(self) -> u8 {
        match self {
            Self::Text => 0x1,
            Self::Pong => 0xA,
        }
    }
}

/// Send one final masked frame: a text message, or a pong carrying back the
/// payload of the ping it answers, as RFC 6455 requires.
fn send_frame(stream: &mut TcpStream, kind: Outgoing, payload: &[u8]) -> Result<(), String> {
    let mut frame = vec![0x80_u8 | kind.opcode()];
    let length = payload.len();
    if let Ok(short) = u8::try_from(length)
        && short < 126
    {
        frame.push(0x80 | short);
    } else {
        let medium = u16::try_from(length).map_err(|_| "a message too long to send")?;
        frame.push(0x80 | 0x7E);
        frame.extend_from_slice(&medium.to_be_bytes());
    }
    frame.extend_from_slice(&MASK);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ MASK[index % 4]),
    );
    stream
        .write_all(&frame)
        .map_err(|error| format!("the push socket refused a frame: {error}"))
}
