//! The real ingress path, driven with what the producer sends.
//!
//! Every journey here posts a body through [`ObicoIngress::receive`] — the path
//! the server's webhook route will call — against a store it reads back
//! through and an image host it controls, and asserts on what the store holds
//! afterwards. Nothing is mocked: the adapter's own HTTP client fetches from a
//! real server on the loopback address, so the bounds it runs under are the
//! ones under test rather than ones a stub agreed to.
//!
//! The committed samples under `printobserver-types` are what the producer
//! sends, and this tier is what holds this crate to them. Where a journey needs
//! the snapshot actually fetched it re-points `img_url` at the host the test
//! controls — that one field, and nothing else, because no test can reach the
//! address the committed sample names.

#[path = "support/image_host.rs"]
mod image_host;
#[path = "support/store.rs"]
mod store;

use core::time::Duration;
use std::path::PathBuf;
use std::sync::Arc;

use image_host::{Answer, ImageHost, unreachable_url};
use printobserver_obico::{
    DEFAULT_FETCH_TIMEOUT, DEFAULT_MAX_IMAGE_BYTES, IngressError, ObicoIngress, ObicoVisionConfig,
    Receipt,
};
use printobserver_store_api::{HistoryQuery, ImageLookup, StorePort};
use printobserver_types::serde_json::{self, Value, json};
use printobserver_types::{
    EventKind, EventPayload, EventRecord, EventSource, ImageRecord, ObicoFailureAlertPayload,
    ObicoNotificationType, ObicoPrinterNotificationPayload, PrintRecord, PrinterState, RawBytes,
    Timestamp,
};
use printobserver_vision_api::VisionError;
use store::MemoryStore;

/// Obico's own identifier for the print every committed sample is about.
const SAMPLE_OBICO_PRINT_ID: i64 = 4211;

/// The bytes of a committed sample, exactly as they are on disk.
fn sample_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("printobserver-types")
        .join("samples")
        .join("obico")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// One committed sample, parsed so a journey can vary one field of it.
fn sample(name: &str) -> Value {
    serde_json::from_slice(&sample_bytes(name)).expect("a committed sample is JSON")
}

/// The failure alert the producer sends.
fn failure_alert() -> Value {
    sample("failure-alert.json")
}

/// One body's bytes, as the ingress receives them.
fn body_of(value: &Value) -> RawBytes {
    RawBytes::new(serde_json::to_vec(value).expect("a body serializes"))
}

/// The bounds a journey that is not about a bound runs under.
fn prompt_bounds() -> ObicoVisionConfig {
    ObicoVisionConfig {
        fetch_timeout: Duration::from_secs(2),
        ..ObicoVisionConfig::default()
    }
}

/// The ingress, over one store, under one set of bounds.
fn ingress(store: &Arc<MemoryStore>, config: ObicoVisionConfig) -> ObicoIngress {
    ObicoIngress::new(Arc::clone(store) as Arc<dyn StorePort>, config).expect("the ingress builds")
}

/// The failure payload one event carries.
fn failure_payload(event: &EventRecord) -> &ObicoFailureAlertPayload {
    match &event.payload {
        EventPayload::ObicoFailureAlert(payload) => payload,
        other => panic!("expected a failure alert, found {other:?}"),
    }
}

/// The notification payload one event carries.
fn notification_payload(event: &EventRecord) -> &ObicoPrinterNotificationPayload {
    match &event.payload {
        EventPayload::ObicoPrinterNotification(payload) => payload,
        other => panic!("expected a printer notification, found {other:?}"),
    }
}

/// Read one print's events back out of the store, newest first.
async fn history_of(store: &MemoryStore, print: &PrintRecord) -> Vec<EventRecord> {
    store
        .history(HistoryQuery {
            print_id: print.id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: None,
        })
        .await
        .expect("the store answers a history read")
}

/// The event the store holds for one receipt, read back through the store.
fn stored_event(store: &MemoryStore, receipt: &Receipt) -> EventRecord {
    store
        .events()
        .into_iter()
        .find(|record| record.id == receipt.event.id)
        .expect("the store holds the event the receipt names")
}

/// The image the store holds, and the bytes at the path it names.
async fn stored_image(store: &MemoryStore, record: &ImageRecord) -> (ImageRecord, Vec<u8>) {
    match store.image(record.id).await.expect("the store answers") {
        ImageLookup::Found { record, path } => {
            let bytes = std::fs::read(&path).expect("the image's bytes are where the record says");
            (record, bytes)
        }
        ImageLookup::FileMissing { record } => {
            panic!("the store holds {record:?} with no file behind it")
        }
    }
}

