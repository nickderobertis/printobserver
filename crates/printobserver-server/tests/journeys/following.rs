//! The event log, followed through the real `events` route.
//!
//! Every event here is stored through the server's own store handle, under
//! kinds no crate declares — the log is open, and a kind nothing else writes is
//! one no background part of the server can store beside a journey's own. What
//! is asserted is what the route answers.

use core::time::Duration;
use std::time::Instant;

use printobserver_core::store::EventDraft;
use printobserver_core::{PrintEndedPayload, PrintOutcome, system_source};
use printobserver_printer_api::PrinterState;
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{EventBody, EventKind, EventPayload, EventRecord, PrintId, Timestamp};

use crate::world::World;

/// A kind no crate of this workspace declares, which only these journeys store.
const MARKER: &str = "following_marker";

/// A second such kind, for what a filter leaves out.
const OTHER: &str = "following_other";

/// Store one event of a kind, on a print or on none.
async fn store(world: &World, print_id: Option<PrintId>, kind: &str) -> EventRecord {
    world
        .stores
        .events
        .append_event(EventDraft {
            print_id,
            source: system_source(),
            received_at: Timestamp::now(),
            body: EventBody {
                kind: EventKind::new(kind).expect("a kind name"),
                payload: json!({ "said": kind }),
            },
            raw: None,
        })
        .await
        .expect("the event is stored")
}

/// Store one print's end.
async fn end(world: &World, print_id: PrintId) -> EventRecord {
    let payload = PrintEndedPayload {
        outcome: PrintOutcome::Completed,
        state: PrinterState::Operational,
        reason: "it ran to its end".to_owned(),
        last_position: None,
    };
    world
        .stores
        .events
        .append_event(EventDraft {
            print_id: Some(print_id),
            source: system_source(),
            received_at: Timestamp::now(),
            body: EventBody {
                kind: EventKind::new(PrintEndedPayload::KIND).expect("a kind name"),
                payload: printobserver_types::serde_json::to_value(payload)
                    .expect("the payload renders"),
            },
            raw: None,
        })
        .await
        .expect("the end is stored")
}

/// The events route, asked these values.
fn events_url(world: &World, values: &[(&str, String)]) -> String {
    let asked: Vec<String> = values
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    format!("{}?{}", world.url("/events"), asked.join("&"))
}

/// The identifiers an answer carries, oldest first.
fn ids(answer: &Value) -> Vec<String> {
    answer["events"]
        .as_array()
        .expect("an events answer carries its events")
        .iter()
        .map(|event| event["id"].as_str().expect("an event has an id").to_owned())
        .collect()
}

/// Ask the route on a task of its own, so a journey can store while it waits.
fn asking(world: &World, url: String) -> tokio::task::JoinHandle<(reqwest::StatusCode, Value)> {
    let client = world.client.clone();
    tokio::spawn(async move {
        let response = client
            .get(url)
            .header(reqwest::header::ACCEPT, printobserver_server::MEDIA_TYPE)
            .send()
            .await
            .expect("the server answers");
        let status = response.status();
        let body = response.json::<Value>().await.expect("the answer is JSON");
        (status, body)
    })
}

/// A long time to wait, which no journey here waits out.
const LONG_WAIT_S: u32 = 30;

/// How long a journey lets a request reach the server and start waiting.
const SETTLE: Duration = Duration::from_millis(400);

/// A read that is waiting is answered by the event stored while it waits: the
/// answer carries that event, it was not answered before the event was stored,
/// and it was answered long before its own deadline.
#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_read_is_answered_by_the_event_stored_while_it_waits() {
    let world = World::open().await;
    let started = Instant::now();
    let waiting = asking(
        &world,
        events_url(
            &world,
            &[
                ("kinds", MARKER.to_owned()),
                ("wait_s", LONG_WAIT_S.to_string()),
            ],
        ),
    );
    tokio::time::sleep(SETTLE).await;
    assert!(
        !waiting.is_finished(),
        "the read answered before anything it asks for was stored"
    );

    let stored = store(&world, None, MARKER).await;
    let (status, answer) = waiting.await.expect("the read finishes");

    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(ids(&answer), vec![stored.id.to_string()], "{answer}");
    assert_eq!(answer["events"][0]["kind"], json!(MARKER), "{answer}");
    assert_eq!(answer["cursor"], json!(stored.id.to_string()), "{answer}");
    assert!(
        answer.get("print_ended").is_none(),
        "a read naming no print carries a print's end: {answer}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(u64::from(LONG_WAIT_S) / 2),
        "the read was answered at its deadline rather than by the event: {:?}",
        started.elapsed()
    );
    world.server.stop().await;
}

/// A read nothing matches is answered empty once its wait is over.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_nothing_matches_is_answered_empty_after_its_wait() {
    let world = World::open().await;
    let started = Instant::now();

    let (status, answer) = world
        .get(&events_url(
            &world,
            &[("kinds", MARKER.to_owned()), ("wait_s", "1".to_owned())],
        ))
        .await;

    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(answer["events"], json!([]), "{answer}");
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "the read did not wait: {:?}",
        started.elapsed()
    );
    world.server.stop().await;
}

