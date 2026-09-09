//! Restarting the service loses nothing, and what survives still acts.
//!
//! The store and the session directory are the whole of the state, so this
//! journey writes one of every kind of record this system holds — a print with
//! an open session, an unexpired intervention, a written manifest, a stored
//! image, and at least one event of **every kind the contracts declare** — and
//! then stops the server and starts another over the same state directory.
//!
//! Reading the records back is half of it. The other half is that they still
//! act: the print takes an action, the session continues rather than being
//! replaced, and the intervention expires on its own and puts the prior value
//! back through the ordinary policy.

use core::time::Duration;

use printobserver_store_api::{EventDraft, HistoryQuery};
use printobserver_types::contract::Sample as _;
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{Adjustable, EventKind, EventPayload, EventSource, PrintId, Timestamp};

use crate::http_host::image_host;
use crate::ingress::snapshot_bytes;
use crate::world::{SECRET, World, failure_alert};

/// How long the bounded intervention this journey opens stands for.
const DURATION_S: i64 = 5;

/// One operation's whole path, by its name.
fn path(name: &str) -> String {
    printobserver_server::operation(name)
        .unwrap_or_else(|| panic!("`{name}` is served"))
        .full_path()
}

/// Every kind the contracts declare, written against one print.
///
/// The payloads are the contracts' own canonical values, so a kind added to
/// the vocabulary is one this journey writes without anybody adding it here.
async fn write_every_kind(world: &World, print_id: PrintId) {
    let mut payloads = vec![EventPayload::sample_full()];
    payloads.extend(EventPayload::sample_alternates());
    assert_eq!(
        payloads.len(),
        EventKind::ALL.len(),
        "the contracts declare {} kinds and this journey writes {}",
        EventKind::ALL.len(),
        payloads.len()
    );
    for payload in payloads {
        world
            .store
            .append_event(EventDraft {
                print_id: Some(print_id),
                source: EventSource::System,
                received_at: Timestamp::now(),
                payload,
                raw: None,
            })
            .await
            .expect("an event is appended");
    }
}

/// The kinds one history carries, counted.
fn kinds(
    events: &[printobserver_types::EventRecord],
) -> std::collections::BTreeMap<EventKind, usize> {
    let mut found = std::collections::BTreeMap::new();
    for event in events {
        *found.entry(event.kind()).or_insert(0) += 1;
    }
    found
}

