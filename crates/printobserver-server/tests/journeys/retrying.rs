//! A camera that answers a scheduled check with a server error, through the
//! real server.
//!
//! go2rtc answers a frame it could not take with `500` and `exit status 69`,
//! and the next request often succeeds. So a fetch answered `500` to `599` is
//! asked again, three more times half a second apart under the defaults this
//! server ships, each answer written down as a `camera_retry` as it happens;
//! only a fetch whose every attempt was refused is the port failure it was
//! before. Any other refusal — a `404`, a connection refused, a request that
//! times out — fails the fetch at once. What each journey reads is the print's
//! history and the camera's own record of the requests it was sent and when.

use core::time::Duration;

use printobserver_core::{DEFAULT_CAMERA_RETRIES, DEFAULT_CAMERA_RETRY_BACKOFF};
use printobserver_obico::DEFAULT_FETCH_TIMEOUT;
use printobserver_types::EventPayload as _;

use crate::camera_host::{Exchange, GO2RTC_FAILURE, Scripted, ScriptedHost, refusing_url};
use crate::checks::{FirstCheck, checking, checking_over, stored};
use crate::held_events::HeldEvents;
use crate::ingress::snapshot_bytes;

/// How far a moment the camera's host stamps may stand from the moment the
/// client acted, on a shared host: the round trip of a request on loopback and a
/// waiting thread waking. So a retry may be made this much later than the backoff
/// and still be "near" it, and a request stamped this much after the client's
/// timer started is still one abandoned at the timeout.
const SLACK: Duration = Duration::from_millis(400);

/// The exchanges one check made, given the moment it was seen stored: every
/// request the camera received before then. The next check is scheduled an
/// interval after this one finished, so none of its requests is among them.
fn exchanges_before(camera: &ScriptedHost, check: &FirstCheck) -> Vec<Exchange> {
    camera
        .exchanges()
        .into_iter()
        .filter(|exchange| exchange.received < check.seen)
        .collect()
}

/// Every retry waited at least the backoff after the answer before it, and,
/// when `near`, not much longer.
fn assert_spaced(exchanges: &[Exchange], near: bool) {
    for pair in exchanges.windows(2) {
        let answered = pair[0].answered.expect("a refusal was answered");
        let waited = pair[1].received.duration_since(answered);
        assert!(
            waited >= DEFAULT_CAMERA_RETRY_BACKOFF,
            "a retry was made {waited:?} after the answer before it, inside the \
             {DEFAULT_CAMERA_RETRY_BACKOFF:?} backoff"
        );
        if near {
            assert!(
                waited <= DEFAULT_CAMERA_RETRY_BACKOFF + SLACK,
                "a retry was made {waited:?} after the answer before it, far from the \
                 {DEFAULT_CAMERA_RETRY_BACKOFF:?} backoff"
            );
        }
    }
}

/// Two `500`s saying `exit status 69` and then a frame: the check carries the
/// frame, no port failure is recorded, and each `500` is one retry naming its
/// attempt, its status and what the camera said, each made at least the
/// backoff after the answer before it.
#[tokio::test(flavor = "multi_thread")]
async fn two_server_errors_then_a_frame_are_retried_and_the_check_carries_the_frame() {
    let camera = ScriptedHost::answering(vec![
        Scripted::exit_status_69(),
        Scripted::exit_status_69(),
        Scripted::frame(snapshot_bytes()),
    ])
    .await;
    let world = checking(camera.url()).await;
    let print_id = world.open_print().await;

    let check = FirstCheck::of(&world, print_id).await;

    assert!(
        check.observation.image.is_some(),
        "the check does not carry the frame the third attempt gave"
    );
    let retries: Vec<(u32, u32, u16, &str)> = check
        .retries
        .iter()
        .map(|retry| {
            (
                retry.attempt,
                retry.attempts,
                retry.status,
                retry.excerpt.as_str(),
            )
        })
        .collect();
    let attempts = DEFAULT_CAMERA_RETRIES + 1;
    assert_eq!(
        retries,
        vec![
            (1, attempts, 500, GO2RTC_FAILURE),
            (2, attempts, 500, GO2RTC_FAILURE)
        ]
    );
    assert!(
        check.retries.iter().all(|retry| retry.backoff_ms == 500),
        "{:?}",
        check.retries
    );
    let exchanges = exchanges_before(&camera, &check);
    assert_eq!(exchanges.len(), 3, "{exchanges:?}");
    assert_spaced(&exchanges, false);
    assert_eq!(check.failures(&world).await, Vec::new());
    world.server.stop().await;
}

/// A `502` and then a `503` before a frame are retried as a `500` is.
#[tokio::test(flavor = "multi_thread")]
async fn a_bad_gateway_then_an_unavailable_before_a_frame_are_retried_the_same_way() {
    let camera = ScriptedHost::answering(vec![
        Scripted::refusing("502 Bad Gateway", "upstream camera went away"),
        Scripted::refusing("503 Service Unavailable", "the stream is starting"),
        Scripted::frame(snapshot_bytes()),
    ])
    .await;
    let world = checking(camera.url()).await;
    let print_id = world.open_print().await;

    let check = FirstCheck::of(&world, print_id).await;

    assert!(
        check.observation.image.is_some(),
        "the check carries no frame"
    );
    let retries: Vec<(u32, u16, &str)> = check
        .retries
        .iter()
        .map(|retry| (retry.attempt, retry.status, retry.excerpt.as_str()))
        .collect();
    assert_eq!(
        retries,
        vec![
            (1, 502, "upstream camera went away"),
            (2, 503, "the stream is starting")
        ]
    );
    let exchanges = exchanges_before(&camera, &check);
    assert_eq!(exchanges.len(), 3, "{exchanges:?}");
    assert_spaced(&exchanges, false);
    assert_eq!(check.failures(&world).await, Vec::new());
    world.server.stop().await;
}

