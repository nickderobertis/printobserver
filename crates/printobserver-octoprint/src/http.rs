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

use core::fmt::Write as _;
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
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| Transport::Malformed(format!("{status_line:?} is no status line")))?;

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