/// The cursor is the last event examined, so a row the filters dropped is not
/// examined again — and neither is a row already answered.
#[tokio::test(flavor = "multi_thread")]
async fn the_cursor_skips_the_rows_its_filters_dropped() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let kept = store(&world, Some(print_id), MARKER).await;
    let dropped = store(&world, Some(print_id), OTHER).await;

    let (status, answer) = world
        .get(&events_url(
            &world,
            &[
                ("print_id", print_id.to_string()),
                ("kinds", MARKER.to_owned()),
            ],
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(ids(&answer), vec![kept.id.to_string()], "{answer}");
    let cursor = answer["cursor"]
        .as_str()
        .expect("the read examined something")
        .to_owned();
    assert_ne!(
        cursor,
        kept.id.to_string(),
        "the cursor stopped at the last event answered"
    );

    // Read on from the cursor with no filter at all: neither row comes back.
    let (status, after) = world
        .get(&events_url(
            &world,
            &[
                ("print_id", print_id.to_string()),
                ("after", cursor.clone()),
            ],
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{after}");
    let again = ids(&after);
    assert!(
        !again.contains(&kept.id.to_string()) && !again.contains(&dropped.id.to_string()),
        "reading on from {cursor} examined a row again: {after}"
    );
    world.server.stop().await;
}

/// With no cursor and no print, a read starts after the newest event stored at
/// the request: an earlier event of the kind it asks for is not answered, and
/// the one stored while it waits is.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_with_no_cursor_answers_only_what_is_stored_after_it() {
    let world = World::open().await;
    let earlier = store(&world, None, MARKER).await;
    let waiting = asking(
        &world,
        events_url(
            &world,
            &[
                ("kinds", MARKER.to_owned()),
                ("wait_s", LONG_WAIT_S.to_string()),
            ],
        ),
    );
    tokio::time::sleep(SETTLE).await;
    let later = store(&world, None, MARKER).await;

    let (status, answer) = waiting.await.expect("the read finishes");

    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(ids(&answer), vec![later.id.to_string()], "{answer}");
    assert!(!ids(&answer).contains(&earlier.id.to_string()), "{answer}");
    world.server.stop().await;
}

/// Scoped to a print with no cursor, a read replays that print from its first
/// event: its own events and the printer's from there, and neither another
/// print's nor a printer event stored before the print began.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_scoped_to_a_print_replays_it_from_its_first_event() {
    let world = World::open().await;
    let before = store(&world, None, MARKER).await;
    let print_id = world.open_print().await;
    let other = world.open_print().await;
    let first = store(&world, Some(print_id), MARKER).await;
    let elsewhere = store(&world, Some(other), MARKER).await;
    let printer = store(&world, None, MARKER).await;
    let second = store(&world, Some(print_id), MARKER).await;

    let (status, answer) = world
        .get(&events_url(
            &world,
            &[
                ("print_id", print_id.to_string()),
                ("kinds", MARKER.to_owned()),
            ],
        ))
        .await;

    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(
        ids(&answer),
        vec![
            first.id.to_string(),
            printer.id.to_string(),
            second.id.to_string()
        ],
        "before {} and elsewhere {}: {answer}",
        before.id,
        elsewhere.id
    );
    assert_eq!(answer["print_ended"], Value::Null, "{answer}");
    world.server.stop().await;
}

/// A limit is honoured, and the cursor it leaves reads on to the rest.
#[tokio::test(flavor = "multi_thread")]
async fn a_limit_is_honoured_and_its_cursor_reads_on() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let mut stored = Vec::new();
    for _ in 0..5 {
        stored.push(store(&world, Some(print_id), MARKER).await.id.to_string());
    }
    let asked = |after: Option<String>| {
        let mut values = vec![
            ("print_id", print_id.to_string()),
            ("kinds", MARKER.to_owned()),
            ("limit", "2".to_owned()),
        ];
        if let Some(after) = after {
            values.push(("after", after));
        }
        events_url(&world, &values)
    };

    let (_, page) = world.get(&asked(None)).await;
    assert_eq!(ids(&page), stored[..2].to_vec(), "{page}");
    assert_eq!(page["cursor"], json!(stored[1]), "{page}");
    let (_, next) = world
        .get(&asked(page["cursor"].as_str().map(str::to_owned)))
        .await;
    assert_eq!(ids(&next), stored[2..4].to_vec(), "{next}");
    world.server.stop().await;
}

