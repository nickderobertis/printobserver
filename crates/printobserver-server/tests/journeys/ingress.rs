//! The endpoint `Obico` posts to: what it refuses, what it takes, and when.
//!
//! Every journey here drives the real endpoint over HTTP with the committed
//! `Obico` sample — the contracts' own file, not a body written here — so what
//! is proven is that this server takes what that producer sends.
//!
//! The two properties that are not about a status code are the point of the
//! file. An unauthenticated post is **recorded** as well as refused, because
//! anything that can post here can pause a printer and the one class of post an
//! operator most needs to see must not be dropped. And an authenticated post is
//! **answered before its handling completes** and inside the bound this
//! repository declares, because `Obico` posts best-effort with a short timeout
//! and does not retry.

use core::time::Duration;
use std::time::Instant;

use printobserver_store_api::HistoryQuery;
use printobserver_types::EventKind;

use crate::agent::StandInAgent;
use crate::http_host::image_host;
use crate::printer::RecordingPrinter;
use crate::world::{SECRET, World, committed_sample, failure_alert};

/// How long a journey lets the handling of one alert run for, when it is
/// watching the answer precede it.
const DWELL: Duration = Duration::from_secs(2);

/// The bytes a journey's own image host serves.
pub fn snapshot_bytes() -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8, 0xff];
    bytes.extend(b"printobserver-journey-snapshot".repeat(8));
    bytes
}

/// Post one body to the real ingress, with the token given.
async fn post(world: &World, body: &str, token: Option<&str>) -> reqwest::StatusCode {
    let url = match token {
        Some(token) => format!("{}?token={token}", world.server.ingress_url()),
        None => world.server.ingress_url(),
    };
    world
        .client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_owned())
        .send()
        .await
        .expect("the ingress answers")
        .status()
}

/// Every event recorded against a print this server still holds open.
async fn recorded_against_open_prints(world: &World) -> Vec<printobserver_types::EventRecord> {
    let mut found = Vec::new();
    for print in world
        .store
        .open_prints()
        .await
        .expect("the open prints read")
    {
        found.extend(
            world
                .store
                .history(HistoryQuery {
                    print_id: print.id,
                    kinds: Vec::new(),
                    since: None,
                    until: None,
                    limit: None,
                })
                .await
                .expect("a history reads"),
        );
    }
    found
}

/// The secret is taken from the header as well as from the query.
///
/// `Obico`'s own plugin is configured with a URL and nothing else, so the query
/// form is the one a real producer carries — and a caller that can set headers
/// should not have to put a secret in a URL that ends up in an access log.
#[tokio::test(flavor = "multi_thread")]
async fn the_secret_is_taken_from_the_header_as_well_as_from_the_query() {
    let host = image_host(snapshot_bytes()).await;
    let world = World::open().await;
    let mut completions = world.server.completions();

    let accepted = world
        .client
        .post(world.server.ingress_url())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(printobserver_server::TOKEN_HEADER, SECRET)
        .body(failure_alert(4211, &host.url()).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(accepted, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");
    assert_eq!(
        world.agent.turns().len(),
        1,
        "a post authenticated by the header did not reach the loop"
    );

    let refused = world
        .client
        .post(world.server.ingress_url())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(printobserver_server::TOKEN_HEADER, "not-the-secret")
        .body(failure_alert(4211, &host.url()).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(
        refused,
        reqwest::StatusCode::UNAUTHORIZED,
        "a header carrying the wrong secret was taken"
    );
    world.server.stop().await;
}

/// A post carrying no valid secret is refused, and written down.
#[tokio::test(flavor = "multi_thread")]
async fn a_post_with_no_valid_secret_is_refused_and_recorded() {
    let world = World::open().await;

    for offered in [None, Some("not-the-secret")] {
        let status = post(&world, committed_sample(), offered).await;
        assert_eq!(
            status,
            reqwest::StatusCode::UNAUTHORIZED,
            "a post carrying {offered:?} was taken"
        );
    }

    assert!(
        world
            .store
            .open_prints()
            .await
            .expect("the open prints read")
            .is_empty(),
        "a refused post opened a print"
    );

    // What was written is the malformed-external-event kind carrying the bytes
    // that arrived, which is the one kind that carries bytes with no print.
    let written = unauthenticated_records(&world.state_dir());
    assert_eq!(
        written.len(),
        2,
        "two posts were refused and {} were written down",
        written.len()
    );
    for (kind, raw) in written {
        assert_eq!(kind, "malformed_external_event");
        assert_eq!(
            raw,
            committed_sample().as_bytes().to_vec(),
            "the record of a refused post does not carry the bytes that arrived"
        );
    }
    world.server.stop().await;
}

/// Every event this server recorded that belongs to no print.
///
/// The store port answers no read for one, because every read it declares is
/// about a print and this record is about none — which is exactly what makes it
/// worth asserting. So it is read out of the store's own database, opened
/// through `printobserver-store-sqlite`'s own `connect`, rather than through a
/// second belief about where the record went.
fn unauthenticated_records(state_dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let path = state_dir.join(printobserver_store_sqlite::DATABASE_FILE_NAME);
    assert!(
        path.exists(),
        "the store is not where the state directory says"
    );
    let connection = printobserver_store_sqlite::connect(&path).expect("the store opens");
    let mut statement = connection
        .prepare("SELECT kind, raw FROM events WHERE print_id IS NULL ORDER BY received_at, id")
        .expect("the events table is where the schema says");
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<Vec<u8>>>(1)?.unwrap_or_default(),
            ))
        })
        .expect("the events read");
    rows.collect::<Result<Vec<_>, _>>()
        .expect("the events read")
}

