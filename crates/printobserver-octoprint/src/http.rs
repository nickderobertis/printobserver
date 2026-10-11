//! The narrow HTTP client this adapter reaches `OctoPrint` with.
//!
//! It speaks exactly what the endpoints in [`crate::client`] need: one request
//! per connection, `Connection: close`, an `X-Api-Key` header, and a JSON body
//! on the requests that carry one. There is no connection pool, no redirect
//! following and no TLS, because none of those is something an adapter for one
//! instance on the machine's own network needs — and each of them is a place a
//! key could be sent somewhere the configuration did not name.
//!
//! The body is read to end-of-stream and then framed, so a response is read
//! whether the instance frames it by `Content-Length`, by chunked transfer or by
//! closing. A frame this module cannot read is a [`Transport::Malformed`],
//! never a silently truncated body.
//!
//! One request is read differently: a file's download, which can be megabytes
//! and of which the caller wants only a prefix. [`stream`] hands its body over a
//! piece at a time, as it arrives, and stops reading — closing the connection —
//! the moment the reader says it has enough, so the file is never held whole.

use core::fmt::Write as _;
use core::ops::ControlFlow;
use std::io::{Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::Duration;

use crate::config::{ApiKey, Endpoint};

/// The header the instance authenticates a request by.
pub(crate) const API_KEY_HEADER: &str = "X-Api-Key";

/// The methods this adapter uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    /// Read something.
    Get,
    /// Ask for something to happen.
    Post,
}

impl Method {
    /// The word this method is spelled with on the wire.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// What the instance answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    /// The status it answered with.
    pub(crate) status: u16,
    /// The body it answered with, which is empty for every action endpoint.
    pub(crate) body: Vec<u8>,
}

impl Response {
    /// The body as text, with whatever is not UTF-8 replaced.
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).trim().to_owned()
    }
}

/// Why a request did not produce a response this adapter could read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transport {
    /// The instance could not be reached, or stopped answering part-way.
    Unreachable(String),
    /// Something answered, but not something this client could frame.
    Malformed(String),
}

/// Send one request and read one response.
pub(crate) fn send(
    endpoint: &Endpoint,
    api_key: &ApiKey,
    method: Method,
    path: &str,
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<Response, Transport> {
    let mut stream = connect(endpoint, timeout)?;
    let request = compose(endpoint, api_key, method, path, body);
    stream
        .write_all(&request)
        .and_then(|()| stream.flush())
        .map_err(|error| Transport::Unreachable(format!("writing to {endpoint}: {error}")))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| Transport::Unreachable(format!("reading from {endpoint}: {error}")))?;
    parse(&raw)
}

/// How much of a streamed body is read at a time.
const STREAM_PIECE: usize = 16 * 1024;

/// What reads a streamed body, a piece at a time.
pub(crate) trait Sink {
    /// The next piece of the body; `Break` stops reading it.
    fn take(&mut self, bytes: &[u8]) -> ControlFlow<()>;
    /// The body ended before the sink said it had enough.
    fn end(&mut self);
}

/// Send one `GET` and hand a successful answer's body to a sink as it arrives.
///
/// Answers `None` once the sink has what it wanted or the body ended, and the
/// whole response — read to its end — when the status is not a success, so that
/// a refusal is ruled on exactly as [`send`]'s are.
pub(crate) fn stream(
    endpoint: &Endpoint,
    api_key: &ApiKey,
    path: &str,
    timeout: Duration,
    sink: &mut dyn Sink,
) -> Result<Option<Response>, Transport> {
    let mut stream = connect(endpoint, timeout)?;
    let request = compose(endpoint, api_key, Method::Get, path, None);
    stream
        .write_all(&request)
        .and_then(|()| stream.flush())
        .map_err(|error| Transport::Unreachable(format!("writing to {endpoint}: {error}")))?;
    let mut source = Source {
        stream,
        endpoint,
        buffered: Vec::new(),
    };

    let head = source.head()?;
    let (status, framing) = framing_of(&head)?;
    if !(200..300).contains(&status) {
        let mut raw = head;
        raw.extend_from_slice(b"\r\n\r\n");
        raw.append(&mut source.buffered);
        source
            .stream
            .read_to_end(&mut raw)
            .map_err(|error| Transport::Unreachable(format!("reading from {endpoint}: {error}")))?;
        return parse(&raw).map(Some);
    }
    let flow = match framing {
        Framing::Length(length) => source.length(length, sink)?,
        Framing::Chunked => source.chunked(sink)?,
        Framing::Close => source.until_close(sink)?,
    };
    if flow.is_continue() {
        sink.end();
    }
    Ok(None)
}

/// How a response says where its body ends.
enum Framing {
    /// After this many bytes.
    Length(usize),
    /// At the last chunk.
    Chunked,
    /// When the connection closes.
    Close,
}

