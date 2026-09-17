//! The supervising agent's own responder, authenticating the way a turn does.
//!
//! The responder this crate's integration tier points `OneHarness` at issues its
//! actions through the running API before it answers, and it finds where the
//! server is and what authenticates to it in the client configuration the
//! server wrote — the file a turn's context command names. That tier runs it
//! against a real `OctoPrint`; here it runs against a real server over a
//! recording machine, so that what the tier depends on is proven on every
//! change: the credential it reads is the one in force and is served, a request
//! under any other is refused before it reaches the machine, and a
//! configuration naming nothing a header could carry sends no request at all.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use printobserver_server::{CLIENT_CONFIG_FILE, context_command};
use printobserver_types::PrintId;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::authenticating::history;
use crate::printer::Call;
use crate::world::World;

/// The one action every run here is scripted with: an adjustment the agent is
/// granted, inside the bounds the base configuration allows it.
fn slow_down() -> Value {
    json!([{
        "operation": "set_feedrate_factor",
        "body": { "reason": "the long edges are widening", "factor": 0.9 },
    }])
}

/// Written beside the server's own file rather than over it, so that every
/// later run in a journey still reads what the server wrote.
fn with_credential(written: &Path, credential: &str, beside: &Path) -> PathBuf {
    with_client_value(written, "credential", credential, beside)
}

/// Copy the server-written client configuration with one value changed.
fn with_client_value(written: &Path, key: &str, value: &str, beside: &Path) -> PathBuf {
    let mut document: toml::Table =
        toml::from_str(&std::fs::read_to_string(written).expect("the client configuration reads"))
            .expect("the client configuration is a document");
    document
        .get_mut("client")
        .and_then(toml::Value::as_table_mut)
        .expect("the client configuration has a `[client]` table")
        .insert(key.to_owned(), toml::Value::from(value));
    let path = beside.join(CLIENT_CONFIG_FILE);
    std::fs::write(
        &path,
        toml::to_string(&document).expect("the document renders"),
    )
    .expect("the copy is writable");
    path
}

/// One action by operation name, with an empty request body.
fn action(operation: &str) -> Value {
    json!([{ "operation": operation, "body": {} }])
}

/// Run the responder once, as `OneHarness` does, over a prompt naming the
/// context command for one client configuration and one print — and answer
/// every line it wrote about what it did.
async fn respond(client_config: &Path, print_id: PrintId, actions: Value) -> Vec<Value> {
    let prompt = format!(
        "Read this print's context by running {} and then act on it.",
        context_command(client_config).replace("{print_id}", &print_id.to_string())
    );
    tokio::task::spawn_blocking(move || {
        let scratch = TempDir::new().expect("a run's own directory");
        let log = scratch.path().join("responder.log");
        let status = Command::new(env!("CARGO_BIN_EXE_printobserver-server-responder"))
            .arg("-p")
            .arg(&prompt)
            .env("PRINTOBSERVER_RESPONDER_LOG", &log)
            .env("PRINTOBSERVER_RESPONDER_ACTIONS", actions.to_string())
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

/// Under the client configuration the server wrote, the responder's action is
/// served and reaches the machine.
#[tokio::test(flavor = "multi_thread")]
async fn the_responder_acts_under_the_credential_the_server_wrote_for_it() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let held = history(&world.stores, print_id).await;
    world.printer.forget();

    let did = respond(
        &world.state_dir().join(CLIENT_CONFIG_FILE),
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
        "the responder's action under the credential in force was not served: {did:?}"
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
}

/// Under a credential that is not the one in force the responder's action is
/// refused before it reaches anything, and under one no header could carry it
/// sends nothing at all.
#[tokio::test(flavor = "multi_thread")]
async fn the_responder_under_any_other_credential_reaches_nothing() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let held = history(&world.stores, print_id).await;
    world.printer.forget();
    let written = world.state_dir().join(CLIENT_CONFIG_FILE);

    let wrong = TempDir::new().expect("a directory of the journey's own");
    let did = respond(
        &with_credential(&written, "not-the-credential-in-force", wrong.path()),
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
        let beside = TempDir::new().expect("a directory of the journey's own");
        let did = respond(
            &with_credential(&written, unpresentable, beside.path()),
            print_id,
            slow_down(),
        )
        .await;
        assert_eq!(
            did,
            vec![json!({
                "status": null,
                "refused": "this turn's prompt names no context command",
            })],
            "the responder under {named} did something other than decline to act"
        );
    }

    assert!(
        world.printer.calls().is_empty(),
        "a responder that did not present the credential in force reached the machine: {:?}",
        world.printer.calls()
    );
    assert_eq!(
        history(&world.stores, print_id).await,
        held,
        "a responder that did not present the credential in force was recorded"
    );
}

/// Every failure before a server can answer is reported by the responder
/// process itself, rather than hidden behind the harness answer it prints.
#[tokio::test(flavor = "multi_thread")]
async fn the_responder_reports_why_an_action_could_not_reach_a_server() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let written = world.state_dir().join(CLIENT_CONFIG_FILE);

    let unknown = respond(&written, print_id, action("not_an_operation")).await;
    assert_eq!(
        unknown[0]["refused"],
        json!("no such operation not_an_operation")
    );

    let https = TempDir::new().expect("a directory of the journey's own");
    let https_config = with_client_value(&written, "server", "https://127.0.0.1:9", https.path());
    let unsupported = respond(&https_config, print_id, slow_down()).await;
    assert_eq!(
        unsupported[0]["refused"],
        json!("https://127.0.0.1:9 is no address this responder speaks to")
    );

    let unused = TcpListener::bind("127.0.0.1:0").expect("an unused address is reserved");
    let unused_address = unused.local_addr().expect("the unused address reads");
    drop(unused);
    let unavailable = TempDir::new().expect("a directory of the journey's own");
    let unavailable_config = with_client_value(
        &written,
        "server",
        &format!("http://{unused_address}"),
        unavailable.path(),
    );
    let refused = respond(&unavailable_config, print_id, slow_down()).await;
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
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).expect("the request reads");
        stream
            .write_all(b"not an HTTP answer")
            .expect("the malformed answer writes");
    });
    let malformed = TempDir::new().expect("a directory of the journey's own");
    let malformed_config = with_client_value(
        &written,
        "server",
        &format!("http://{address}"),
        malformed.path(),
    );
    let unreadable = respond(&malformed_config, print_id, slow_down()).await;
    server.join().expect("the malformed server finishes");
    assert!(
        unreadable[0]["refused"]
            .as_str()
            .is_some_and(|message| message.contains("answered something unreadable")),
        "the malformed answer was not diagnosed: {unreadable:?}"
    );
}
