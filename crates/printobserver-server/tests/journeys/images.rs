//! An image reaches a caller as a path, and never as bytes.
//!
//! The server, the command-line program and the supervising agent all run on
//! the one host by design, so image transport is a filesystem path. Two things
//! are asserted here, and the second is why the first is not enough: a caller
//! that received a valid path **and** the encoded bytes beside it would satisfy
//! the first and would still be putting a snapshot through JSON.
//!
//! 1. Every answer that carries an image carries an **absolute path on this
//!    host** which exists and whose contents digest to the digest the record
//!    declares.
//! 2. **No response of any route this server serves** carries those bytes, or
//!    any encoding of them this repository can produce — the base64 its own
//!    `RawBytes` renders as, or their hexadecimal.

use printobserver_server::{Method, OPERATIONS, Operation};
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{ImageId, PrintId, RawBytes};
use sha2::{Digest as _, Sha256};

use crate::http_host::image_host;
use crate::ingress::snapshot_bytes;
use crate::world::{SECRET, World, failure_alert, manifest_write};

/// One alert with an image, handled to completion; answers the print and image.
async fn stored_image(world: &World, bytes: Vec<u8>) -> (PrintId, ImageId) {
    let host = image_host(bytes).await;
    let mut completions = world.server.completions();
    let status = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(failure_alert(4211, &host.url()).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");

    let print = world
        .store
        .print_by_obico_id(4211)
        .await
        .expect("the print reads")
        .expect("the alert opened a print");
    let events = world
        .store
        .history(printobserver_store_api::HistoryQuery {
            print_id: print.id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: None,
        })
        .await
        .expect("the history reads");
    let image = events
        .iter()
        .find_map(|event| event.image.clone())
        .expect("the alert's image was stored");
    (print.id, image.id)
}

/// The digest of what is at one path, lowercase hexadecimal.
fn digest_at(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut rendered, byte| {
            use core::fmt::Write as _;
            let _ = write!(rendered, "{byte:02x}");
            rendered
        })
}

/// An image answer is an absolute path whose contents are what the record says.
#[tokio::test(flavor = "multi_thread")]
async fn an_image_answers_a_path_whose_contents_are_what_the_record_declares() {
    let world = World::open().await;
    let (_, image_id) = stored_image(&world, snapshot_bytes()).await;

    let (status, answer) = world
        .get(&world.at(&path("image").replace("{image_id}", &image_id.to_string())))
        .await;

    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    let materialized = std::path::PathBuf::from(
        answer["path"]
            .as_str()
            .unwrap_or_else(|| panic!("the image answer carries no path: {answer}")),
    );
    assert!(
        materialized.is_absolute(),
        "the image answer is not an absolute path: {}",
        materialized.display()
    );
    assert!(
        materialized.exists(),
        "the image answer names {}, where nothing is",
        materialized.display()
    );
    assert_eq!(
        digest_at(&materialized),
        answer["record"]["sha256"]
            .as_str()
            .expect("the record declares a digest"),
        "what is at the path this server answered is not what the record declares"
    );
    world.server.stop().await;
}

/// No route this server serves answers image bytes, in any encoding.
#[tokio::test(flavor = "multi_thread")]
async fn no_route_this_server_serves_answers_image_bytes() {
    let bytes = snapshot_bytes();
    let world = World::open().await;
    let (print_id, image_id) = stored_image(&world, bytes.clone()).await;

    // Every spelling of those bytes this repository can produce: the bytes
    // themselves, the base64 its own `RawBytes` renders as, and hexadecimal.
    let base64 = RawBytes::new(bytes.clone()).to_string();
    let hexadecimal = bytes.iter().fold(String::new(), |mut rendered, byte| {
        use core::fmt::Write as _;
        let _ = write!(rendered, "{byte:02x}");
        rendered
    });
    let forbidden = [
        (
            "the bytes themselves",
            String::from_utf8_lossy(&bytes[3..]).into_owned(),
        ),
        ("their base64", base64),
        ("their hexadecimal", hexadecimal),
    ];

    for operation in OPERATIONS {
        let url = world
            .operation_url(&operation.full_path(), print_id)
            .replace("{image_id}", &image_id.to_string());
        let (_, answer) = match operation.method {
            Method::Get => world.get(&url).await,
            Method::Post => world.post(&url, &accepted_body(&operation)).await,
            Method::Put => world.put(&url, &manifest_write(&manifest())).await,
        };
        let rendered = answer.to_string();
        for (spelling, needle) in &forbidden {
            assert!(
                !rendered.contains(needle.as_str()),
                "`{}` answered {spelling} of a stored image",
                operation.name
            );
        }
    }
    world.server.stop().await;
}

/// One operation's whole path, by its name.
fn path(name: &str) -> String {
    printobserver_server::operation(name)
        .unwrap_or_else(|| panic!("`{name}` is served"))
        .full_path()
}

/// A manifest a write can carry.
fn manifest() -> Value {
    printobserver_types::serde_json::to_value(
        <printobserver_types::JobManifest as printobserver_types::contract::Sample>::sample_full(),
    )
    .expect("a manifest renders")
}

/// A body one mutating operation takes.
fn accepted_body(operation: &Operation) -> Value {
    let mut body = json!({ "reason": "a journey is asking", "actor": "operator" });
    let Some(kind) = operation.action_kind() else {
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