/// A response's status and framing, from its header block.
fn framing_of(head: &[u8]) -> Result<(u16, Framing), Transport> {
    let head = String::from_utf8_lossy(head).into_owned();
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| Transport::Malformed("the response carries no status line".to_owned()))?;
    let status = status_of(status_line)?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for line in lines {
        let (name, value) = field_of(line)?;
        if name == "transfer-encoding" {
            // Chunked must be the last coding, and no other is one this
            // client can undo, so it is the only value read as a body.
            if !value.eq_ignore_ascii_case("chunked") {
                return Err(Transport::Malformed(format!(
                    "a body transfer-encoded {value:?} is not one this port reads"
                )));
            }
            chunked = true;
        } else if name == "content-length" {
            let declared = value
                .parse::<usize>()
                .map_err(|_| Transport::Malformed(format!("{value:?} is no content length")))?;
            if length.is_some_and(|earlier| earlier != declared) {
                return Err(Transport::Malformed(
                    "the response declares two different content lengths".to_owned(),
                ));
            }
            length = Some(declared);
        }
    }
    let framing = match (chunked, length) {
        (true, Some(_)) => {
            return Err(Transport::Malformed(
                "the response declares both a length and chunks".to_owned(),
            ));
        }
        (true, None) => Framing::Chunked,
        (false, Some(length)) => Framing::Length(length),
        (false, None) => Framing::Close,
    };
    Ok((status, framing))
}

/// One header or trailer field, its name lowercased and its value trimmed.
///
/// The name is a token — one or more of the characters HTTP allows in one, with
/// nothing between it and the colon — and the value carries no control
/// character but a tab, so a line that is not a field is never read as one.
fn field_of(line: &str) -> Result<(String, &str), Transport> {
    let refused = || Transport::Malformed(format!("{line:?} is no header field"));
    let (name, value) = line.split_once(':').ok_or_else(refused)?;
    let token = |byte: u8| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte);
    if name.is_empty() || !name.bytes().all(token) {
        return Err(refused());
    }
    if value
        .bytes()
        .any(|byte| byte.is_ascii_control() && byte != b'\t')
    {
        return Err(refused());
    }
    Ok((name.to_ascii_lowercase(), value.trim()))
}

/// The status a status line carries, when it is an HTTP/1 status line.
fn status_of(status_line: &str) -> Result<u16, Transport> {
    let mut parts = status_line.split_whitespace();
    let version = parts.next().unwrap_or_default();
    parts
        .next()
        .filter(|_| matches!(version, "HTTP/1.0" | "HTTP/1.1"))
        .and_then(|code| code.parse::<u16>().ok())
        .filter(|code| (100..=599).contains(code))
        .ok_or_else(|| Transport::Malformed(format!("{status_line:?} is no status line")))
}

/// A connection being read a piece at a time, with what has arrived and not
/// yet been handed on.
struct Source<'a> {
    /// The connection, read only as far as the sink asks.
    stream: TcpStream,
    /// Where it reaches, for naming in an error.
    endpoint: &'a Endpoint,
    /// Bytes read off the connection and not yet consumed.
    buffered: Vec<u8>,
}

impl Source<'_> {
    /// Read one more piece off the connection; `false` when it has closed.
    fn fill(&mut self) -> Result<bool, Transport> {
        let mut piece = [0_u8; STREAM_PIECE];
        let read = self.stream.read(&mut piece).map_err(|error| {
            Transport::Unreachable(format!("reading from {}: {error}", self.endpoint))
        })?;
        self.buffered.extend_from_slice(&piece[..read]);
        Ok(read > 0)
    }

    /// The header block, without its terminating blank line.
    fn head(&mut self) -> Result<Vec<u8>, Transport> {
        loop {
            if let Some(split) = find(&self.buffered, b"\r\n\r\n") {
                let head = self.buffered[..split].to_vec();
                self.buffered.drain(..split + 4);
                return Ok(head);
            }
            if !self.fill()? {
                return Err(Transport::Malformed(
                    "the response carries no header block".to_owned(),
                ));
            }
        }
    }

    /// A body framed by its length.
    fn length(&mut self, length: usize, sink: &mut dyn Sink) -> Result<ControlFlow<()>, Transport> {
        let mut left = length;
        while left > 0 {
            if self.buffered.is_empty() && !self.fill()? {
                return Err(Transport::Unreachable(format!(
                    "the response promised {length} bytes and delivered {}",
                    length - left
                )));
            }
            let take = left.min(self.buffered.len());
            let piece: Vec<u8> = self.buffered.drain(..take).collect();
            left -= take;
            if sink.take(&piece).is_break() {
                return Ok(ControlFlow::Break(()));
            }
        }
        Ok(ControlFlow::Continue(()))
    }

    /// A body framed by chunks.
    fn chunked(&mut self, sink: &mut dyn Sink) -> Result<ControlFlow<()>, Transport> {
        loop {
            let line = self.line()?;
            let header = String::from_utf8_lossy(&line).into_owned();
            let size_text = header.split(';').next().unwrap_or_default().trim();
            let size = usize::from_str_radix(size_text, 16)
                .map_err(|_| Transport::Malformed(format!("{size_text:?} is no chunk size")))?;
            if size == 0 {
                // The trailer section: header fields, ended by a blank line.
                loop {
                    let trailer = self.line()?;
                    if trailer.is_empty() {
                        return Ok(ControlFlow::Continue(()));
                    }
                    field_of(&String::from_utf8_lossy(&trailer))?;
                }
            }
            if self.length(size, sink)?.is_break() {
                return Ok(ControlFlow::Break(()));
            }
            if !self.line()?.is_empty() {
                return Err(Transport::Malformed(
                    "a chunk runs past the size it declared".to_owned(),
                ));
            }
        }
    }

    /// A body framed by the connection closing.
    fn until_close(&mut self, sink: &mut dyn Sink) -> Result<ControlFlow<()>, Transport> {
        loop {
            if !self.buffered.is_empty() {
                let piece = core::mem::take(&mut self.buffered);
                if sink.take(&piece).is_break() {
                    return Ok(ControlFlow::Break(()));
                }
            }
            if !self.fill()? {
                return Ok(ControlFlow::Continue(()));
            }
        }
    }

    /// One CRLF-terminated line, without its terminator.
    fn line(&mut self) -> Result<Vec<u8>, Transport> {
        loop {
            if let Some(end) = find(&self.buffered, b"\r\n") {
                let line = self.buffered[..end].to_vec();
                self.buffered.drain(..end + 2);
                return Ok(line);
            }
            if !self.fill()? {
                return Err(Transport::Malformed(
                    "the chunked body stops before its last chunk".to_owned(),
                ));
            }
        }
    }
}

