//! One request as it arrived on a socket, and how it is read off one.
//!
//! Shared by the two servers this crate's journeys stand up: the stub the
//! deterministic tier drives, and the recording proxy the integration tier puts
//! in front of a real `OctoPrint` so that what the instance received can be
//! read rather than inferred.

use std::io::Read as _;
use std::net::TcpStream;

/// One request a server received, exactly as it arrived.
#[derive(Debug, Clone)]
pub struct Recorded {
    /// The method the request was made with.
    pub method: String,
    /// The path it was made against, query string and all.
    pub path: String,
    /// Every header it carried, in the order they arrived, names lowercased.
    pub headers: Vec<(String, String)>,
    /// The body it carried, as text.
    pub body: String,
}

impl Recorded {
    /// One header's value, when the request carried it.
    pub fn header(&self, name: &str) -> Option<&str> {
        let wanted = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(found, _)| *found == wanted)
            .map(|(_, value)| value.as_str())
    }

    /// The body as JSON, which every body this adapter sends is.
    ///
    /// # Panics
    ///
    /// Panics when the body is not JSON, which is a broken journey rather than
    /// a finding: every request this adapter carries a body on carries JSON.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|error| panic!("{:?} is not JSON: {error}", self.body))
    }
}

/// Read one whole request off a connection.
pub fn read_request(connection: &mut TcpStream) -> Option<Recorded> {
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        match connection.read(&mut byte) {
            Ok(0) | Err(_) => return None,
            Ok(_) => raw.push(byte[0]),
        }
    }
    let head = String::from_utf8_lossy(&raw).into_owned();
    let mut lines = head.split("\r\n");
    let mut start = lines.next()?.split_whitespace();
    let method = start.next()?.to_owned();
    let path = start.next()?.to_owned();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let length: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    if length > 0 && connection.read_exact(&mut body).is_err() {
        return None;
    }
    Some(Recorded {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}