/// Under the defaults, a camera that answers `500` every time is asked
/// exactly four times, the retries spaced near the half-second backoff, and
/// exactly one port failure is recorded after them, against the check — which
/// is still stored, with no frame.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_refusing_every_attempt_is_asked_four_times_and_failed_once() {
    let camera = ScriptedHost::answering(vec![Scripted::exit_status_69()]).await;
    let world = checking(camera.url()).await;
    let print_id = world.open_print().await;

    let check = FirstCheck::of(&world, print_id).await;

    assert!(check.observation.image.is_none(), "{:?}", check.observation);
    let exchanges = exchanges_before(&camera, &check);
    assert_eq!(
        exchanges.len(),
        usize::try_from(DEFAULT_CAMERA_RETRIES + 1).expect("a count"),
        "{exchanges:?}"
    );
    assert_spaced(&exchanges, true);
    let attempts: Vec<u32> = check.retries.iter().map(|retry| retry.attempt).collect();
    assert_eq!(attempts, vec![1, 2, 3]);
    let failure = check.camera_failure(&world).await;
    assert!(failure.detail.contains("500"), "{failure:?}");
    assert!(failure.detail.contains(GO2RTC_FAILURE), "{failure:?}");
    world.server.stop().await;
}

/// A `404`, a connection refused and a request that times out are each
/// failed as before: the camera is asked at most once, no retry is written
/// down, and the check is stored with its port failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_no_retry_could_change_is_failed_at_once() {
    let missing =
        ScriptedHost::answering(vec![Scripted::refusing("404 Not Found", "no such stream")]).await;
    let world = checking(missing.url()).await;
    let print_id = world.open_print().await;
    let check = FirstCheck::of(&world, print_id).await;
    assert_eq!(exchanges_before(&missing, &check).len(), 1);
    assert!(check.retries.is_empty(), "{:?}", check.retries);
    assert!(check.camera_failure(&world).await.detail.contains("404"));
    world.server.stop().await;

    let world = checking(refusing_url().await).await;
    let print_id = world.open_print().await;
    let check = FirstCheck::of(&world, print_id).await;
    assert!(check.retries.is_empty(), "{:?}", check.retries);
    assert!(
        check
            .camera_failure(&world)
            .await
            .detail
            .contains("unreachable"),
        "a refused connection is not failed as unreachable"
    );
    world.server.stop().await;
}

/// A camera that takes the request and never answers is abandoned near the
/// default five-second per-fetch timeout after the request, asked once, and no
/// retry is written down.
///
/// The client's timer starts before it connects and the host's stamp comes after
/// it has read the request, so the time between that stamp and the check being
/// seen can fall short of the timeout by the lag between them. That the adapter
/// never gives up early is held on the client's own clock, by
/// `a_snapshot_served_too_late_times_out` in the Obico crate's ingress tests.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_that_never_answers_is_abandoned_near_the_timeout_and_not_retried() {
    assert_eq!(DEFAULT_FETCH_TIMEOUT, Duration::from_secs(5));
    let silent = ScriptedHost::answering(vec![Scripted::silent()]).await;
    let world = checking(silent.url()).await;
    let print_id = world.open_print().await;

    let check = FirstCheck::of(&world, print_id).await;

    let exchanges = exchanges_before(&silent, &check);
    assert_eq!(exchanges.len(), 1, "{exchanges:?}");
    let abandoned = check.seen.duration_since(exchanges[0].received);
    assert!(
        abandoned + SLACK >= DEFAULT_FETCH_TIMEOUT
            && abandoned <= DEFAULT_FETCH_TIMEOUT + Duration::from_secs(2),
        "the request was abandoned {abandoned:?} after it was made"
    );
    assert!(check.retries.is_empty(), "{:?}", check.retries);
    let failure = check.camera_failure(&world).await;
    assert!(failure.detail.contains("too long"), "{failure:?}");
    world.server.stop().await;
}

/// A store that refuses every retry's record costs the check nothing: the
/// camera is still asked again after each `500`, the backoff still kept, and
/// the check carries the frame the third attempt gave, with no port failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_retry_the_store_will_not_write_down_is_still_made() {
    let camera = ScriptedHost::answering(vec![
        Scripted::exit_status_69(),
        Scripted::exit_status_69(),
        Scripted::frame(snapshot_bytes()),
    ])
    .await;
    let held: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<HeldEvents>>> =
        std::sync::Arc::default();
    let holding = std::sync::Arc::clone(&held);
    let world = checking_over(camera.url(), move |stores| {
        let (stores, events) = HeldEvents::over(stores);
        events.refusing_kind(printobserver_core::CameraRetryPayload::KIND, true);
        let _ = holding.set(events);
        stores
    })
    .await;
    let print_id = world.open_print().await;

    let check = FirstCheck::of(&world, print_id).await;

    assert!(
        held.get().is_some(),
        "the store standing in front of the server was not the one refusing"
    );
    assert!(
        check.observation.image.is_some(),
        "the check does not carry the frame the third attempt gave"
    );
    assert!(check.retries.is_empty(), "{:?}", check.retries);
    let exchanges = exchanges_before(&camera, &check);
    assert_eq!(exchanges.len(), 3, "{exchanges:?}");
    assert_spaced(&exchanges, false);
    assert_eq!(check.failures(&world).await, Vec::new());
    assert!(
        stored(&world, print_id)
            .await
            .iter()
            .all(|event| event.kind().as_str() != printobserver_core::CameraRetryPayload::KIND),
        "a refused retry was written down"
    );
    world.server.stop().await;
}