/// The committed sample is taken, handled, and leaves the record it should.
#[tokio::test(flavor = "multi_thread")]
async fn the_committed_sample_is_taken_and_handled() {
    let host = image_host(snapshot_bytes()).await;
    let world = World::open().await;
    let mut completions = world.server.completions();

    let body = failure_alert(4211, &host.url()).to_string();
    let status = post(&world, &body, Some(SECRET)).await;
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);

    completions.changed().await.expect("the handling finishes");

    let events = recorded_against_open_prints(&world).await;
    let alert = events
        .iter()
        .find(|event| event.kind() == EventKind::ObicoFailureAlert)
        .expect("the alert was recorded");
    assert!(
        alert.image.is_some(),
        "the alert was recorded with no image beside it"
    );
    assert!(
        events
            .iter()
            .any(|event| event.kind() == EventKind::SupervisionSessionOpened),
        "no session opened for the alert: {:?}",
        events
            .iter()
            .map(printobserver_types::EventRecord::kind)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        world.agent.turns().len(),
        1,
        "the alert did not reach one supervision turn"
    );
    world.server.stop().await;
}

/// The ingress answers before its handling completes, and inside its bound.
#[tokio::test(flavor = "multi_thread")]
async fn the_answer_precedes_the_handling_and_arrives_inside_the_bound() {
    let host = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    agent.taking(DWELL);
    let world = World::open_with(RecordingPrinter::printing(), agent).await;
    let bound = world.server.config().ingress_answer_bound;
    let mut completions = world.server.completions();

    let started = Instant::now();
    let status = post(
        &world,
        &failure_alert(4211, &host.url()).to_string(),
        Some(SECRET),
    )
    .await;
    let answered = started.elapsed();

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert!(
        answered < bound,
        "the ingress answered in {answered:?}, outside its own {bound:?} bound"
    );
    assert_eq!(
        *completions.borrow(),
        0,
        "the handling had already finished when the answer arrived, so nothing here \
         says the answer preceded it"
    );

    completions.changed().await.expect("the handling finishes");
    assert!(
        started.elapsed() >= DWELL,
        "the handling finished in {:?}, which is less than it was made to take, so \
         the answer did not precede anything",
        started.elapsed()
    );
    world.server.stop().await;
}

/// A body this system cannot read is taken, recorded, and costs nothing else.
#[tokio::test(flavor = "multi_thread")]
async fn a_body_this_system_cannot_read_is_written_down() {
    let world = World::open().await;
    let mut completions = world.server.completions();

    let status = post(&world, "{\"event\": {}}", Some(SECRET)).await;
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);

    completions.changed().await.expect("the handling finishes");

    let written = unauthenticated_records(&world.state_dir());
    assert_eq!(
        written.len(),
        1,
        "a body this system could not read was not written down"
    );
    assert_eq!(written[0].0, "malformed_external_event");
    world.server.stop().await;
}

/// The ingress refuses, inside its bound, what it cannot take.
///
/// The queue between the answer and the handling is what lets the answer
/// precede it, and it is bounded on purpose: a body the worker is too far
/// behind to take is refused inside the bound rather than held past it, because
/// past the bound is an alert Obico has already abandoned.
#[tokio::test(flavor = "multi_thread")]
async fn what_the_ingress_cannot_take_is_refused_inside_its_bound() {
    let host = image_host(snapshot_bytes()).await;
    let agent = StandInAgent::new();
    agent.taking(DWELL);
    let world = World::open_with(RecordingPrinter::printing(), agent).await;
    let bound = world.server.config().ingress_answer_bound;
    let body = failure_alert(4211, &host.url()).to_string();

    // One more than the worker can be holding and the queue can be carrying, so
    // the last of them meets a queue with nowhere to put it.
    let mut refused = 0;
    let started = std::time::Instant::now();
    for _ in 0..=(printobserver_server::QUEUE_DEPTH + 1) {
        if post(&world, &body, Some(SECRET)).await == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            refused += 1;
        }
    }
    let taken = started.elapsed();

    assert!(
        refused > 0,
        "the queue took more than it can hold, so nothing here is bounded"
    );
    assert!(
        taken < bound * u32::try_from(printobserver_server::QUEUE_DEPTH + 2).expect("a count"),
        "the ingress held a body it could not take past its own {bound:?} bound: {taken:?}"
    );
    world.server.stop().await;
}

/// What the ingress says about itself carries no secret.
#[tokio::test(flavor = "multi_thread")]
async fn what_the_ingress_says_about_itself_carries_no_secret() {
    let world = World::open().await;
    let ingress =
        printobserver_server::IngressState::start(
            std::sync::Arc::clone(world.server.supervisor()),
            std::sync::Arc::clone(world.server.store()),
            std::sync::Arc::new(
                printobserver_obico::ObicoVision::new(
                    printobserver_obico::ObicoVisionConfig::default(),
                )
                .expect("the adapter is built"),
            ),
            printobserver_server::SharedSecret::new(SECRET).expect("a secret"),
            world.server.config().ingress_answer_bound,
        );

    let rendered = format!("{ingress:?}");

    assert!(
        !rendered.contains(SECRET),
        "a rendering of the ingress carries the secret it requires: {rendered}"
    );
    assert!(
        rendered.contains("IngressState"),
        "a rendering of the ingress says nothing about what it is: {rendered}"
    );
    assert_eq!(*ingress.completions().borrow(), 0);
    world.server.stop().await;
}
