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

/// The variable naming the file this responder writes what it was started with
/// to: the directory it was run in and the system prompt it was handed, as one
/// JSON object.
///
/// Where the harness stands is what the skill's relative links resolve from,
/// and what it was handed as its system prompt is the skill's prose. The one
/// witness of either that is not the run request this server built is the
/// process that request started.
const SEEN: &str = "PRINTOBSERVER_RESPONDER_SEEN";

/// The flags a Claude Code run is handed its system prompt under: inline, or
/// as a file when the prompt is large enough to risk the argument ceiling.
const SYSTEM_FLAGS: (&str, &str) = ("--append-system-prompt", "--append-system-prompt-file");

/// What the context command in a prompt begins with, which is what a turn is
/// told to run and what this responder finds its configuration and its print
/// in.
const CONTEXT_MARKER: &str = "printobserver context --config ";

/// The table the client configuration names the server and the credential in.
const CLIENT_TABLE: &str = "client";

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

/// Where the supervisor is, what authenticates to it, and which print the turn
/// is about.
struct Turn {
    /// The address the server is answering on.
    server: String,
    /// The credential it serves under.
    credential: Credential,
    /// The print the turn is about.
    print: String,
}

/// A credential this responder presents, held only once the server's own rule
/// admits it as one.
struct Credential(String);

impl Credential {
    /// The credential a text names, when [`printobserver_server::ApiCredential`]
    /// admits it — so what this responder puts into a request head is held to
    /// the one rule the server holds its own credential to.
    fn admitted(text: String) -> Option<Self> {
        printobserver_server::ApiCredential::new(&text).ok()?;
        Some(Self(text))
    }
}

/// One text value of the client configuration's `[client]` table.
///
/// Read as the TOML document the server wrote rather than line by line, so a
/// credential carrying a character TOML escapes is read as the credential
/// rather than as its escaped spelling. A value that is empty, or that carries
/// anything outside printable ASCII, is not one this responder puts into a
/// request head, and is taken as absent.
fn client_value(configuration: &str, key: &str) -> Option<String> {
    let document: toml::Table = toml::from_str(configuration).ok()?;
    document
        .get(CLIENT_TABLE)?
        .get(key)?
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.bytes().all(|byte| (b' '..=b'~').contains(&byte))
        })
        .map(str::to_owned)
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
    // not that program, so it reads the two values it needs out of the file the
    // server wrote — which is where a real client reads them from too.
    let configuration = std::fs::read_to_string(words.next()?).ok()?;
    let server = client_value(&configuration, "server")?
        .trim_end_matches('/')
        .to_owned();
    let credential = Credential::admitted(client_value(&configuration, "credential")?)?;
    let print = words
        .skip_while(|word| *word != "--print-id")
        .nth(1)?
        .trim()
        .to_owned();
    Some(Turn {
        server,
        credential,
        print,
    })
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
         Content-Length: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n{body}",
        printobserver_server::MEDIA_TYPE,
        body.len(),
        turn.credential.0
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
    record_what_was_seen();
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

/// Write down where this responder was run and the system prompt it was handed.
fn record_what_was_seen() {
    let Ok(record) = std::env::var(SEEN) else {
        return;
    };
    let arguments: Vec<String> = std::env::args().collect();
    let after = |flag: &str| {
        arguments
            .iter()
            .position(|argument| argument == flag)
            .and_then(|at| arguments.get(at + 1))
    };
    // Each failure is written down beside the value it cost, so a journey that
    // finds no value reads why rather than a bare absence.
    let system = match (after(SYSTEM_FLAGS.0), after(SYSTEM_FLAGS.1)) {
        (Some(inline), _) => Ok(inline.clone()),
        (None, Some(path)) => std::fs::read_to_string(path)
            .map_err(|error| format!("the system prompt file {path} is unreadable: {error}")),
        (None, None) => Err("the run handed over no system prompt".to_owned()),
    };
    let here = std::env::current_dir()
        .map(|directory| directory.display().to_string())
        .map_err(|error| format!("the working directory is unreadable: {error}"));
    let document = serde_json::json!({
        "cwd": here.as_ref().ok(),
        "cwd_error": here.as_ref().err(),
        "system": system.as_ref().ok(),
        "system_error": system.as_ref().err(),
    });
    // A record nobody can read is a journey that cannot see what it asserts
    // on, so failing to write one ends the run rather than passing unseen.
    if let Err(error) = std::fs::write(&record, document.to_string()) {
        eprintln!("printobserver-server-responder: {record} could not be written: {error}");
        std::process::exit(1);
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

#[cfg(not(windows))]
fn main() {
    act();
    oneharness::mock_harness::run();
}

/// Run the exit-oriented harness below a wrapper that can flush its profile.
///
/// `mock_harness::run` ends with `process::exit`, which bypasses LLVM's Windows
/// profile writer. An instrumented responder therefore performs its action in
/// this normally-returning process and delegates only the harness protocol to
/// a child. Outside coverage, the responder keeps its ordinary one-process
/// path.
#[cfg(windows)]
fn main() -> std::process::ExitCode {
    const INNER: &str = "PRINTOBSERVER_RESPONDER_INNER";
    if std::env::var_os(INNER).is_some() {
        oneharness::mock_harness::run();
    }
    if std::env::var_os("LLVM_PROFILE_FILE").is_none() {
        act();
        oneharness::mock_harness::run();
    }

    act();
    let status = std::process::Command::new(
        std::env::current_exe().expect("the responder's executable has a path"),
    )
    .args(std::env::args_os().skip(1))
    .env(INNER, "1")
    .status()
    .expect("the responder's harness child runs");
    std::process::ExitCode::from(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1),
    )
}