/// A kind no crate declares is taken, and every value out of its range or
/// shape is refused naming it.
#[tokio::test(flavor = "multi_thread")]
async fn a_well_formed_kind_is_taken_and_every_bad_value_is_refused_naming_it() {
    let world = World::open().await;

    let (status, answer) = world
        .get(&events_url(
            &world,
            &[("kinds", "no_crate_declares_this".to_owned())],
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");

    for (name, value) in [
        ("kinds", "Not-A-Kind"),
        ("exclude_kinds", "9lives"),
        ("wait_s", "61"),
        ("wait_s", "-1"),
        ("limit", "0"),
        ("limit", "501"),
        ("after", "not-an-event"),
        ("print_ended", "1"),
    ] {
        let (status, refused) = world
            .get(&events_url(&world, &[(name, value.to_owned())]))
            .await;
        assert_eq!(
            status,
            reqwest::StatusCode::BAD_REQUEST,
            "{name}={value}: {refused}"
        );
        let said = refused["error"].as_str().unwrap_or_default();
        assert!(
            said.contains(&format!("`{name}`")),
            "{name}={value}: {said}"
        );
    }
    world.server.stop().await;
}

/// Twenty reads waiting at once are all answered by one stored event, each
/// with that event — and a status read made while they wait is answered while
/// every one of them is still waiting.
#[tokio::test(flavor = "multi_thread")]
async fn twenty_waiting_reads_are_all_answered_by_one_event_and_hold_no_worker() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let url = events_url(
        &world,
        &[
            ("kinds", MARKER.to_owned()),
            ("wait_s", LONG_WAIT_S.to_string()),
        ],
    );
    let waiting: Vec<_> = (0..20).map(|_| asking(&world, url.clone())).collect();
    tokio::time::sleep(SETTLE).await;

    let (status, read) = world
        .get(&world.operation_url("/v1/prints/{print_id}/status", print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{read}");
    assert!(
        waiting.iter().all(|one| !one.is_finished()),
        "a read finished before anything it asks for was stored"
    );

    let stored = store(&world, None, MARKER).await;
    for one in waiting {
        let (status, answer) = one.await.expect("the read finishes");
        assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
        assert_eq!(ids(&answer), vec![stored.id.to_string()], "{answer}");
    }
    world.server.stop().await;
}

/// A read scoped to a print tells that print's end whatever its filters say:
/// null while it is open, and its `print_ended` event's identifier, instant
/// and outcome once it has ended, with that event filtered out.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_end_is_told_whatever_the_filters_say() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    store(&world, Some(print_id), MARKER).await;
    let asked = events_url(
        &world,
        &[
            ("print_id", print_id.to_string()),
            ("exclude_kinds", PrintEndedPayload::KIND.to_owned()),
        ],
    );

    let (_, open) = world.get(&asked).await;
    assert_eq!(open["print_ended"], Value::Null, "{open}");

    let ended = end(&world, print_id).await;
    let (status, answer) = world.get(&asked).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(
        answer["print_ended"],
        json!({
            "event_id": ended.id.to_string(),
            "received_at": ended.received_at,
            "outcome": "completed",
        }),
        "{answer}"
    );
    assert!(
        !ids(&answer).contains(&ended.id.to_string()),
        "the filtered-out end was answered as an event: {answer}"
    );

    let (_, unscoped) = world
        .get(&events_url(&world, &[("kinds", MARKER.to_owned())]))
        .await;
    assert!(
        unscoped.get("print_ended").is_none(),
        "a read naming no print carries a print's end: {unscoped}"
    );
    world.server.stop().await;
}

/// A value given twice that is read once, a print nothing holds and an event
/// nothing stored are each refused naming what will not do.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_single_value_a_missing_print_and_a_missing_cursor_are_refused() {
    let world = World::open().await;

    let (status, refused) = world
        .get(&events_url(
            &world,
            &[("limit", "1".to_owned()), ("limit", "2".to_owned())],
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|said| said.contains("`limit`")),
        "{refused}"
    );

    let nobody = PrintId::new();
    let (status, refused) = world
        .get(&events_url(&world, &[("print_id", nobody.to_string())]))
        .await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|said| said.contains(&nobody.to_string())),
        "{refused}"
    );

    let (status, refused) = world
        .get(&events_url(
            &world,
            &[("after", printobserver_types::EventId::new().to_string())],
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND, "{refused}");
    world.server.stop().await;
}

/// A read whose filters reject more stored events than it examines at once
/// reads on past them to the one that passes, and its cursor is the last event
/// it examined.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_examines_past_every_event_its_filters_reject_to_the_one_that_passes() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let first = store(&world, Some(print_id), OTHER).await;
    for _ in 0..1_200 {
        store(&world, Some(print_id), OTHER).await;
    }
    let passing = store(&world, Some(print_id), MARKER).await;

    let (status, answer) = world
        .get(&events_url(
            &world,
            &[
                ("print_id", print_id.to_string()),
                ("after", first.id.to_string()),
                ("kinds", MARKER.to_owned()),
            ],
        ))
        .await;

    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(ids(&answer), vec![passing.id.to_string()], "{answer}");
    assert_eq!(answer["cursor"], json!(passing.id.to_string()), "{answer}");
    world.server.stop().await;
}
