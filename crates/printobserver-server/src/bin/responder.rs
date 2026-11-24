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
//! the agent, before it answers. It finds the server and its credential the way
//! the command-line program a real turn runs does — in its environment, which
//! carries the address the server bound and the credential minted for this
//! turn — and the print out of its own prompt, which carries the context
//! command this server told the turn to run.
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
/// declares it under, and the body exactly as a caller sends it. A read, the
/// `context` a turn is told to run above all, is sent as a `GET` and its body
/// ignored, and what it answered is written down like any other.
const ACTIONS: &str = "PRINTOBSERVER_RESPONDER_ACTIONS";

/// What a scripted action's body names the turn's own event by, replaced with
/// the identifier of the event the turn's prompt names: what an acknowledgement
/// the turn makes by command is about.
const EVENT_PLACEHOLDER: &str = "{event_id}";

/// The variable naming the file this responder writes what it was started with
/// to: the directory it was run in and the system prompt it was handed, as one
/// JSON object.
///
/// Where the harness stands is what the skill's relative links resolve from,
/// and what it was handed as its system prompt is the skill's prose. The one
/// witness of either that is not the run request this server built is the
/// process that request started.
const SEEN: &str = "PRINTOBSERVER_RESPONDER_SEEN";

/// The variable naming a file this responder waits on, once its actions are
/// issued, for as long as the file exists, and at most [`BOUND`]: a turn held
/// open while a journey does what a person does during one.
const HOLD: &str = "PRINTOBSERVER_RESPONDER_HOLD";

/// The variable naming the file this responder appends one line to as a
/// compaction it was started for begins: its process id and when it began.
const COMPACT_LOG: &str = "PRINTOBSERVER_RESPONDER_COMPACT_LOG";

/// The variable naming a file a compaction this responder was started for
/// waits on, once it has begun, for as long as the file exists and at most
/// [`BOUND`]: a compaction still running when the next event arrives.
const COMPACT_HOLD: &str = "PRINTOBSERVER_RESPONDER_COMPACT_HOLD";

/// The variable naming, comma-separated, the environment variables whose
/// values this responder writes into what it records of a run: what the run's
/// harness process was really handed, beside the request that asked for it.
const SEEN_ENV: &str = "PRINTOBSERVER_RESPONDER_SEEN_ENV";

/// The variable naming a directory whose every file this responder writes into
/// what it records of a run, as it held them when the run's process started.
const SEEN_STORE: &str = "PRINTOBSERVER_RESPONDER_SEEN_STORE";

/// The prompt Claude Code is started with to compact a session.
const COMPACT_PROMPT: &str = "/compact";

/// How a system-prompt flag's value carries the prompt.
#[derive(Clone, Copy)]
enum Carried {
    /// The value is the prompt.
    Inline,
    /// The value names a file holding the prompt, which a prompt large enough
    /// to risk the argument ceiling is handed as.
    InFile,
}

/// The flags a Claude Code run is handed its system prompt under, each with
/// how it carries it: replacing Claude Code's own prompt or appended to it.
const SYSTEM_FLAGS: [(&str, Carried); 4] = [
    ("--system-prompt", Carried::Inline),
    ("--system-prompt-file", Carried::InFile),
    ("--append-system-prompt", Carried::Inline),
    ("--append-system-prompt-file", Carried::InFile),
];

/// What the context command in a prompt begins with, which is what a turn is
/// told to run and what this responder finds its print in.
const CONTEXT_MARKER: &str = "printobserver context ";

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
    /// The event the turn is about, as its prompt names it first.
    event: Option<String>,
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

/// One variable of this turn's environment, when it is something a request
/// head carries: not empty, and printable ASCII throughout.
fn from_the_environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| {
        !value.is_empty() && value.bytes().all(|byte| (b' '..=b'~').contains(&byte))
    })
}