/// The one failure recorded beside an event, read back out of the store.
///
/// It is an event of its own against the same print, so it is found the way
/// any consumer would find it: by reading that print's history.
async fn recorded_failure(store: &MemoryStore, receipt: &Receipt) -> EventRecord {
    let print = receipt.print.clone().expect("the alert named a print");
    let mut malformed: Vec<EventRecord> = history_of(store, &print)
        .await
        .into_iter()
        .filter(|record| record.kind() == EventKind::MalformedExternalEvent)
        .collect();
    assert_eq!(
        malformed.len(),
        1,
        "expected exactly one recorded failure beside the event"
    );
    malformed.pop().expect("one recorded failure")
}

/// What a recorded failure says.
fn failure_detail(record: &EventRecord) -> String {
    match &record.payload {
        EventPayload::MalformedExternalEvent(payload) => payload.detail.clone(),
        other => panic!("expected a malformed external event, found {other:?}"),
    }
}

/// A small snapshot, of the shape and size a printer's camera produces.
fn snapshot() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.extend(std::iter::repeat_n(0x5A_u8, 4_096));
    bytes.extend([0xFF, 0xD9]);
    bytes
}

// --- The failure alert, and the two flags it carries -------------------------

/// Every one of the four flag combinations is read off the body.
///
/// One of the four is the committed sample posted byte for byte, and the other
/// three are it with one or both flags varied. An implementation answering the
/// sample's own flag values whatever arrives fails three of the four, and one
/// recording any source but Obico fails all four.
#[tokio::test]
async fn each_of_the_four_flag_combinations_is_carried_from_the_body() {
    let host = ImageHost::serving(Answer::image(snapshot())).await;
    for (is_warning, print_paused) in [(false, true), (true, true), (false, false), (true, false)] {
        let committed = !is_warning && print_paused;
        let body = if committed {
            RawBytes::new(sample_bytes("failure-alert.json"))
        } else {
            let mut varied = failure_alert();
            varied["event"]["is_warning"] = json!(is_warning);
            varied["event"]["print_paused"] = json!(print_paused);
            varied["img_url"] = json!(host.snapshot_url());
            body_of(&varied)
        };

        let store = Arc::new(MemoryStore::new());
        let receipt = ingress(&store, prompt_bounds())
            .receive(body.clone(), Some("application/json".to_owned()))
            .await
            .expect("the alert is accepted");

        let held = stored_event(&store, &receipt);
        assert_eq!(held.kind(), EventKind::ObicoFailureAlert);
        assert_eq!(held.source, EventSource::Obico);
        assert_eq!(
            held.raw.as_ref().map(RawBytes::as_slice),
            Some(body.as_slice())
        );
        let payload = failure_payload(&held);
        assert_eq!(
            (payload.is_warning, payload.print_paused),
            (is_warning, print_paused),
            "the flags were not read off the body"
        );
    }
}

// --- The printer notification, in both of the forms it is sent in ------------

/// The notification about a print names one, and its snapshot is stored.
#[tokio::test]
async fn the_notification_about_a_print_names_it_and_stores_its_snapshot() {
    let host = ImageHost::serving(Answer::image(snapshot())).await;
    let mut body = sample("printer-notification-about-a-print.json");
    body["img_url"] = json!(host.snapshot_url());

    let store = Arc::new(MemoryStore::new());
    let receipt = ingress(&store, ObicoVisionConfig::default())
        .receive(body_of(&body), Some("application/json".to_owned()))
        .await
        .expect("the notification is accepted");

    let held = stored_event(&store, &receipt);
    assert_eq!(held.kind(), EventKind::ObicoPrinterNotification);
    assert_eq!(
        notification_payload(&held).notification_type,
        ObicoNotificationType::Started
    );
    assert!(held.print_id.is_some(), "the event names no print");
    let record = receipt.image.clone().expect("the snapshot was stored");
    let (stored, bytes) = stored_image(&store, &record).await;
    assert_eq!(stored.event_id, held.id);
    assert_eq!(bytes, snapshot());
}