/// Open a connection to the instance, within the configured timeout.
fn connect(endpoint: &Endpoint, timeout: Duration) -> Result<TcpStream, Transport> {
    let address = (endpoint.host(), endpoint.port())
        .to_socket_addrs()
        .map_err(|error| Transport::Unreachable(format!("resolving {endpoint}: {error}")))?
        .next()
        .ok_or_else(|| Transport::Unreachable(format!("{endpoint} resolves to no address")))?;
    let stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| Transport::Unreachable(format!("connecting to {endpoint}: {error}")))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|error| Transport::Unreachable(format!("timing {endpoint}: {error}")))?;
    Ok(stream)
}

/// The bytes of one request.
///
/// The key is written here and read through `ApiKey::expose` here alone: every
/// other reference to it in this crate is to a value that renders redacted.
fn compose(
    endpoint: &Endpoint,
    api_key: &ApiKey,
    method: Method,
    path: &str,
    body: Option<&[u8]>,
) -> Vec<u8> {
    let mut head = format!(
        "{} {path} HTTP/1.1\r\nHost: {}:{}\r\n{API_KEY_HEADER}: {}\r\n\
         Accept: application/json\r\nConnection: close\r\n",
        method.as_str(),
        endpoint.host(),
        endpoint.port(),
        api_key.expose(),
    );
    if let Some(payload) = body {
        write!(
            head,
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        )
        .expect("a String never fails to be written to");
    }
    head.push_str("\r\n");
    let mut request = head.into_bytes();
    if let Some(payload) = body {
        request.extend_from_slice(payload);
    }
    request
}

/// The status and body of one response, however it was framed.
fn parse(raw: &[u8]) -> Result<Response, Transport> {
    let split = find(raw, b"\r\n\r\n")
        .ok_or_else(|| Transport::Malformed("the response carries no header block".to_owned()))?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let rest = &raw[split + 4..];

    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| Transport::Malformed("the response carries no status line".to_owned()))?;
    let status = status_of(status_line)?;

    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "content-length" {
            length = value.parse::<usize>().ok();
        } else if name == "transfer-encoding" {
            chunked = value.to_ascii_lowercase().contains("chunked");
        }
    }

    let body = if chunked {
        dechunk(rest)?
    } else if let Some(length) = length {
        if rest.len() < length {
            return Err(Transport::Unreachable(format!(
                "the response promised {length} bytes and delivered {}",
                rest.len()
            )));
        }
        rest[..length].to_vec()
    } else {
        rest.to_vec()
    };
    Ok(Response { status, body })
}

/// The body of a chunked response.
fn dechunk(raw: &[u8]) -> Result<Vec<u8>, Transport> {
    let mut body = Vec::new();
    let mut rest = raw;
    loop {
        let line_end = find(rest, b"\r\n")
            .ok_or_else(|| Transport::Malformed("a chunk carries no size line".to_owned()))?;
        let header = String::from_utf8_lossy(&rest[..line_end]).into_owned();
        let size_text = header.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| Transport::Malformed(format!("{size_text:?} is no chunk size")))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(body);
        }
        if rest.len() < size + 2 {
            return Err(Transport::Unreachable(format!(
                "a chunk promised {size} bytes and delivered {}",
                rest.len()
            )));
        }
        body.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

/// Where a needle first occurs in a haystack.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