/// The server and the credential this turn's environment carries, and the
/// print its own prompt names.
///
/// The environment is where a turn is handed both — the address the server
/// bound and the credential minted for this turn — and the context command it
/// is told to run names neither, which is why the program a real turn runs
/// reads them from there too. The prompt reaches the harness on its argv or,
/// for a run naming an `--input-format`, on its standard input, which is where
/// a real one reads it from. A turn that carries neither
/// variable, or whose prompt names no context command, is a turn this
/// responder does not act on — it answers and nothing else.
fn turn_from_the_prompt(stdin: &str) -> Option<Turn> {
    let prompt = std::env::args()
        .chain(prompt_texts(stdin))
        .find(|word| word.contains(CONTEXT_MARKER))?;
    let after = prompt.split(CONTEXT_MARKER).nth(1)?;
    let server = from_the_environment(printobserver_supervisor_api::SERVER_ENV)?
        .trim_end_matches('/')
        .to_owned();
    let credential = Credential::admitted(from_the_environment(
        printobserver_supervisor_api::CREDENTIAL_ENV,
    )?)?;
    let print = after
        .split_whitespace()
        .skip_while(|word| *word != "--print-id")
        .nth(1)?
        .trim()
        .to_owned();
    // The event's first field, written compact or pretty.
    let event = prompt
        .split_once("\"id\":")
        .and_then(|(_, rest)| rest.trim_start().strip_prefix('"'))
        .and_then(|rest| rest.split_once('"'))
        .map(|(id, _)| id.to_owned());
    Some(Turn {
        server,
        credential,
        print,
        event,
    })
}

/// Everything a run handed this responder on its standard input, read once and
/// only for a run naming an `--input-format`: a run that names none writes no
/// prompt there, and reading it would wait on input nobody sends. Input that
/// cannot be read whole, or is no text, stops this responder saying so rather
/// than acting on part of a prompt.
fn standard_input() -> String {
    if !std::env::args().any(|argument| argument == "--input-format") {
        return String::new();
    }
    let mut read = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut read) {
        eprintln!("printobserver-server-responder: its standard input could not be read: {error}");
        std::process::exit(1);
    }
    read
}