/// The notification about no print names none, and stores no snapshot.
#[tokio::test]
async fn the_notification_about_no_print_names_none_and_stores_no_snapshot() {
    let body = RawBytes::new(sample_bytes("printer-notification-not-about-a-print.json"));

    let store = Arc::new(MemoryStore::new());
    let receipt = ingress(&store, ObicoVisionConfig::default())
        .receive(body.clone(), Some("application/json".to_owned()))
        .await
        .expect("the notification is accepted");

    let held = stored_event(&store, &receipt);
    assert_eq!(held.kind(), EventKind::ObicoPrinterNotification);
    assert_eq!(
        notification_payload(&held).notification_type,
        ObicoNotificationType::HeaterCooled
    );
    assert_eq!(held.print_id, None, "the event names a print");
    assert_eq!(
        held.raw.as_ref().map(RawBytes::as_slice),
        Some(body.as_slice())
    );
    assert!(receipt.image.is_none(), "a snapshot was stored");
    assert!(store.prints().is_empty(), "a print record was opened");
}

// --- The two timestamps, in each of the three states they arrive in ----------

/// One timestamp field, in one of the three states the producer sends it in.
enum State {
    /// A Unix timestamp number.
    Numeric(f64),
    /// The empty string the producer sends when it has no instant.
    Empty,
    /// The field omitted altogether.
    Missing,
}

/// Post one alert whose named timestamp field is in one of the three states.
async fn post_with(field: &str, state: &State) -> (Arc<MemoryStore>, Receipt) {
    let mut body = failure_alert();
    // The other field is left in whatever state the committed sample carries.
    match state {
        State::Numeric(seconds) => body["print"][field] = json!(seconds),
        State::Empty => body["print"][field] = json!(""),
        State::Missing => {
            body["print"]
                .as_object_mut()
                .expect("the print is an object")
                .remove(field);
        }
    }
    body["img_url"] = json!(unreachable_url().await);
    let store = Arc::new(MemoryStore::new());
    let receipt = ingress(&store, prompt_bounds())
        .receive(body_of(&body), Some("application/json".to_owned()))
        .await
        .expect("the alert is accepted");
    (store, receipt)
}

/// Both fields, in all three states, and none of the six is an epoch date.
#[tokio::test]
async fn both_timestamps_parse_in_each_of_the_three_states() {
    let epoch: Timestamp = "1970-01-01T00:00:00Z".parse().expect("the epoch parses");
    let named: Timestamp = "2026-03-01T12:00:00.5Z"
        .parse()
        .expect("the instant 1772366400.5 names");

    for field in ["started_at", "ended_at"] {
        for (state, expected) in [
            (State::Numeric(1_772_366_400.5), Some(named)),
            (State::Empty, None),
            (State::Missing, None),
        ] {
            let (store, receipt) = post_with(field, &state).await;
            let held = stored_event(&store, &receipt);
            let payload = failure_payload(&held);
            let stored = if field == "started_at" {
                payload.started_at
            } else {
                payload.ended_at
            };
            assert_eq!(stored, expected, "{field} was not stored as it arrived");
            assert_ne!(stored, Some(epoch), "{field} was stored as an epoch date");
        }
    }
}

// --- A body this system cannot read ------------------------------------------

/// Each of the three ways a body cannot be read is recorded, then refused.
#[tokio::test]
async fn a_body_that_cannot_be_read_is_recorded_and_then_refused() {
    let not_well_formed = RawBytes::new(b"{ this is not a body at all".to_vec());

    let mut without_required = failure_alert();
    without_required
        .as_object_mut()
        .expect("the alert is an object")
        .remove("printer")
        .expect("the sample carries a printer");

    let mut wrong_kind = failure_alert();
    wrong_kind["event"]["is_warning"] = json!("yes");

    for (what, body) in [
        ("a body that is not well-formed", not_well_formed),
        (
            "a body omitting a required field",
            body_of(&without_required),
        ),
        (
            "a body whose required field is of the wrong kind",
            body_of(&wrong_kind),
        ),
    ] {
        let store = Arc::new(MemoryStore::new());
        let refusal = ingress(&store, prompt_bounds())
            .receive(body.clone(), Some("application/json".to_owned()))
            .await
            .expect_err(what);

        let IngressError::Refused { refusal, recorded } = refusal else {
            panic!("{what} was not refused as unreadable: {refusal}");
        };
        assert!(
            matches!(refusal, VisionError::Malformed { .. }),
            "{what}: {refusal}"
        );

        let held = store.events();
        assert_eq!(held.len(), 1, "{what} left no single record");
        assert_eq!(held[0].id, recorded.id);
        assert_eq!(held[0].kind(), EventKind::MalformedExternalEvent);
        assert_eq!(held[0].source, EventSource::Obico);
        assert_eq!(held[0].print_id, None);
        assert_eq!(
            held[0].raw.as_ref().map(RawBytes::as_slice),
            Some(body.as_slice()),
            "{what} was not recorded with its own bytes"
        );
    }
}

