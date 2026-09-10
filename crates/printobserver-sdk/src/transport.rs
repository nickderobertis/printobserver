//! One request to the one address this client was given.
//!
//! Nothing here uses an HTTP client crate. One request, written out, is less
//! in a dependent's build than a client library and its transport-layer
//! security stack would be — and this client speaks to a supervisor over a
//! loopback or a local network rather than to the internet, because the
//! supervisor is the thing beside the printer.
//!
//! An answer that declared more than it sent is a truncated document, and
//! handing it to a caller would be handing back less than the server said. It
//! is refused instead.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

use crate::error::ClientError;

/// The media type every operation takes a body in and answers in.
pub const MEDIA_TYPE: &str = "application/json";

/// The header a configured credential travels in.
pub const CREDENTIAL_HEADER: &str = "Authorization";

/// What the supervisor answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The status it answered under.
    pub status: u16,
    /// The whole document it answered with.
    pub body: String,
}

/// Ask the configured supervisor one thing.
///
/// # Errors
///
/// Returns [`ClientError::Unreachable`] when nothing answered at the address,
/// when the request could not be written, and when what came back is not an
/// answer this client can read — which includes an answer declaring more than
/// it sent.
pub fn send(
    address: &str,
    credential: Option<&str>,
    method: &str,
    target: &str,
    body: Option<&str>,
) -> Result<Answered, ClientError> {
    let failing = |detail: String| ClientError::Unreachable {
        address: address.to_owned(),
        detail,
    };
    let mut stream =
        TcpStream::connect(address).map_err(|error| failing(format!("{:?}", error.kind())))?;

    let authenticating = credential.map_or_else(String::new, |credential| {
        format!("{CREDENTIAL_HEADER}: Bearer {credential}\r\n")
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
        "{method} {target} HTTP/1.1\r\nHost: {address}\r\nAccept: {MEDIA_TYPE}\r\n\
         Connection: close\r\n{authenticating}{carried}"
    );
    stream.write_all(request.as_bytes()).map_err(|error| {
        failing(format!(
            "the request could not be sent ({:?})",
            error.kind()
        ))
    })?;

    let mut answer = Vec::new();
    stream
        .read_to_end(&mut answer)
        .map_err(|error| failing(format!("the answer could not be read ({:?})", error.kind())))?;
    read_answer(&answer).map_err(failing)
}

/// The status and the whole body of one HTTP answer.
fn read_answer(answer: &[u8]) -> Result<Answered, String> {
    let separator = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "the supervisor answered something this client cannot read".to_owned())?;
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

#[cfg(test)]
mod tests {
    use super::{Answered, read_answer, send};
    use crate::error::ClientError;

    /// A whole answer is read as its status and its body.
    #[test]
    fn a_whole_answer_is_read_as_its_status_and_its_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(
            read_answer(raw).expect("a whole answer"),
            Answered {
                status: 200,
                body: "{}".to_owned(),
            }
        );
    }

    /// An answer that declared more than it sent is refused rather than handed on.
    #[test]
    fn an_answer_that_declared_more_than_it_sent_is_refused() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n{}";
        let refusal = read_answer(raw).expect_err("a truncated answer is refused");
        assert!(refusal.contains("declared 9 bytes and sent 2"), "{refusal}");
    }

    /// An answer with no status, and one with no declared length, are refused.
    #[test]
    fn an_answer_with_no_status_and_one_with_no_length_are_refused() {
        assert!(read_answer(b"nothing at all").is_err());
        assert!(read_answer(b"ICY 200 OK\r\nContent-Length: 0\r\n\r\n").is_err());
        assert!(read_answer(b"HTTP/1.1 200 OK\r\n\r\n").is_err());
    }

    /// An address nothing is listening on is a failure naming that address.
    #[test]
    fn an_address_nothing_is_listening_on_names_that_address() {
        // Port zero is never listened on, so this connects to nothing without
        // depending on which ports this host happens to have free.
        let refusal = send("127.0.0.1:0", None, "GET", "/v1/status", None)
            .expect_err("nothing answers on port zero");
        assert!(
            matches!(&refusal, ClientError::Unreachable { address, .. } if address == "127.0.0.1:0"),
            "{refusal}"
        );
    }
}