/// Deliver one alert about the same print, and wait its handling out.
async fn alert(world: &World, image_url: &str) {
    let mut completions = world.server.completions();
    let status = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(failure_alert(4211, image_url).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");
}

/// Everything one print held before the restart.
struct Written {
    /// The print itself.
    print_id: PrintId,
    /// What its status read.
    status: Value,
    /// Its whole history.
    events: Vec<printobserver_types::EventRecord>,
    /// What its image read.
    image: Value,
    /// What its manifest read.
    manifest: Value,
}

/// Write one of every kind of record this system holds, and read each back.
async fn write_everything(world: &World, image_url: &str) -> Written {
    alert(world, image_url).await;
    let print_id = world
        .store
        .print_by_obico_id(4211)
        .await
        .expect("the print reads")
        .expect("the alert opened a print")
        .id;

    let manifest =
        printobserver_types::serde_json::to_value(printobserver_types::JobManifest::sample_full())
            .expect("a manifest renders");
    let (status, _) = world
        .put(
            &world.operation_url(&path("manifest_set"), print_id),
            &manifest,
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);

    let (status, opened) = world
        .post(
            &world.operation_url(&path("set_feedrate_factor"), print_id),
            &json!({
                "reason": "the walls are thin",
                "actor": "operator",
                "factor": 1.2,
                "duration_s": DURATION_S,
            }),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{opened}");
    assert!(!opened["intervention"].is_null(), "{opened}");

    write_every_kind(world, print_id).await;

    let events = read_history(world, print_id).await;
    Written {
        print_id,
        status: world
            .get(&world.operation_url(&path("status"), print_id))
            .await
            .1,
        image: world
            .get(&world.at(&path("image").replace("{image_id}", &image_of(&events).to_string())))
            .await
            .1,
        manifest: world
            .get(&world.operation_url(&path("manifest_get"), print_id))
            .await
            .1,
        events,
    }
}

/// Every record survives the process that wrote it, and still acts afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn every_record_survives_a_restart_and_still_acts() {
    let host = image_host(snapshot_bytes()).await;
    let world = World::open().await;
    let written = write_everything(&world, &host.url()).await;
    let print_id = written.print_id;
    let before = written.status.clone();
    let events_before = written.events.clone();
    let image_before = written.image.clone();
    let manifest_before = written.manifest.clone();

    let world = world.restart().await;

    // Every kind the contracts declare is still there, and the history reads
    // back exactly as it was written.
    let events_after = read_history(&world, print_id).await;
    assert_eq!(
        kinds(&events_after).len(),
        EventKind::ALL.len(),
        "a restart lost a kind of event: {:?}",
        kinds(&events_after)
    );
    // The restart writes its own adoptions into the history, so what is
    // asserted is that every event written before it is still there and still
    // exactly as it was written.
    for written in &events_before {
        let found = events_after
            .iter()
            .find(|event| event.id == written.id)
            .unwrap_or_else(|| panic!("the restart lost event {}", written.id));
        assert_eq!(
            found, written,
            "event {} did not read back as it was written",
            written.id
        );
    }

    let after = world
        .get(&world.operation_url(&path("status"), print_id))
        .await
        .1;
    assert_eq!(
        after["print"], before["print"],
        "the print did not read back"
    );
    assert_eq!(
        after["session"], before["session"],
        "the session did not read back"
    );
    assert_eq!(
        after["interventions"], before["interventions"],
        "the intervention did not read back"
    );
    assert_eq!(
        world
            .get(&world.operation_url(&path("manifest_get"), print_id))
            .await
            .1,
        manifest_before,
        "the manifest did not read back"
    );

    // The stored image is still resolvable to a file whose contents match its
    // recorded digest.
    let image_after = world
        .get(&world.at(&path("image").replace("{image_id}", &image_of(&events_after).to_string())))
        .await
        .1;
    assert_eq!(
        image_after, image_before,
        "the image record did not read back"
    );
    let materialized = std::path::PathBuf::from(
        image_after["path"]
            .as_str()
            .expect("an image answers a path"),
    );
    assert_eq!(
        digest_at(&materialized),
        image_after["record"]["sha256"].as_str().expect("a digest"),
        "what survived the restart is not what the record declares"
    );

    still_acts(&world, print_id, &before, &host.url()).await;
    world.server.stop().await;
}

/// What survived the restart still acts: the print, the session and the
/// intervention.
async fn still_acts(world: &World, print_id: PrintId, status_before: &Value, image_url: &str) {
    // The print still acts: an action against it is accepted after the restart.
    let (status, acted) = world
        .post(
            &world.operation_url(&path("pause"), print_id),
            &json!({ "reason": "after the restart", "actor": "operator" }),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{acted}");

    // The session still acts: a second alert continues it rather than opening
    // a second one.
    let opened_name = status_before["session"]["session_name"].clone();
    alert(world, image_url).await;
    let continued = world
        .get(&world.operation_url(&path("status"), print_id))
        .await
        .1;
    assert_eq!(
        continued["session"]["session_name"], opened_name,
        "the second alert opened a second session rather than continuing the first"
    );

    // The intervention still acts: it expires on its own, and the value it
    // should restore goes back through the ordinary policy.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let active = world
            .store
            .active_interventions(print_id)
            .await
            .expect("the interventions read");
        if active.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the intervention outlived its own bound across the restart"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        world.printer.value_of(Adjustable::Feedrate),
        Some(1.0),
        "the intervention expired and the prior value did not go back"
    );
}

/// One print's whole history, newest first.
async fn read_history(world: &World, print_id: PrintId) -> Vec<printobserver_types::EventRecord> {
    world
        .store
        .history(HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: Some(printobserver_store_api::MAX_HISTORY_LIMIT),
        })
        .await
        .expect("the history reads")
}

/// The image one history carries.
fn image_of(events: &[printobserver_types::EventRecord]) -> printobserver_types::ImageId {
    events
        .iter()
        .find_map(|event| event.image.as_ref().map(|image| image.id))
        .expect("the history carries a stored image")
}

/// The digest of what is at one path, lowercase hexadecimal.
fn digest_at(path: &std::path::Path) -> String {
    use sha2::{Digest as _, Sha256};
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