// --- Correlating an alert to a print -----------------------------------------

/// Post the committed alert against a store, with no reachable snapshot.
async fn post_alert(store: &Arc<MemoryStore>) -> Receipt {
    let mut body = failure_alert();
    body["img_url"] = json!(unreachable_url().await);
    ingress(store, prompt_bounds())
        .receive(body_of(&body), Some("application/json".to_owned()))
        .await
        .expect("the alert is accepted")
}

/// An id an open print already holds continues that print.
#[tokio::test]
async fn an_alert_for_an_open_print_continues_it() {
    let store = Arc::new(MemoryStore::new());
    let open = store
        .open_print(Some(SAMPLE_OBICO_PRINT_ID), Some("benchy.gcode".to_owned()))
        .await
        .expect("a print opens");

    let receipt = post_alert(&store).await;

    assert_eq!(store.prints().len(), 1, "a second print record was opened");
    assert_eq!(stored_event(&store, &receipt).print_id, Some(open.id));
}

/// An id no print record holds opens one carrying it.
#[tokio::test]
async fn an_alert_for_an_unknown_id_opens_a_print_carrying_it() {
    let store = Arc::new(MemoryStore::new());

    let receipt = post_alert(&store).await;

    let prints = store.prints();
    assert_eq!(prints.len(), 1, "no single print record was opened");
    assert_eq!(prints[0].obico_print_id, Some(SAMPLE_OBICO_PRINT_ID));
    assert_eq!(stored_event(&store, &receipt).print_id, Some(prints[0].id));
}

/// An id an ended print holds is correlated to it, and it stays ended.
#[tokio::test]
async fn an_alert_for_an_ended_print_does_not_reopen_it() {
    let store = Arc::new(MemoryStore::new());
    let open = store
        .open_print(Some(SAMPLE_OBICO_PRINT_ID), Some("benchy.gcode".to_owned()))
        .await
        .expect("a print opens");
    let ended_at: Timestamp = "2026-03-01T12:00:00Z".parse().expect("a fixed instant");
    let ended = store
        .end_print(
            open.id,
            PrinterState::Operational,
            ended_at,
            "the print finished".to_owned(),
        )
        .await
        .expect("the print ends");

    let receipt = post_alert(&store).await;

    let carrying: Vec<PrintRecord> = store
        .prints()
        .into_iter()
        .filter(|record| record.obico_print_id == Some(SAMPLE_OBICO_PRINT_ID))
        .collect();
    assert_eq!(carrying.len(), 1, "a replacement print record was opened");
    assert_eq!(carrying[0].id, ended.id);
    assert_eq!(carrying[0].ended_at, Some(ended_at), "the end moved");
    assert_eq!(carrying[0].state, PrinterState::Operational);
    assert_eq!(
        stored_event(&store, &receipt).print_id,
        Some(ended.id),
        "the event was not correlated to the ended print"
    );
}

// --- Fetching the snapshot ---------------------------------------------------

/// Post the committed alert with its snapshot at one URL.
async fn post_alert_for(
    store: &Arc<MemoryStore>,
    snapshot_url: String,
    config: ObicoVisionConfig,
) -> Receipt {
    let mut body = failure_alert();
    body["img_url"] = json!(snapshot_url);
    ingress(store, config)
        .receive(body_of(&body), Some("application/json".to_owned()))
        .await
        .expect("the alert is accepted")
}

/// The snapshot is stored during the handling, under the declared defaults.
///
/// The bounds are [`DEFAULT_FETCH_TIMEOUT`] and [`DEFAULT_MAX_IMAGE_BYTES`]
/// themselves, so a timeout no real response can meet or a maximum no real
/// snapshot fits under fails here while leaving every refusal below passing.
#[tokio::test]
async fn the_snapshot_is_stored_during_the_handling_under_the_defaults() {
    let host = ImageHost::serving(Answer::image(snapshot())).await;
    let store = Arc::new(MemoryStore::new());
    let defaults = ObicoVisionConfig::default();
    assert_eq!(defaults.fetch_timeout, DEFAULT_FETCH_TIMEOUT);
    assert_eq!(defaults.max_image_bytes, DEFAULT_MAX_IMAGE_BYTES);

    let receipt = post_alert_for(&store, host.snapshot_url(), defaults).await;

    // Read the moment the handling returns: nothing below fetches anything.
    let record = receipt
        .image
        .clone()
        .expect("the snapshot was stored during the handling");
    assert!(receipt.image_failure.is_none());
    let (stored, bytes) = stored_image(&store, &record).await;
    assert_eq!(stored.event_id, receipt.event.id);
    assert_eq!(stored.print_id, receipt.print.expect("a print").id);
    assert_eq!(stored.content_type, "image/jpeg");
    assert_eq!(bytes, snapshot());
}

