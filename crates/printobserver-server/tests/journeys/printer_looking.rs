//! A look at the printer with no print named, when something on its path
//! fails, through the real route.
//!
//! Each journey opens a print, so that what a look at the printer writes can
//! be told apart from what an open print's history holds, and then asks
//! `GET /v1/look` with a camera, a store or a machine that will not do its
//! part. What is asserted is the answer, the printer's own history — the
//! events that belong to no print — and that the open print's history gained
//! nothing.

use std::sync::{Arc, OnceLock};

use printobserver_core::store::{HistoryQuery, StoredQuery};
use printobserver_core::{
    CameraDegradedPayload, CameraLookPayload, CameraRetryPayload, PortFailurePayload,
    PortFailureSite,
};
use printobserver_types::{EventPayload as _, EventRecord, PrintId};

use crate::agent::StandInAgent;
use crate::camera_host::{GO2RTC_FAILURE, Scripted, ScriptedHost};
use crate::held_events::HeldEvents;
use crate::ingress::snapshot_bytes;
use crate::printer::RecordingPrinter;
use crate::world::{World, set};

/// A world whose camera answers at `url`, under the stores as `between` stands
/// something in front of them.
async fn with_camera_over(
    url: String,
    between: impl FnOnce(printobserver_core::store::Stores) -> printobserver_core::store::Stores,
) -> World {
    World::configured_over(
        RecordingPrinter::printing(),
        StandInAgent::new(),
        |document| {
            let mut camera = toml::Table::new();
            camera.insert("snapshot_url".to_owned(), toml::Value::String(url));
            set(document, "camera", toml::Value::Table(camera));
        },
        between,
    )
    .await
}

/// A world whose camera answers at `url`.
async fn with_camera(url: String) -> World {
    with_camera_over(url, |stores| stores).await
}

/// Every event that belongs to no print, in the order it was stored.
async fn printers_own(world: &World) -> Vec<EventRecord> {
    world
        .stores
        .events
        .stored_after(StoredQuery {
            after: None,
            print_id: None,
            limit: Some(500),
        })
        .await
        .expect("the log reads")
        .into_iter()
        .filter(|event| event.print_id.is_none())
        .collect()
}

/// The kinds of an open print's history, newest first.
async fn print_kinds(world: &World, print_id: PrintId) -> Vec<String> {
    world
        .stores
        .events
        .history(HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: Some(100),
        })
        .await
        .expect("the history reads")
        .iter()
        .map(|event| event.kind().as_str().to_owned())
        .collect()
}

/// Whether any of these kinds is one a look writes.
fn carries_a_looks_kind(kinds: &[String]) -> bool {
    [
        CameraLookPayload::KIND,
        CameraRetryPayload::KIND,
        CameraDegradedPayload::KIND,
        PortFailurePayload::KIND,
    ]
    .iter()
    .any(|kind| kinds.iter().any(|held| held == kind))
}

/// The one port failure at the camera the printer's history holds.
fn camera_failure(events: &[EventRecord]) -> PortFailurePayload {
    let failures: Vec<PortFailurePayload> = events
        .iter()
        .filter_map(EventRecord::payload_as::<PortFailurePayload>)
        .map(|failure| failure.expect("a failure reads as one"))
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].site, PortFailureSite::CameraLook);
    failures[0].clone()
}

/// A camera refusing every attempt with a server error is asked four times;
/// each refusal but the last is a retry, the look is written down without a
/// frame, and the one port failure names it — all of them the printer's, none
/// of them in the open print's history.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_refusing_every_attempt_is_recorded_as_the_printers() {
    let camera = ScriptedHost::answering(vec![Scripted::exit_status_69()]).await;
    let world = with_camera(camera.url()).await;
    let print_id = world.open_print().await;

    let (status, look) = world.get(&world.url("/look")).await;

    assert_eq!(status, reqwest::StatusCode::OK, "{look}");
    assert!(look.get("frame").is_none(), "{look}");
    assert!(look.get("image_path").is_none(), "{look}");
    assert_eq!(camera.exchanges().len(), 4);
    let events = printers_own(&world).await;
    let retries: Vec<(u32, u16, String)> = events
        .iter()
        .filter_map(EventRecord::payload_as::<CameraRetryPayload>)
        .map(|retry| retry.expect("a retry reads as one"))
        .map(|retry| (retry.attempt, retry.status, retry.excerpt))
        .collect();
    assert_eq!(
        retries,
        (1..=3)
            .map(|attempt| (attempt, 500, GO2RTC_FAILURE.to_owned()))
            .collect::<Vec<_>>()
    );
    let failure = camera_failure(&events);
    assert_eq!(
        failure.event_id.to_string(),
        look["event"]["id"].as_str().expect("the look's own event")
    );
    assert!(!carries_a_looks_kind(&print_kinds(&world, print_id).await));
    world.server.stop().await;
}

