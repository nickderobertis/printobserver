//! Reading `OctoPrint`'s own answers, in `OctoPrint`'s own units.
//!
//! The conversions this adapter owns are pinned against the instance's own
//! figures rather than against values this crate produced, so the tier needs a
//! way to ask the instance directly. This is that way, and it is deliberately
//! not the adapter's client: a reader that shared the adapter's own parsing
//! would agree with it about a mapping they were both wrong about.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

use crate::env::Scripted;

/// What `OctoPrint` itself answers one path with, as JSON.
///
/// # Panics
///
/// Panics when the instance cannot be reached or answers something that is not
/// JSON, either of which is a broken environment rather than a finding.
pub fn get(instance: &Scripted, path: &str) -> serde_json::Value {
    let (status, body) = call(instance, path);
    assert_eq!(status, 200, "GET {path} answered {status}: {body}");
    serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("GET {path} answered something that is not JSON: {error}"))
}

/// The status and body `OctoPrint` answers one path with.
///
/// # Panics
///
/// Panics when the instance cannot be reached, which is a broken environment.
pub fn call(instance: &Scripted, path: &str) -> (u16, String) {
    let authority = instance
        .url
        .strip_prefix("http://")
        .expect("the record names an http URL");
    let mut stream = TcpStream::connect(authority)
        .unwrap_or_else(|error| panic!("connecting to {authority}: {error}"));
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nX-Api-Key: {}\r\n\
         Accept: application/json\r\nConnection: close\r\n\r\n",
        instance.api_key
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.flush())
        .unwrap_or_else(|error| panic!("writing to {authority}: {error}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .unwrap_or_else(|error| panic!("reading from {authority}: {error}"));
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("{authority} answered no header block"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("{authority} answered no status: {head}"));
    (status, body.to_owned())
}
