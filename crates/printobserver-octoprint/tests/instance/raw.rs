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
    send(instance, "GET", path, None)
}

/// Remove one file of the instance's own storage, if it holds it.
///
/// # Panics
///
/// Panics when the instance answers anything but a removal or "no such file".
pub fn delete_local(instance: &Scripted, name: &str) {
    let path = format!("/api/files/local/{name}");
    let (status, body) = send(instance, "DELETE", &path, None);
    assert!(
        status == 204 || status == 404,
        "DELETE {path} answered {status}: {body}"
    );
}

/// Upload one file to the instance's own storage, into a folder of it or to
/// its top, as a browser would.
///
/// # Panics
///
/// Panics when the instance does not accept it.
pub fn upload_local(instance: &Scripted, folder: Option<&str>, name: &str, content: &[u8]) {
    let (body, content_type) = multipart(folder.map(|path| ("path", path)), Some((name, content)));
    let (status, answer) = send(
        instance,
        "POST",
        "/api/files/local",
        Some((&content_type, &body)),
    );
    assert_eq!(status, 201, "uploading {name} answered {status}: {answer}");
}

/// Create one folder at the top of the instance's own storage.
///
/// # Panics
///
/// Panics when the instance does not create it.
pub fn create_folder(instance: &Scripted, name: &str) {
    let (body, content_type) = multipart(Some(("foldername", name)), None);
    let (status, answer) = send(
        instance,
        "POST",
        "/api/files/local",
        Some((&content_type, &body)),
    );
    assert_eq!(status, 201, "creating {name} answered {status}: {answer}");
}

/// Send one JSON command, and expect it taken.
///
/// # Panics
///
/// Panics when the instance answers anything but success.
pub fn post_json(instance: &Scripted, path: &str, json: &serde_json::Value) {
    let body = json.to_string();
    let (status, answer) = send(
        instance,
        "POST",
        path,
        Some(("application/json", body.as_bytes())),
    );
    assert!(
        (200..300).contains(&status),
        "POST {path} answered {status}: {answer}"
    );
}

/// One multipart body, carrying at most one field and at most one file.
fn multipart(field: Option<(&str, &str)>, file: Option<(&str, &[u8])>) -> (Vec<u8>, String) {
    let boundary = "printobserver-upload-boundary";
    let mut body = Vec::new();
    if let Some((name, value)) = field {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n\
                 {value}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((name, content)) = file {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
                 filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (body, format!("multipart/form-data; boundary={boundary}"))
}

/// The status and body `OctoPrint` answers one request with.
fn send(
    instance: &Scripted,
    method: &str,
    path: &str,
    body: Option<(&str, &[u8])>,
) -> (u16, String) {
    let authority = instance
        .url
        .strip_prefix("http://")
        .expect("the record names an http URL");
    let mut stream = TcpStream::connect(authority)
        .unwrap_or_else(|error| panic!("connecting to {authority}: {error}"));
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nX-Api-Key: {}\r\n\
         Accept: application/json\r\nConnection: close\r\n",
        instance.api_key
    )
    .into_bytes();
    match body {
        Some((content_type, payload)) => {
            request.extend_from_slice(
                format!(
                    "Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                    payload.len()
                )
                .as_bytes(),
            );
            request.extend_from_slice(payload);
        }
        None => request.extend_from_slice(b"\r\n"),
    }
    stream
        .write_all(&request)
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
