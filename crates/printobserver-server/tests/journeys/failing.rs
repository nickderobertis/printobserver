//! What this server answers when something under it has failed.
//!
//! A caller that cannot tell "you asked for a print that does not exist" from
//! "the disk is full" cannot act on either: the first is a request to change
//! and the second is a machine to go and look at. So every read and every
//! mutating operation is driven against a store that has failed, and each is
//! asserted to answer this server's own failure carrying the store's own words.

use std::sync::Arc;

use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_server::{Method, OPERATIONS, Ports, Running, Server, ServerConfig, StartError};
use printobserver_store_api::StorePort;
use printobserver_types::PrintId;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::failing_store::{DETAIL, FailingStore};
use crate::printer::RecordingPrinter;
use crate::world::{document, manifest_write, write};

/// One server over the store given, in a root of its own.
async fn served(root: &std::path::Path, store: Arc<dyn StorePort>) -> Result<Running, StartError> {
    let path = write(root, &document(root, "http://127.0.0.1:1"));
    let config = ServerConfig::load(&path).expect("the configuration is accepted");
    Server::start_with(
        config,
        Ports {
            printer: RecordingPrinter::printing()
                as Arc<dyn printobserver_printer_api::PrinterPort>,
            store,
            vision: Arc::new(
                ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"),
            ),
            agent: StandInAgent::new() as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
}

/// A store that has already failed refuses the start rather than half-starting.
#[tokio::test(flavor = "multi_thread")]
async fn a_store_that_has_already_failed_refuses_the_start() {
    let root = TempDir::new().expect("a journey's own root");

    let started = served(root.path(), FailingStore::from_the_start()).await;

    let Err(refusal) = started else {
        panic!("a server came up over a store it could not read");
    };
    assert!(
        matches!(refusal, StartError::Reconciliation { .. }),
        "the refusal is not about what the store held: {refusal}"
    );
    assert!(
        refusal.to_string().contains(DETAIL),
        "the refusal does not carry the store's own words: {refusal}"
    );
}

/// Every operation answers this server's own failure when the store has failed.
#[tokio::test(flavor = "multi_thread")]
async fn every_operation_answers_this_servers_own_failure_when_the_store_has() {
    let root = TempDir::new().expect("a journey's own root");
    let server = served(root.path(), FailingStore::after_starting())
        .await
        .expect("a store that fails afterwards lets the server start");
    let client = reqwest::Client::new();
    let print_id = PrintId::new();

    for operation in OPERATIONS {
        let url = format!(
            "http://{}{}",
            server.address(),
            operation
                .full_path()
                .replace("{print_id}", &print_id.to_string())
                .replace(
                    "{image_id}",
                    &printobserver_types::ImageId::new().to_string()
                )
        );
        let request = match operation.method {
            Method::Get => client.get(&url),
            Method::Post => client.post(&url).json(&body(operation.action_kind())),
            Method::Put => client.put(&url).json(&manifest_write(&manifest())),
        };
        let response = request.send().await.expect("the server answers");
        let status = response.status();
        let answer: Value = response.json().await.expect("the answer is JSON");

        assert_eq!(
            status,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "`{}` answered {status} for a store that had failed: {answer}",
            operation.name
        );
        assert!(
            answer["error"]
                .as_str()
                .unwrap_or_default()
                .contains(DETAIL),
            "`{}` did not carry the store's own words: {answer}",
            operation.name
        );
    }
    server.stop().await;
}

/// A manifest a write can carry.
fn manifest() -> Value {
    printobserver_types::serde_json::to_value(
        <printobserver_types::JobManifest as printobserver_types::contract::Sample>::sample_full(),
    )
    .expect("a manifest renders")
}

/// A body one mutating operation takes, carrying whatever that action needs.
fn body(kind: Option<printobserver_types::ActionKind>) -> Value {
    let mut body = json!({ "reason": "a journey is asking", "actor": "operator" });
    let Some(kind) = kind else {
        return body;
    };
    let object = body.as_object_mut().expect("the body is an object");
    match kind {
        printobserver_types::ActionKind::StartPrint => {
            object.insert("file_name".to_owned(), json!("benchy.gcode"));
            object.insert("manifest".to_owned(), manifest());
        }
        printobserver_types::ActionKind::SetFeedrateFactor
        | printobserver_types::ActionKind::SetFlowrateFactor => {
            object.insert("factor".to_owned(), json!(1.0));
        }
        printobserver_types::ActionKind::SetToolTargetC => {
            object.insert("tool".to_owned(), json!(0));
            object.insert("target_c".to_owned(), json!(215.0));
        }
        printobserver_types::ActionKind::SetBedTargetC => {
            object.insert("target_c".to_owned(), json!(60.0));
        }
        printobserver_types::ActionKind::SetFanPercent => {
            object.insert("percent".to_owned(), json!(40.0));
        }
        printobserver_types::ActionKind::AcknowledgeFailure => {
            object.insert(
                "event_id".to_owned(),
                json!(printobserver_types::EventId::new()),
            );
            object.insert("disposition".to_owned(), json!("continue"));
        }
        _ => {}
    }
    body
}

/// A machine reporting something JSON cannot denote is a failure, not a
/// truncated answer.
///
/// The contracts refuse a non-finite reported value on emission rather than
/// letting a serializer write `null` for it, because `null` parses back as a
/// field the source did not report — and a supervision decision taken on "the
/// printer reported nothing" when the printer reported `NaN` is the wrong
/// decision. So the answer is this server's own failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_reading_json_cannot_denote_is_answered_as_a_failure() {
    let world = crate::world::World::open().await;
    let print_id = world.open_print().await;
    world.printer.reporting_nothing_denotable();

    let url = world.operation_url(
        &printobserver_server::operation("status")
            .expect("status is served")
            .full_path(),
        print_id,
    );
    let response = world
        .client
        .get(&url)
        .send()
        .await
        .expect("the server answers");

    assert_eq!(
        response.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "a reading JSON cannot denote was answered as an ordinary status"
    );
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some(printobserver_server::MEDIA_TYPE),
        "the failure was not answered as JSON"
    );
    world.server.stop().await;
}
