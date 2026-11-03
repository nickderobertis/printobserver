//! How late a scheduled check whose camera fails every attempt is written
//! down, through the real server: the figure of the budget
//! `scheduled-check-worst-delay-seconds`.
//!
//! One scheduled check of an active print, against a camera on the loopback
//! address that answers `500` `exit status 69` to every attempt and holds each
//! answer until shortly before the vision adapter's default per-fetch timeout.
//! Everything else is as this server ships it: the five-second timeout, three
//! retries half a second apart. The journey records the time from the check's
//! due moment to its event being appended, with each attempt and wait beside
//! it, to `telemetry/scheduled-check-delay.json`; the budget's command reads
//! that file, and `onebudgetspec check` is the only judge of the figure. What
//! this asserts is what the check did — four attempts, three retries written
//! down, one port failure, the check stored with no frame — and no bound on
//! the time.
//!
//! A binary and an Nx project of its own, because its four held answers cost
//! about twenty seconds of wall clock that no other suite should pay.

/// The stand-in supervising agent the journeys' server runs turns through.
#[path = "../support/agent.rs"]
pub mod agent;
/// The scripted camera on the loopback address.
#[path = "../support/camera_host.rs"]
pub mod camera_host;
/// A scheduled check of one print, through the real server.
#[path = "../support/checks.rs"]
pub mod checks;
/// The stand-in machine on the far side of the printer port.
#[path = "../support/printer.rs"]
pub mod printer;
/// A real server, over a real HTTP client.
#[path = "../support/world.rs"]
pub mod world;

use core::time::Duration;
use std::path::PathBuf;

use printobserver_core::{DEFAULT_CAMERA_RETRIES, DEFAULT_CAMERA_RETRY_BACKOFF};
use printobserver_obico::DEFAULT_FETCH_TIMEOUT;
use printobserver_types::{Timestamp, serde_json};

use crate::camera_host::{GO2RTC_FAILURE, Scripted, ScriptedHost};
use crate::checks::{FirstCheck, INTERVAL_S, checking};

/// How long before the per-fetch timeout each refusal is answered: shortly
/// before it, so every attempt costs nearly the whole timeout and none of them
/// times out.
const BEFORE_THE_TIMEOUT: Duration = Duration::from_millis(300);

/// Where the figure is recorded, under this project's own `telemetry/`.
fn telemetry() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("check_delay_budget")
        .join("telemetry")
        .join("scheduled-check-delay.json")
}

/// Seconds from one wall-clock instant to a later one.
///
/// # Panics
///
/// Panics when the later one is earlier, which is a check appended before it
/// was due.
fn seconds_between(from: Timestamp, to: Timestamp) -> f64 {
    let micros = to.as_utc().timestamp_micros() - from.as_utc().timestamp_micros();
    Duration::from_micros(u64::try_from(micros).expect("the check was appended after it was due"))
        .as_secs_f64()
}

/// A camera answering `500` to every attempt, each held until shortly before
/// the timeout, delays its scheduled check by four attempts and three waits:
/// the check is written down with no frame and one port failure, after three
/// retries, and the time it took is recorded as the budget's figure.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_whose_camera_fails_every_attempt_is_recorded_with_its_delay() {
    let held = DEFAULT_FETCH_TIMEOUT
        .checked_sub(BEFORE_THE_TIMEOUT)
        .expect("the per-fetch timeout is longer than the margin before it");
    let camera = ScriptedHost::answering(vec![Scripted::exit_status_69().held(held)]).await;
    // The first round is due one interval after the server starts, and the
    // server starts after this: so the figure measured from here is the delay
    // plus the start, never less than the delay.
    let started = Timestamp::now();
    let world = checking(camera.url()).await;
    let print_id = world.open_print().await;

    let check = FirstCheck::of(&world, print_id).await;
    let failure = check.camera_failure(&world).await;
    let exchanges: Vec<_> = camera
        .exchanges()
        .into_iter()
        .filter(|exchange| exchange.received < check.seen)
        .collect();
    world.server.stop().await;

    let attempts = DEFAULT_CAMERA_RETRIES + 1;
    assert_eq!(
        exchanges.len(),
        usize::try_from(attempts).expect("a count"),
        "{exchanges:?}"
    );
    let retried: Vec<u32> = check.retries.iter().map(|retry| retry.attempt).collect();
    assert_eq!(retried, vec![1, 2, 3], "{:?}", check.retries);
    assert!(
        check
            .retries
            .iter()
            .all(|retry| retry.status == 500 && retry.excerpt == GO2RTC_FAILURE),
        "{:?}",
        check.retries
    );
    assert!(failure.detail.contains(GO2RTC_FAILURE), "{failure:?}");
    assert!(check.observation.image.is_none(), "{:?}", check.observation);

    let interval = Duration::from_secs(u64::try_from(INTERVAL_S).expect("an interval"));
    let due = started
        .plus_seconds(INTERVAL_S)
        .expect("the due moment is a moment");
    let seconds = seconds_between(due, check.observation.received_at);
    let first = exchanges[0].received;
    let durations: Vec<f64> = exchanges
        .iter()
        .map(|exchange| {
            exchange
                .answered
                .expect("every refusal was answered")
                .duration_since(exchange.received)
                .as_secs_f64()
        })
        .collect();
    let waits: Vec<f64> = exchanges
        .windows(2)
        .map(|pair| {
            pair[1]
                .received
                .duration_since(pair[0].answered.expect("a refusal was answered"))
                .as_secs_f64()
        })
        .collect();
    let last = exchanges[exchanges.len() - 1]
        .answered
        .expect("the last refusal was answered");
    let document = serde_json::json!({
        "figures": { "seconds": seconds },
        "detail": {
            "measured": "from the check's due moment — one interval after the moment just \
                         before the server started, so the server's own start is counted — \
                         to the received_at of its periodic_observation event",
            "workload": "one scheduled check of an active print; the camera answering 500 \
                         `exit status 69` to every attempt, each answer held until shortly \
                         before the default per-fetch timeout",
            "per_fetch_timeout_s": DEFAULT_FETCH_TIMEOUT.as_secs_f64(),
            "answer_held_s": held.as_secs_f64(),
            "retries": DEFAULT_CAMERA_RETRIES,
            "backoff_s": DEFAULT_CAMERA_RETRY_BACKOFF.as_secs_f64(),
            "interval_s": interval.as_secs_f64(),
            "attempts": durations.len(),
            "attempt_durations_s": durations,
            "backoffs_s": waits,
            "first_attempt_to_last_answer_s": last.duration_since(first).as_secs_f64(),
            "due_moment": due,
            "appended_at": check.observation.received_at,
        },
    });
    let path = telemetry();
    std::fs::create_dir_all(path.parent().expect("the telemetry directory"))
        .expect("the telemetry directory is writable");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&document).expect("the telemetry renders"),
    )
    .expect("the telemetry is writable");
}
