//! The supervising agent's own responder, authenticating the way a turn does.
//!
//! The responder this crate's integration tier points `OneHarness` at issues its
//! actions through the running API before it answers, and it finds where the
//! server is and what authenticates to it in the environment a turn is handed —
//! the address the server bound and the credential minted for that turn. That
//! tier runs it against a real `OctoPrint`; here it runs against a real server
//! over a recording machine, with a real turn held open for the credential to
//! be live, so that what the tier depends on is proven on every change: the
//! turn's own credential is served, a request under any other is refused before
//! it reaches the machine, and an environment naming nothing a header could
//! carry sends no request at all.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::process::{Command, Stdio};

use printobserver_server::context_command;
use printobserver_supervisor_api::{CREDENTIAL_ENV, SERVER_ENV};
use printobserver_types::PrintId;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::agent::Pass;
use crate::authenticating::history;
use crate::printer::Call;
use crate::world::{SECRET, World};

/// The one action every run here is scripted with: an adjustment the agent is
/// granted, inside the bounds the base configuration allows it.
fn slow_down() -> Value {
    json!([{
        "operation": "set_feedrate_factor",
        "body": { "reason": "the long edges are widening", "factor": 0.9 },
    }])
}

/// One action by operation name, with an empty request body.
fn action(operation: &str) -> Value {
    json!([{ "operation": operation, "body": {} }])
}

/// What declines to act on a turn: no context command, or no server and
/// credential in its environment.
const DECLINED: &str = "this turn's prompt names no context command, or its environment carries \
                        no server and credential";

