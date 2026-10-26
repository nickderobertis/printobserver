//! Replies that are not framed as HTTP/1 frames a body, each with what it is.
//!
//! One table, because every request this adapter makes is framed by one set of
//! rules: an ordinary read, an action and a file's download each refuse every
//! reply here as malformed, and a journey proves that by sending the same table
//! down each path.

/// The payload every reply here carries, standing where a body would be.
const PAYLOAD: &[u8] = b"M83";

/// Each reply as it would go on the wire, its payload swapped for `payload` —
/// three bytes, so that every length the table declares still holds.
pub fn misframed(payload: [u8; 3]) -> Vec<(&'static str, Vec<u8>)> {
    MISFRAMED
        .iter()
        .map(|&(describing, raw)| (describing, swapped(raw, payload)))
        .collect()
}

/// The bytes with every occurrence of the table's payload swapped.
fn swapped(raw: &[u8], payload: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut rest = raw;
    while !rest.is_empty() {
        if rest.starts_with(PAYLOAD) {
            out.extend_from_slice(&payload);
            rest = &rest[PAYLOAD.len()..];
        } else {
            out.push(rest[0]);
            rest = &rest[1..];
        }
    }
    out
}

/// The replies, each carrying `M83` where its body would be.
const MISFRAMED: [(&str, &[u8]); 26] = [
    (
        "a chunk size that is no number",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nM83\r\n0\r\n\r\n",
    ),
    (
        "a chunk longer than it declared",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nM83\r\n0\r\n\r\n",
    ),
    (
        "chunks that stop without a last one",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nM83\n\r\n",
    ),
    (
        "no header block",
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n",
    ),
    (
        "a status line that is not HTTP's",
        b"garbage 200 OK\r\nContent-Length: 4\r\n\r\nM83\n",
    ),
    (
        "a length that is no number",
        b"HTTP/1.1 200 OK\r\nContent-Length: lots\r\n\r\nM83\n",
    ),
    (
        "two lengths that disagree",
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Length: 9\r\n\r\nM83\n",
    ),
    (
        "a coding that only names chunked",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: notchunked\r\n\r\n4\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "a coding this client cannot undo",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\nM83\n",
    ),
    (
        "an HTTP version that is not one",
        b"HTTP/1.garbage 200 OK\r\nContent-Length: 4\r\n\r\nM83\n",
    ),
    (
        "a header line with no field name",
        b"HTTP/1.1 200 OK\r\nContent-Length 4\r\n\r\nM83\n",
    ),
    (
        "a header field name carrying a space",
        b"HTTP/1.1 200 OK\r\nContent Length: 4\r\n\r\nM83\n",
    ),
    (
        "a header field value carrying a control character",
        b"HTTP/1.1 200 OK\r\nX-Note: a\x01b\r\nContent-Length: 4\r\n\r\nM83\n",
    ),
    (
        "a trailer field with no name",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nM83\n\r\n0\r\n: x\r\n\r\n",
    ),
    (
        "a status that is not three digits",
        b"HTTP/1.1 0200 OK\r\nContent-Length: 4\r\n\r\nM83\n",
    ),
    (
        "a status carrying a sign",
        b"HTTP/1.1 +20 OK\r\nContent-Length: 4\r\n\r\nM83\n",
    ),
    (
        "a length carrying a sign",
        b"HTTP/1.1 200 OK\r\nContent-Length: +4\r\n\r\nM83\n",
    ),
    (
        "a chunk size carrying a sign",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n+4\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "both a length and chunks",
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\n\r\n\
              4\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "a trailer that is no field",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nM83\n\r\n0\r\nnonsense\r\n\r\n",
    ),
    (
        "a chunk extension with no name",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "a chunk extension carrying a control character",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;a\x01b\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "a chunk extension with no value after its `=`",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;a=\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "a chunk extension whose quoted value never closes",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;a=\"b\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "a chunk size followed by something that is no extension",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4 x\r\nM83\n\r\n0\r\n\r\n",
    ),
    (
        "a last chunk with no blank line after it",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nM83\n\r\n0\r\n",
    ),
];
