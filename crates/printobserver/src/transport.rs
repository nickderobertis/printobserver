//! One request to the one address this program was configured with.
//!
//! # There is exactly one place a connection is opened
//!
//! [`send`] is it, and the only address it can be given is the one
//! [`ClientConfig`] carries. Nothing in this program reaches a printer, a
//! failure detector, or anything else a supervisor talks to: everything a
//! caller asks for is a request to the supervisor, and the supervisor is what
//! holds those other connections.
//!
//! # The answer is taken as the length it declares
//!
//! An answer that declared more than it sent is a truncated document, and
//! printing it would hand a caller something that parses as less than the
//! server said. It is refused instead.
//!
//! Nothing here uses an HTTP client crate. One request, written out, is less in
//! the installed artifact than a client library and its transport-layer
//! security stack would be — and this program speaks to a supervisor over a
//! loopback or a local network rather than to the internet.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

use crate::config::{CREDENTIAL_HEADER, ClientConfig};

/// The media type every operation answers in.
pub use printobserver_server::MEDIA_TYPE;

/// What the supervisor answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The status it answered under.
    pub status: u16,
    /// The whole document it answered with.
    pub body: String,
}

/// Why this program did not get an answer at all.
///
/// One class rather than several, because what a caller does about every one of
/// them is the same: find out whether the supervisor is running, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreachable {
    /// Where this program looked.
    pub address: String,
    /// What happened there, in the words of whatever failed.
    pub detail: String,
}

impl core::fmt::Display for Unreachable {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "nothing answered at {}: {}. Start the supervisor there with `printobserver \
             server --config <path>`, or point this program at the address it is answering \
             on",
            self.address, self.detail
        )
    }
}

impl core::error::Error for Unreachable {}

/// Ask the configured supervisor one thing.
///
/// # Errors
///
/// Returns [`Unreachable`] when nothing answered at the configured address,
/// when the request could not be written, and when what came back is not an
/// answer this program can read — which includes an answer declaring more than
/// it sent.
pub fn send(
    config: &ClientConfig,
    method: &str,
    target: &str,
    body: Option<&str>,
) -> Result<Answered, Unreachable> {
    let address = config.address();
    let failing = |detail: String| Unreachable {
        address: address.clone(),
        detail,
    };
    let mut stream =
        TcpStream::connect(config.server).map_err(|error| failing(error.to_string()))?;

    // The credential is read here and nowhere else, and the whole request is
    // assembled in one place, so there is one line in this program that can put
    // it anywhere at all.
    let authenticating = config
        .credential
        .as_ref()
        .map_or_else(String::new, |credential| {
            format!("{CREDENTIAL_HEADER}: {}\r\n", credential.header_value())
        });
    let carried = body.map_or_else(
        || "\r\n".to_owned(),
        |document| {
            format!(
                "Content-Type: {MEDIA_TYPE}\r\nContent-Length: {}\r\n\r\n{document}",
                document.len()
            )
        },
    );
    let request = format!(
        "{method} {target} HTTP/1.1\r\nHost: {}\r\nAccept: {MEDIA_TYPE}\r\n\
         Connection: close\r\n{authenticating}{carried}",
        config.server
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| failing(format!("the request could not be sent: {error}")))?;

    let mut answer = Vec::new();
    stream
        .read_to_end(&mut answer)
        .map_err(|error| failing(format!("the answer could not be read: {error}")))?;
    read_answer(&answer).map_err(failing)
}

/// The status and the whole body of one HTTP answer.
fn read_answer(answer: &[u8]) -> Result<Answered, String> {
    let separator = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "the supervisor answered something this program cannot read".to_owned())?;
    let head = String::from_utf8_lossy(&answer[..separator]).into_owned();
    let body = &answer[separator + 4..];

    let status = head
        .split_whitespace()
        .nth(1)
        .filter(|_| head.starts_with("HTTP/1.1 "))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| format!("the supervisor answered no status: {head}"))?;
    let declared = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .ok_or_else(|| format!("the supervisor declared no length for what it answered: {head}"))?;
    if body.len() != declared {
        return Err(format!(
            "the supervisor declared {declared} bytes and sent {}",
            body.len()
        ));
    }
    Ok(Answered {
        status,
        body: String::from_utf8_lossy(body).into_owned(),
    })
}