/// Hold one turn about one print open, and answer what it was issued.
async fn a_live_turn(world: &World) -> (PrintId, Pass) {
    let print_id = world.open_print().await;
    world.agent.hold_turns();
    let status = reqwest::Client::new()
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(crate::world::failure_alert(4211, "http://127.0.0.1:1/frame.jpg").to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    let pass = world.agent.issued(1).await[0].clone();
    (print_id, pass)
}

/// Run the responder once, as `OneHarness` does, over a prompt naming the
/// context command for one print and an environment carrying one server and
/// one credential — and answer every line it wrote about what it did.
async fn respond(server: &str, credential: &str, print_id: PrintId, actions: Value) -> Vec<Value> {
    let prompt = format!(
        "Read this print's context by running {} and then act on it.",
        context_command().replace("{print_id}", &print_id.to_string())
    );
    let (server, credential) = (server.to_owned(), credential.to_owned());
    tokio::task::spawn_blocking(move || {
        let scratch = TempDir::new().expect("a run's own directory");
        let log = scratch.path().join("responder.log");
        let status = Command::new(env!("CARGO_BIN_EXE_printobserver-server-responder"))
            .arg("-p")
            .arg(&prompt)
            .env("PRINTOBSERVER_RESPONDER_LOG", &log)
            .env("PRINTOBSERVER_RESPONDER_ACTIONS", actions.to_string())
            .env(SERVER_ENV, server)
            .env(CREDENTIAL_ENV, credential)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("the responder runs");
        assert!(status.success(), "the responder exited {status}");
        std::fs::read_to_string(&log)
            .expect("the responder wrote what it did")
            .lines()
            .map(|line| {
                printobserver_types::serde_json::from_str(line)
                    .unwrap_or_else(|error| panic!("the responder wrote {line:?}: {error}"))
            })
            .collect()
    })
    .await
    .expect("the responder's run completes")
}

/// Under the environment its turn was handed, the responder's action is served
/// and reaches the machine.
#[tokio::test(flavor = "multi_thread")]
async fn the_responder_acts_under_the_credential_its_turn_was_issued() {
    let world = World::open().await;
    let (print_id, pass) = a_live_turn(&world).await;
    let held = history(&world.stores, print_id).await;
    world.printer.forget();

    let did = respond(
        pass.server
            .as_deref()
            .expect("the turn was told the address"),
        &pass.credential,
        print_id,
        slow_down(),
    )
    .await;

    assert_eq!(
        did.len(),
        1,
        "the responder did not issue its one action: {did:?}"
    );
    assert_eq!(
        did[0]["status"],
        json!(200),
        "the responder's action under its turn's credential was not served: {did:?}"
    );
    assert_eq!(did[0]["operation"], json!("set_feedrate_factor"));
    assert_eq!(
        world.printer.calls(),
        vec![Call::Feedrate(0.9)],
        "the responder's served action did not reach the machine"
    );
    assert!(
        history(&world.stores, print_id).await > held,
        "the responder's served action was not recorded"
    );
    world.agent.release_turns();
}

/// Under a credential that is not a live turn's the responder's action is
/// refused before it reaches anything, and under one no header could carry it
/// sends nothing at all.
#[tokio::test(flavor = "multi_thread")]
async fn the_responder_under_any_other_credential_reaches_nothing() {
    let world = World::open().await;
    let (print_id, pass) = a_live_turn(&world).await;
    let server = pass.server.clone().expect("the turn was told the address");
    let held = history(&world.stores, print_id).await;
    world.printer.forget();

    let did = respond(
        &server,
        "not-the-credential-in-force",
        print_id,
        slow_down(),
    )
    .await;
    assert_eq!(
        did.len(),
        1,
        "the responder did not issue its one action: {did:?}"
    );
    assert_eq!(
        did[0]["status"],
        json!(401),
        "the responder's action under a wrong credential was not refused: {did:?}"
    );
    assert!(
        did[0]["body"]["error"].is_string(),
        "the refusal the responder read is not the error body: {did:?}"
    );

    for (unpresentable, named) in [
        ("", "an empty credential"),
        ("   ", "a credential of spaces"),
        ("tab\tinside", "a credential carrying a control character"),
    ] {
        let did = respond(&server, unpresentable, print_id, slow_down()).await;
        assert_eq!(
            did,
            vec![json!({ "status": null, "refused": DECLINED })],
            "the responder under {named} did something other than decline to act"
        );
    }

    assert!(
        world.printer.calls().is_empty(),
        "a responder that did not present a live turn's credential reached the machine: {:?}",
        world.printer.calls()
    );
    assert_eq!(
        history(&world.stores, print_id).await,
        held,
        "a responder that did not present a live turn's credential was recorded"
    );
    world.agent.release_turns();
}

/// Every failure before a server can answer is reported by the responder
/// process itself, rather than hidden behind the harness answer it prints.
#[tokio::test(flavor = "multi_thread")]
async fn the_responder_reports_why_an_action_could_not_reach_a_server() {
    let world = World::open().await;
    let (print_id, pass) = a_live_turn(&world).await;
    let server = pass.server.clone().expect("the turn was told the address");
    let credential = pass.credential.clone();

    let unknown = respond(&server, &credential, print_id, action("not_an_operation")).await;
    assert_eq!(
        unknown[0]["refused"],
        json!("no such operation not_an_operation")
    );

    let unsupported = respond("https://127.0.0.1:9", &credential, print_id, slow_down()).await;
    assert_eq!(
        unsupported[0]["refused"],
        json!("https://127.0.0.1:9 is no address this responder speaks to")
    );

    let unused = TcpListener::bind("127.0.0.1:0").expect("an unused address is reserved");
    let unused_address = unused.local_addr().expect("the unused address reads");
    drop(unused);
    let refused = respond(
        &format!("http://{unused_address}"),
        &credential,
        print_id,
        slow_down(),
    )
    .await;
    assert_eq!(
        refused[0]["refused"],
        json!(format!("nothing is answering at {unused_address}"))
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("the malformed server binds");
    let address = listener
        .local_addr()
        .expect("the malformed server has an address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the responder connects");
        let mut reader = BufReader::new(&mut stream);
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("a request line reads");
            if line == "\r\n" {
                break;
            }
            if let Some(length) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = length.trim().parse().expect("content length is a number");
            }
        }
        let mut body = vec![0; content_length];
        reader
            .read_exact(&mut body)
            .expect("the request body reads");
        drop(reader);
        stream
            .write_all(b"not an HTTP answer")
            .expect("the malformed answer writes");
    });
    let unreadable = respond(
        &format!("http://{address}"),
        &credential,
        print_id,
        slow_down(),
    )
    .await;
    server.join().expect("the malformed server finishes");
    assert!(
        unreadable[0]["refused"]
            .as_str()
            .is_some_and(|message| message.contains("answered something unreadable")),
        "the malformed answer was not diagnosed: {unreadable:?}"
    );
    world.agent.release_turns();
}
