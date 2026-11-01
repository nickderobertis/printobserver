//! SPIKE ONLY (check-costs, never landed): how long a scheduled check takes
//! from its due moment to its last append, with a camera answering `500
//! exit status 69` at once on every attempt.
//!
//! The driver sleeps one interval after a round returns, so round `k+1` is due
//! at the last append of round `k` plus the interval. The rest of the check
//! path — the printer and job reads, settling the open prints, the
//! observation's append, the camera's (instant) refusal and the record of it —
//! is the last append of round `k+1` minus that due moment.
//!
//! `cargo test -p printobserver-server --test api spike_check_delay -- --ignored --nocapture`

use core::time::Duration;
use std::sync::Arc;

use printobserver_core::store::HistoryQuery;
use printobserver_core::PeriodicObservationPayload;

use crate::agent::StandInAgent;
use crate::http_host::Host;
use crate::printer::RecordingPrinter;
use crate::world::{World, set};

const ROUNDS: usize = 41;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spike measurement, run by spikes/check-costs"]
async fn spike_check_delay() {
    let camera = Host::answering(
        "500 Internal Server Error",
        "text/plain",
        b"exit status 69".to_vec(),
    )
    .await;
    let url = camera.url();
    let agent = StandInAgent::new();
    let world = World::configured(RecordingPrinter::printing(), Arc::clone(&agent), |document| {
        set(document, "supervisor.observation_interval_s", toml::Value::Integer(1));
        let mut table = toml::Table::new();
        table.insert("snapshot_url".to_owned(), toml::Value::String(url));
        set(document, "camera", toml::Value::Table(table));
    })
    .await;
    let print_id = world.open_print().await;
    let read = || async {
        let mut events = world
            .stores
            .events
            .history(HistoryQuery {
                print_id,
                kinds: Vec::new(),
                since: None,
                until: None,
                limit: Some(1_000),
            })
            .await
            .expect("the history reads");
        events.reverse();
        events
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(ROUNDS as u64 * 3 + 30);
    let events = loop {
        let events = read().await;
        let rounds = events
            .iter()
            .filter(|e| e.payload_as::<PeriodicObservationPayload>().is_some())
            .count();
        if rounds > ROUNDS {
            break events;
        }
        assert!(tokio::time::Instant::now() < deadline, "only {rounds} rounds ran");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    world.server.stop().await;

    // Group the history into rounds, each starting at its observation.
    let mut rounds = Vec::new();
    for event in &events {
        let at = event.received_at.as_utc().timestamp_micros();
        if event.payload_as::<PeriodicObservationPayload>().is_some() {
            rounds.push(Vec::new());
        }
        // The turn's own records (its assessment) are written by the turn,
        // which runs beside the next sleep, not by the round.
        let kind = event.kind().to_string();
        if kind != "periodic_observation" && kind != "port_failure" {
            continue;
        }
        if let Some(round) = rounds.last_mut() {
            let round: &mut Vec<_> = round;
            round.push((kind, at));
        }
    }
    let rounds = &rounds[..ROUNDS];
    let kinds: Vec<String> = rounds[1].iter().map(|(k, _)| k.clone()).collect();
    let ms = |later: i64, earlier: i64| (later - earlier) as f64 / 1000.0;
    let mut rest_ms = Vec::new();
    let mut observation_after_due_ms = Vec::new();
    for pair in rounds.windows(2) {
        let previous_end = pair[0].last().expect("a round has its observation").1;
        let end = pair[1].last().expect("a round has its observation").1;
        let observed = pair[1][0].1;
        rest_ms.push(ms(end, previous_end) - 1000.0);
        observation_after_due_ms.push(ms(observed, previous_end) - 1000.0);
    }
    println!(
        "SPIKE_CHECK_DELAY {}",
        serde_json::json!({
            "rounds_measured": rest_ms.len(),
            "round_event_kinds": kinds,
            "due_to_last_append_ms": rest_ms,
            "due_to_observation_append_ms": observation_after_due_ms,
        })
    );
}
