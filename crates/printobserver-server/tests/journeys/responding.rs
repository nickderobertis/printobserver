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
        "body": { "reason": "the long edges are widening", "factor": 1.1 },
    }])
}

/// A copy of the client configuration a server wrote, with its credential
/// replaced, under a directory of the journey's own.
fn with_credential(written: &Path, credential: &str, beside: &Path) -> PathBuf {
    let mut document: toml::Table =
        toml::from_str(&std::fs::read_to_string(written).expect("the client configuration reads"))
            .expect("the client configuration is a document");
    document
        .get_mut("client")
        .and_then(toml::Value::as_table_mut)
        .expect("the client configuration has a `[client]` table")
        .insert("credential".to_owned(), toml::Value::from(credential));
    let path = beside.join(CLIENT_CONFIG_FILE);
    std::fs::write(
        &path,
        toml::to_string(&document).expect("the document renders"),
    )
    .expect("the copy is writable");
    path
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
        vec![Call::Feedrate(1.1)],
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
