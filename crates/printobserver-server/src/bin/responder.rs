//! The deterministic responder `OneHarness` resolves to in this crate's tier.
//!
//! It delegates to `oneharness::mock_harness`, the responder `OneHarness` itself
//! publishes: everything from the run request down through argument
//! construction, parsing, the session store and schema validation is the real
//! engine, and only the paid provider process is this.
//!
//! # It acts before it answers, because that is what an agent does
//!
//! A supervising agent's turn is not a document it writes: it reads the print's
//! context and then asks this server for something. A responder that only
//! answered with an assessment would leave the whole loop — an alert reaching a
//! turn, a turn reaching the policy, the policy reaching the machine —
//! unexercised, and a journey that sent the action itself would be proving its
//! own arm rather than the agent's.
//!
//! So this responder issues its actions through the **running HTTP API**, as
//! the agent, before it answers. It finds the server and the print the way an
//! agent does: out of its own prompt, which carries the context command this
//! server told the turn to run.
//!
//! Every action it issues is appended to the file [`LOG`] names as **one JSON
//! object per line** — the operation, the status the server answered under, and
//! the answer's own body parsed back — so a journey reads what the agent did
//! and what it was told rather than matching on the text of an HTTP message.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

/// The variable naming the file this responder appends what it did to.
///
/// Absent means this responder acts as the published one alone, which is what
/// every tier but the whole-loop walk wants.
const LOG: &str = "PRINTOBSERVER_RESPONDER_LOG";

/// The variable naming the actions to issue, as a JSON array of
/// `{"operation": ..., "body": ...}` — the operation by the name this server
/// declares it under, and the body exactly as a caller sends it.
const ACTIONS: &str = "PRINTOBSERVER_RESPONDER_ACTIONS";

/// What the context command in a prompt begins with, which is what a turn is
/// told to run and what this responder finds its configuration and its print
/// in.
const CONTEXT_MARKER: &str = "printobserver context --config ";

/// The key the client configuration names the server under.
const SERVER_KEY: &str = "server = ";

/// How long one action this responder issues may take.
///
/// A supervising agent that hung on its own request would hang the turn, and a
/// turn that hangs is reported as a turn that took too long rather than as the
/// request that never came back. Bounded here so the failure says which it was.
///
/// Generous, because the request behind it reaches a real machine: one action
/// reads the printer and then asks it for something, and a printer part-way
/// through a dwell answers when the dwell does.
const BOUND: core::time::Duration = core::time::Duration::from_secs(90);

/// Where the supervisor is, and which print the turn is about.
struct Turn {
    /// The address the server is answering on.
    server: String,
    /// The print the turn is about.
    print: String,
}

/// The server and the print this turn's own prompt names.
///
/// The prompt reaches the harness on its argv, which is where a real one reads
/// it from too. A prompt that names no context command is a turn this responder
/// does not act on — it answers and nothing else.
fn turn_from_the_prompt() -> Option<Turn> {
    let prompt = std::env::args().find(|word| word.contains(CONTEXT_MARKER))?;
    let after = prompt.split(CONTEXT_MARKER).nth(1)?;
    let mut words = after.split_whitespace();
    // The command names a configuration file rather than an address, because
    // no client command of that program takes an address. This responder is
    // not that program, so it reads the one value it needs out of the file the
    // server wrote — which is where a real client reads it from too.
    let configuration = std::fs::read_to_string(words.next()?).ok()?;
    let server = configuration
        .lines()
        .find_map(|line| line.trim().strip_prefix(SERVER_KEY))?
        .trim()
        .trim_matches('"')
        .trim_end_matches('/')
        .to_owned();
    let print = words
        .skip_while(|word| *word != "--print-id")
        .nth(1)?
        .trim()
        .to_owned();
    Some(Turn { server, print })
}

/// One action this responder could not issue at all.
fn refused(detail: &str) -> serde_json::Value {
    serde_json::json!({ "status": serde_json::Value::Null, "refused": detail })
}

/// Issue one action, and answer the status and the body the server gave.
fn issue(turn: &Turn, operation: &str, body: &str) -> serde_json::Value {
    let Some(declared) = printobserver_server::operation(operation) else {
        return refused(&format!("no such operation {operation}"));
    };
    let path = declared.full_path().replace("{print_id}", &turn.print);
    let Some(authority) = turn.server.strip_prefix("http://") else {
        return refused(&format!(
            "{} is no address this responder speaks to",
            turn.server
        ));
    };
    let Ok(mut stream) = TcpStream::connect(authority) else {
        return refused(&format!("nothing is answering at {authority}"));
    };
    if stream.set_read_timeout(Some(BOUND)).is_err()
        || stream.set_write_timeout(Some(BOUND)).is_err()
    {
        return refused("this responder could not bound its own request");
    }
    if write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: {}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        printobserver_server::MEDIA_TYPE,
        body.len()
    )
    .is_err()
    {
        return refused("the request could not be sent");
    }
    let mut answer = String::new();
    if let Err(error) = stream.read_to_string(&mut answer) {
        return refused(&format!(
            "the answer could not be read: {error}; what came back was {answer:?}"
        ));
    }
    read_answer(&answer)
}

/// The status and the body of one HTTP answer, as a journey reads them.
fn read_answer(answer: &str) -> serde_json::Value {
    let Some((head, body)) = answer.split_once("\r\n\r\n") else {
        return refused(&format!(
            "the server answered something unreadable: {answer:?}"
        ));
    };
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok());
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    serde_json::json!({ "status": status, "body": parsed })
}

/// Issue every action this turn was scripted with, and write down what happened.
fn act() {
    let Ok(log) = std::env::var(LOG) else {
        return;
    };
    let scripted = std::env::var(ACTIONS).unwrap_or_default();
    let Some(turn) = turn_from_the_prompt() else {
        append(
            &log,
            &refused("this turn's prompt names no context command").to_string(),
        );
        return;
    };
    let actions: serde_json::Value = serde_json::from_str(&scripted).unwrap_or_default();
    for action in actions.as_array().into_iter().flatten() {
        let operation = action["operation"].as_str().unwrap_or_default();
        // The session the actor names is the one this print's turn runs in, and
        // the harness names it for the print — so the action the server records
        // carries the same session the assessment beside it does.
        let mut body = action["body"].clone();
        if let Some(object) = body.as_object_mut() {
            object.insert(
                "actor".to_owned(),
                serde_json::json!({ "agent": { "session_name": format!("print-{}", turn.print) } }),
            );
        }
        let started = std::time::Instant::now();
        let mut answered = issue(&turn, operation, &body.to_string());
        if let Some(object) = answered.as_object_mut() {
            object.insert("operation".to_owned(), serde_json::json!(operation));
            object.insert(
                "took_ms".to_owned(),
                serde_json::json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
            );
        }
        append(&log, &answered.to_string());
    }
}

/// Append one line to the file a journey reads what this responder did from.
fn append(log: &str, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(file, "{line}");
    }
}

fn main() {
    act();
    oneharness::mock_harness::run();
}