/// A camera giving only blank frames is asked three times and the look is
/// written down as the printer's degraded camera, carrying no frame.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_giving_only_blank_frames_degrades_the_printers_look() {
    let grey = std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grey-frame.jpg"),
    )
    .expect("the grey frame reads");
    let camera = ScriptedHost::answering(vec![Scripted::frame(grey)]).await;
    let world = with_camera(camera.url()).await;
    let print_id = world.open_print().await;

    let (status, look) = world.get(&world.url("/look")).await;

    assert_eq!(status, reqwest::StatusCode::OK, "{look}");
    assert_eq!(look["event"]["kind"], CameraDegradedPayload::KIND, "{look}");
    assert!(look["event"].get("print_id").is_none(), "{look}");
    assert_eq!(look["event"]["payload"]["cause"], "blank", "{look}");
    assert!(look.get("frame").is_none(), "{look}");
    assert_eq!(camera.exchanges().len(), 3);
    let events = printers_own(&world).await;
    assert!(
        events
            .iter()
            .any(|event| event.kind().as_str() == CameraDegradedPayload::KIND),
        "the degraded look is not the printer's"
    );
    assert!(!carries_a_looks_kind(&print_kinds(&world, print_id).await));
    world.server.stop().await;
}

/// A frame the store will not keep leaves the look written down without it,
/// and the store's refusal recorded against it as the printer's.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_frame_the_store_will_not_keep_is_recorded_against_the_printers_look() {
    use std::os::unix::fs::PermissionsExt as _;

    let camera = ScriptedHost::answering(vec![Scripted::frame(snapshot_bytes())]).await;
    let world = with_camera(camera.url()).await;
    let print_id = world.open_print().await;
    let images = world.state_dir().join("images");
    std::fs::create_dir_all(&images).expect("the images directory");
    std::fs::set_permissions(&images, std::fs::Permissions::from_mode(0o500))
        .expect("the images directory can be made read-only");

    let (status, look) = world.get(&world.url("/look")).await;
    std::fs::set_permissions(&images, std::fs::Permissions::from_mode(0o700))
        .expect("the images directory can be made writable again");

    assert_eq!(status, reqwest::StatusCode::OK, "{look}");
    assert_eq!(look["event"]["kind"], CameraLookPayload::KIND, "{look}");
    assert!(look["event"].get("print_id").is_none(), "{look}");
    assert!(look.get("frame").is_none(), "{look}");
    let failure = camera_failure(&printers_own(&world).await);
    assert_eq!(
        failure.event_id.to_string(),
        look["event"]["id"].as_str().expect("the look's own event")
    );
    assert!(!carries_a_looks_kind(&print_kinds(&world, print_id).await));
    world.server.stop().await;
}

/// A printer that cannot be read leaves the look's printer and job absent, and
/// the look is still taken, framed and written down as the printer's.
#[tokio::test(flavor = "multi_thread")]
async fn a_printer_that_cannot_be_read_leaves_the_printers_look_framed() {
    let camera = ScriptedHost::answering(vec![Scripted::frame(snapshot_bytes())]).await;
    let world = with_camera(camera.url()).await;
    world.printer.unreadable(true);

    let (status, look) = world.get(&world.url("/look")).await;

    assert_eq!(status, reqwest::StatusCode::OK, "{look}");
    assert!(look.get("printer").is_none(), "{look}");
    assert!(look.get("job").is_none(), "{look}");
    assert!(look["frame"].is_object(), "{look}");
    assert!(look["event"].get("print_id").is_none(), "{look}");
    world.server.stop().await;
}

/// A look the store will not write down is refused as the store's failure,
/// and nothing of it is stored.
#[tokio::test(flavor = "multi_thread")]
async fn a_look_the_store_will_not_write_down_is_refused_and_nothing_is_kept() {
    let camera = ScriptedHost::answering(vec![Scripted::frame(snapshot_bytes())]).await;
    let held: Arc<OnceLock<Arc<HeldEvents>>> = Arc::default();
    let holding = Arc::clone(&held);
    let world = with_camera_over(camera.url(), move |stores| {
        let (stores, events) = HeldEvents::over(stores);
        events.refusing_kind(CameraLookPayload::KIND, true);
        let _ = holding.set(events);
        stores
    })
    .await;

    let (status, refused) = world.get(&world.url("/look")).await;

    assert!(
        held.get().is_some(),
        "the refusing store is not the server's"
    );
    assert_eq!(
        status,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "{refused}"
    );
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|said| said.contains(crate::held_events::EVENT_DETAIL)),
        "{refused}"
    );
    assert!(
        printers_own(&world)
            .await
            .iter()
            .all(|event| event.kind().as_str() != CameraLookPayload::KIND),
        "a refused look was kept"
    );
    world.server.stop().await;
}