/// The prompt texts standard input carries: each text block of a stream-json
/// user frame, or the whole input when it is no frame.
fn prompt_texts(stdin: &str) -> Vec<String> {
    let mut texts = Vec::new();
    for line in stdin.lines() {
        let Ok(frame) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        for block in frame["message"]["content"].as_array().into_iter().flatten() {
            if let Some(text) = block["text"].as_str() {
                texts.push(text.to_owned());
            }
        }
    }
    if texts.is_empty() && !stdin.is_empty() {
        texts.push(stdin.to_owned());
    }
    texts
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
    // A read is sent as one, with no body: the context a turn reads is a
    // `GET`, and what it answers is what this turn was given to reason about.
    let sent = if declared.method == printobserver_server::Method::Get {
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {}\r\n\
             Connection: close\r\n\r\n",
            turn.credential.0
        )
    } else {
        write!(
            stream,
            "{} {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: {}\r\n\
             Content-Length: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n{body}",
            declared.method.as_str(),
            printobserver_server::MEDIA_TYPE,
            body.len(),
            turn.credential.0
        )
    };
    if sent.is_err() {
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

/// Write down that a compaction began, and hold it while [`COMPACT_HOLD`]
/// names a file that exists. What it answers is the published responder's.
fn compacting() {
    if let Ok(log) = std::env::var(COMPACT_LOG) {
        let began = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        append(
            &log,
            &serde_json::json!({ "pid": std::process::id(), "began_ms": began }).to_string(),
        );
    }
    let Some(held) = std::env::var_os(COMPACT_HOLD).map(std::path::PathBuf::from) else {
        return;
    };
    let started = std::time::Instant::now();
    while held.exists() && started.elapsed() < BOUND {
        std::thread::sleep(core::time::Duration::from_millis(20));
    }
}

/// Issue every action this turn was scripted with, and write down what happened.
fn act() {
    let stdin = standard_input();
    record_what_was_seen(&stdin);
    if std::env::args().any(|argument| argument == COMPACT_PROMPT) {
        compacting();
        return;
    }
    let Ok(log) = std::env::var(LOG) else {
        return;
    };
    let scripted = std::env::var(ACTIONS).unwrap_or_default();
    let Some(turn) = turn_from_the_prompt(&stdin) else {
        append(
            &log,
            &refused(
                "this turn's prompt names no context command, or its environment carries no \
                 server and credential",
            )
            .to_string(),
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
        let reading = printobserver_server::operation(operation)
            .is_some_and(|declared| declared.method == printobserver_server::Method::Get);
        if !reading && let Some(object) = body.as_object_mut() {
            object.insert(
                "actor".to_owned(),
                serde_json::json!({ "agent": { "session_name": format!("print-{}", turn.print) } }),
            );
        }
        let started = std::time::Instant::now();
        let sent = body
            .to_string()
            .replace(EVENT_PLACEHOLDER, turn.event.as_deref().unwrap_or_default());
        let mut answered = issue(&turn, operation, &sent);
        if let Some(object) = answered.as_object_mut() {
            object.insert("operation".to_owned(), serde_json::json!(operation));
            object.insert(
                "took_ms".to_owned(),
                serde_json::json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
            );
        }
        append(&log, &answered.to_string());
    }
    hold();
}

/// Wait while the file [`HOLD`] names exists, and at most [`BOUND`].
fn hold() {
    let Some(held) = std::env::var_os(HOLD).map(std::path::PathBuf::from) else {
        return;
    };
    let started = std::time::Instant::now();
    while held.exists() && started.elapsed() < BOUND {
        std::thread::sleep(core::time::Duration::from_millis(20));
    }
}

/// Write down where this responder was run, the system prompt it was handed
/// and what it was handed on its standard input.
fn record_what_was_seen(stdin: &str) {
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
    let handed = SYSTEM_FLAGS
        .iter()
        .find_map(|&(flag, carried)| after(flag).map(|value| (carried, value)));
    let system = match handed {
        Some((Carried::Inline, inline)) => Ok(inline.clone()),
        Some((Carried::InFile, path)) => std::fs::read_to_string(path)
            .map_err(|error| format!("the system prompt file {path} is unreadable: {error}")),
        None => Err("the run handed over no system prompt".to_owned()),
    };
    let here = std::env::current_dir()
        .map(|directory| directory.display().to_string())
        .map_err(|error| format!("the working directory is unreadable: {error}"));
    let document = serde_json::json!({
        "cwd": here.as_ref().ok(),
        "cwd_error": here.as_ref().err(),
        "system": system.as_ref().ok(),
        "system_error": system.as_ref().err(),
        // Everything after the program's own name, and the search path the
        // turn's commands would be found on.
        "arguments": arguments.get(1..).unwrap_or_default(),
        "stdin": stdin,
        "path": std::env::var("PATH").ok(),
        "env": std::env::var(SEEN_ENV)
            .unwrap_or_default()
            .split(',')
            .filter(|name| !name.is_empty())
            .map(|name| (name.to_owned(), serde_json::json!(std::env::var(name).ok())))
            .collect::<serde_json::Map<_, _>>(),
        "store": std::env::var_os(SEEN_STORE).map(|root| {
            let mut held = serde_json::Map::new();
            held_files(std::path::Path::new(&root), std::path::Path::new(&root), &mut held);
            held
        }),
    });
    // A record nobody can read is a journey that cannot see what it asserts
    // on, so failing to write one ends the run rather than passing unseen.
    if let Err(error) = std::fs::write(&record, document.to_string()) {
        eprintln!("printobserver-server-responder: {record} could not be written: {error}");
        std::process::exit(1);
    }
    // Every run's record is kept too, one per line beside it, so a journey
    // over several turns reads each turn's rather than only the last.
    append(&format!("{record}.lines"), &document.to_string());
}

/// Every file under `directory`, keyed by its path below `root`, with what it
/// holds.
fn held_files(
    root: &std::path::Path,
    directory: &std::path::Path,
    held: &mut serde_json::Map<String, serde_json::Value>,
) {
    for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            held_files(root, &path, held);
        } else {
            let key = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            held.insert(
                key,
                serde_json::json!(std::fs::read_to_string(&path).unwrap_or_default()),
            );
        }
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