/// Every way the fetch fails leaves the event and the failure written down.
async fn assert_refused(receipt: &Receipt, store: &MemoryStore, expected: &VisionError) {
    assert_eq!(receipt.image_failure.as_ref(), Some(expected));
    assert!(receipt.image.is_none(), "a snapshot was stored anyway");

    let held = stored_event(store, receipt);
    assert_eq!(held.kind(), EventKind::ObicoFailureAlert);
    assert_eq!(held.image, None, "the event names an image");

    let failure = recorded_failure(store, receipt).await;
    assert_eq!(failure.source, EventSource::System);
    assert_eq!(failure.print_id, held.print_id);
    assert!(
        failure_detail(&failure).contains(&expected.to_string()),
        "the recorded failure does not say why: {}",
        failure_detail(&failure)
    );
}

/// A host that answers too late times out, and the event survives it.
#[tokio::test]
async fn a_snapshot_served_too_late_times_out() {
    let host = ImageHost::serving(Answer {
        delay: Duration::from_secs(30),
        ..Answer::image(snapshot())
    })
    .await;
    let store = Arc::new(MemoryStore::new());
    let config = ObicoVisionConfig {
        fetch_timeout: Duration::from_millis(250),
        ..ObicoVisionConfig::default()
    };

    let receipt = post_alert_for(&store, host.snapshot_url(), config).await;

    assert_refused(&receipt, &store, &VisionError::TimedOut).await;
}

/// A snapshot over the maximum is refused, whether or not it declares itself.
#[tokio::test]
async fn a_snapshot_over_the_maximum_is_refused() {
    for declare_length in [true, false] {
        let host = ImageHost::serving(Answer {
            declare_length,
            ..Answer::image(snapshot())
        })
        .await;
        let store = Arc::new(MemoryStore::new());
        let config = ObicoVisionConfig {
            max_image_bytes: 100,
            ..prompt_bounds()
        };

        let receipt = post_alert_for(&store, host.snapshot_url(), config).await;

        assert_refused(&receipt, &store, &VisionError::TooLarge { limit: 100 }).await;
    }
}

/// A content type that is not an image is refused, naming what arrived.
#[tokio::test]
async fn a_content_type_that_is_not_an_image_is_refused() {
    for (declared, named) in [
        (
            Some("text/plain; charset=utf-8".to_owned()),
            "text/plain; charset=utf-8",
        ),
        (None, ""),
    ] {
        let host = ImageHost::serving(Answer {
            content_type: declared,
            ..Answer::image(snapshot())
        })
        .await;
        let store = Arc::new(MemoryStore::new());

        let receipt = post_alert_for(&store, host.snapshot_url(), prompt_bounds()).await;

        assert_refused(
            &receipt,
            &store,
            &VisionError::UnacceptableContentType {
                content_type: named.to_owned(),
            },
        )
        .await;
    }
}

/// A host nothing is listening on is unreachable, and the event survives it.
///
/// This is the one failure no served response can produce, so it is the one an
/// implementation that handled the three refusals above can still lose.
#[tokio::test]
async fn a_host_nothing_answers_on_is_unreachable() {
    let store = Arc::new(MemoryStore::new());

    let receipt = post_alert_for(&store, unreachable_url().await, prompt_bounds()).await;

    let failure = receipt
        .image_failure
        .clone()
        .expect("the fetch failed and said so");
    assert!(
        matches!(failure, VisionError::Unreachable { .. }),
        "expected an unreachable host, found {failure}"
    );
    assert_refused(&receipt, &store, &failure).await;
}

/// A host that answers with a status other than success is unreachable.
#[tokio::test]
async fn a_host_that_refuses_the_request_is_unreachable() {
    let host = ImageHost::serving(Answer {
        status: "404 Not Found",
        content_type: None,
        body: Vec::new(),
        delay: Duration::ZERO,
        declare_length: true,
    })
    .await;
    let store = Arc::new(MemoryStore::new());

    let receipt = post_alert_for(&store, host.snapshot_url(), prompt_bounds()).await;

    let failure = receipt
        .image_failure
        .clone()
        .expect("the fetch failed and said so");
    assert!(
        matches!(failure, VisionError::Unreachable { .. }),
        "expected an unreachable host, found {failure}"
    );
    assert_refused(&receipt, &store, &failure).await;
}
