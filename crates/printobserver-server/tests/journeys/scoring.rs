//! Whether the failure detector is scoring the active print, read through the
//! real server from `Obico`'s own record of it.
//!
//! `Obico`'s REST API is a real loopback host serving the recorded printer
//! answer (`support/obico_api.rs`), whose `current_print`, `pic` and
//! `normalized_p` a journey moves between the server's reads; the detector's
//! `ml_api` is another, answering its `/hc/` as a journey says. What is
//! asserted is what the status and context reads, the histories and the
//! state directory show, and what reached each host.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use printobserver_types::PrintId;
use printobserver_types::serde_json::{Value, json};

use crate::agent::StandInAgent;
use crate::minted::minted;
use crate::obico_api::{AUTH_TOKEN_SENTINEL, Host, Mood};
use crate::printer::RecordingPrinter;
use crate::world::{SECRET, World, failure_alert, files_carrying, set};

/// How a journey waits for what the reads should find.
const PATIENCE: Duration = Duration::from_secs(30);

/// The observation interval every journey here runs at, in seconds.
const INTERVAL_S: u64 = 1;

/// The printer the recorded answer is.
const PRINTER_ID: i64 = 17;

/// The path the printer's record is read at.
const PRINTER_PATH: &str = "/api/v1/printers/17/";

/// A world watching an active print, reading `Obico`'s REST API at `rest`,
/// with the detector's `ml_api` at `ml_api` when one is given and the printer
/// configured when `printer_id` is.
async fn watching(
    rest: &Host,
    ml_api: Option<&Host>,
    printer_id: Option<i64>,
    token: &str,
) -> (World, PrintId) {
    let rest_url = rest.url();
    let ml_api_url = ml_api.map(Host::url);
    let token = token.to_owned();
    let world = World::configured(
        RecordingPrinter::printing(),
        StandInAgent::new(),
        move |document| {
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(i64::try_from(INTERVAL_S).expect("a small interval")),
            );
            let mut obico = toml::Table::new();
            obico.insert("url".to_owned(), toml::Value::String(rest_url));
            obico.insert("access_token".to_owned(), toml::Value::String(token));
            if let Some(url) = ml_api_url {
                obico.insert("ml_api_url".to_owned(), toml::Value::String(url));
            }
            if let Some(id) = printer_id {
                obico.insert("printer_id".to_owned(), toml::Value::Integer(id));
            }
            set(document, "obico", toml::Value::Table(obico));
        },
    )
    .await;
    let print_id = world.open_print().await;
    (world, print_id)
}

/// Move the snapshot `Obico` holds every quarter second, its score left at
/// zero, until the flag is lowered.
fn keep_scoring(rest: &Arc<Host>) -> Arc<AtomicBool> {
    let going = Arc::new(AtomicBool::new(true));
    let still = Arc::clone(&going);
    let host = Arc::clone(rest);
    tokio::spawn(async move {
        let mut snapshot = 0_u64;
        while still.load(Ordering::SeqCst) {
            snapshot += 1;
            host.scores(snapshot, 0.0);
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    going
}

/// The detector, as the print's status read answers it.
async fn status_detector(world: &World, print_id: PrintId) -> Value {
    let (_, status) = world
        .get(&world.operation_url("/v1/prints/{print_id}/status", print_id))
        .await;
    status["detector"].clone()
}

/// The detector, as the print's context read answers it.
async fn context_detector(world: &World, print_id: PrintId) -> Value {
    let (_, context) = world
        .get(&world.operation_url("/v1/prints/{print_id}/context", print_id))
        .await;
    context["context"]["detector"].clone()
}

/// Wait until the status read answers the detector as `status`, and answer
/// what it said.
async fn until_status(world: &World, print_id: PrintId, status: &str) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let detector = status_detector(world, print_id).await;
        if detector["status"] == status {
            return detector;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the detector never read {status}: {detector}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Every port failure one history answers, at `url`.
async fn port_failures(world: &World, url: &str) -> Vec<Value> {
    let (_, answer) = world.get(url).await;
    answer["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| event["kind"] == "port_failure")
        .cloned()
        .collect()
}

/// Wait until a port failure at `site` is in the history at `url`.
async fn until_port_failure(world: &World, url: &str, site: &str) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if let Some(found) = port_failures(world, url)
            .await
            .into_iter()
            .find(|event| event["payload"]["site"] == site)
        {
            return found;
        }
        if tokio::time::Instant::now() >= deadline {
            let (_, all) = world.get(url).await;
            let kinds: Vec<String> = all["events"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|event| format!("{} {}", event["kind"], event["payload"]))
                .collect();
            panic!("no {site} port failure was recorded at {url}: {kinds:#?} {all}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Wait out `rounds` of the server's reads of the printer's record.
async fn reads(rest: &Host, rounds: usize) {
    let before = printer_reads(rest);
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while printer_reads(rest) < before + rounds {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the printer's record was not read {rounds} more times"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// How many times the printer's record has been read.
fn printer_reads(rest: &Host) -> usize {
    rest.received()
        .iter()
        .filter(|request| request.path() == PRINTER_PATH)
        .count()
}

/// Post one body to the ingress, as `Obico`'s webhook plugin does.
async fn post(world: &World, body: &Value) {
    let accepted = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(accepted, reqwest::StatusCode::ACCEPTED);
}

/// With the snapshot changing between reads and no `ml_api_url` at all, the
/// print reads `scoring_nothing_flagged` — while the score stays zero — in its
/// status and context; a failure alert at the webhook makes it
/// `scoring_flagged`; and once the record stops moving for two intervals it
/// reads `unchecked`, with no evidence of any kind. Every read carried the
/// configured bearer token.
#[tokio::test(flavor = "multi_thread")]
async fn a_moving_snapshot_reads_scoring_with_no_ml_api_until_it_stops() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let (world, print_id) = watching(&rest, None, Some(PRINTER_ID), &token).await;
    let scoring = keep_scoring(&rest);

    let detector = until_status(&world, print_id, "scoring_nothing_flagged").await;
    assert!(detector["since"].is_string(), "{detector}");
    assert!(detector.get("probe_failure").is_none(), "{detector}");
    assert_eq!(
        rest.received()
            .iter()
            .find(|request| request.path() == PRINTER_PATH)
            .map(|request| request.line.starts_with("GET ")),
        Some(true)
    );
    let context = context_detector(&world, print_id).await;
    assert_eq!(context["status"], "scoring_nothing_flagged", "{context}");
    assert_eq!(context["since"], detector["since"], "{context}");

    let image = crate::http_host::image_host(
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/recorded-frame.jpg"),
        )
        .expect("the recorded frame reads"),
    )
    .await;
    post(&world, &failure_alert(4211, &image.url())).await;
    let flagged = until_status(&world, print_id, "scoring_flagged").await;
    assert_eq!(flagged["since"], detector["since"], "{flagged}");
    assert_eq!(
        context_detector(&world, print_id).await["status"],
        "scoring_flagged"
    );

    scoring.store(false, Ordering::SeqCst);
    let after = until_status(&world, print_id, "unchecked").await;
    assert_eq!(after["since"], Value::Null, "{after}");

    let bearer = format!("authorization: bearer {token}");
    for request in rest.received() {
        assert!(
            request
                .headers
                .iter()
                .any(|header| header.eq_ignore_ascii_case(&bearer)),
            "{} carried no bearer token",
            request.line
        );
    }
}

/// A score that changes between reads is scoring evidence on its own: with the
/// snapshot left where it is, the print reads `scoring_nothing_flagged`.
#[tokio::test(flavor = "multi_thread")]
async fn a_score_that_moves_alone_is_scoring_evidence() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let (world, print_id) = watching(&rest, None, Some(PRINTER_ID), &token).await;
    let pic = rest.served()["pic"].clone();
    let going = Arc::new(AtomicBool::new(true));
    let still = Arc::clone(&going);
    let host = Arc::clone(&rest);
    tokio::spawn(async move {
        let mut score = 0.0_f64;
        while still.load(Ordering::SeqCst) {
            score += 0.01;
            host.answer(|answer| answer["normalized_p"] = json!(score));
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    until_status(&world, print_id, "scoring_nothing_flagged").await;
    going.store(false, Ordering::SeqCst);
    assert_eq!(
        rest.served()["pic"],
        pic,
        "the snapshot moved, so the score alone proved nothing"
    );
}

/// A print an alert flagged reads `scoring_flagged` again after the server is
/// started again: the flag is read back out of the print's history.
#[tokio::test(flavor = "multi_thread")]
async fn a_flagged_print_reads_flagged_after_a_restart() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let (world, print_id) = watching(&rest, None, Some(PRINTER_ID), &token).await;
    let scoring = keep_scoring(&rest);
    until_status(&world, print_id, "scoring_nothing_flagged").await;
    let image = crate::http_host::image_host(
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/recorded-frame.jpg"),
        )
        .expect("the recorded frame reads"),
    )
    .await;
    post(&world, &failure_alert(4211, &image.url())).await;
    until_status(&world, print_id, "scoring_flagged").await;

    let restarted = world.restart().await;
    let after = until_status(&restarted, print_id, "scoring_flagged").await;
    assert!(after["since"].is_string(), "{after}");
    scoring.store(false, Ordering::SeqCst);
}

/// A read that cannot complete — nothing listening, no answer within the
/// bound, or an answer broken off part-way — is a port failure saying which.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_that_cannot_complete_is_a_port_failure_saying_why() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let (world, print_id) = watching(&rest, None, Some(PRINTER_ID), &token).await;
    let history = world.operation_url("/v1/prints/{print_id}/history", print_id);
    let failed_with = |answer: &Value, said: &str| {
        answer["events"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|event| {
                event["kind"] == "port_failure"
                    && event["payload"]["site"] == "detector_scoring_read"
            })
            .any(|event| {
                event["payload"]["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.contains(said))
            })
    };
    for (mood, said) in [
        (Mood::BreakingOff, "broke off"),
        (Mood::Hanging, "did not answer within"),
    ] {
        rest.answers(mood);
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let (_, answer) = world.get(&format!("{history}?limit=500")).await;
            if failed_with(&answer, said) {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no failed read saying {said:?} was recorded: {answer}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    rest.answers(Mood::Answering);
    rest.stop();
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let (_, answer) = world.get(&format!("{history}?limit=500")).await;
        if failed_with(&answer, "could not be reached") {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no failed read saying it could not reach Obico was recorded: {answer}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let detector = status_detector(&world, print_id).await;
    assert!(
        detector["scoring_unread"]
            .as_str()
            .is_some_and(|why| why.contains("could not be reached")),
        "{detector}"
    );
}

/// The detector's health check is asked as the server starts, not one
/// observation interval later: with an interval of a minute and a print open,
/// a server started again reads the detector `available` within seconds.
#[tokio::test(flavor = "multi_thread")]
async fn the_detector_is_checked_as_the_server_starts() {
    let ml_api = Host::ml_api().await;
    let url = ml_api.url();
    let world = World::configured(
        RecordingPrinter::printing(),
        StandInAgent::new(),
        move |document| {
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(60),
            );
            let mut obico = toml::Table::new();
            obico.insert("ml_api_url".to_owned(), toml::Value::String(url));
            set(document, "obico", toml::Value::Table(obico));
        },
    )
    .await;
    let print_id = world.open_print().await;
    let asked_before = ml_api.received().len();
    let started = Instant::now();
    let restarted = world.restart().await;
    let detector = until_status(&restarted, print_id, "available").await;
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the first check waited for the interval: {detector}"
    );
    assert!(
        ml_api.received().len() > asked_before,
        "the started server never asked the detector"
    );
}

/// With the record moving and a health check that fails, the print still
/// reads `scoring_nothing_flagged`, naming the failing check beside it, and the
/// failure is recorded as a port failure; no turn says nothing is watching.
#[tokio::test(flavor = "multi_thread")]
async fn a_failing_health_check_while_scoring_is_named_beside_it_and_recorded() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let ml_api = Host::ml_api().await;
    let (world, print_id) = watching(&rest, Some(&ml_api), Some(PRINTER_ID), &token).await;
    let scoring = keep_scoring(&rest);
    until_status(&world, print_id, "scoring_nothing_flagged").await;

    ml_api.health("503 Service Unavailable");
    let failure = until_port_failure(
        &world,
        &world.at(&format!(
            "/v1/events?print_id={print_id}&kinds=port_failure&limit=500"
        )),
        "detector_health_check",
    )
    .await;
    let detail = failure["payload"]["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("503"), "{failure}");
    let detector = until_status(&world, print_id, "scoring_nothing_flagged").await;
    let named = detector["probe_failure"].as_str().unwrap_or_default();
    assert!(named.contains("503"), "{detector}");
    assert_eq!(context_detector(&world, print_id).await, detector);
    assert!(
        !world
            .agent
            .turns()
            .iter()
            .any(|turn| turn.event.kind().as_str() == "detector_health_changed"),
        "the agent was told nothing watches a print the detector is scoring"
    );
    assert!(
        ml_api
            .received()
            .iter()
            .all(|request| request.line.starts_with("GET /hc/ ")),
        "{:?}",
        ml_api.received()
    );
    scoring.store(false, Ordering::SeqCst);
}

/// With no evidence, the health check decides: `available` while it answers
/// and the record does not move, `unavailable` once it fails; with no
/// `ml_api_url` and no evidence, `unchecked`.
#[tokio::test(flavor = "multi_thread")]
async fn without_evidence_the_health_check_decides_and_nothing_is_unchecked() {
    let token = minted("obico-token");
    let rest = Host::rest(&token).await;
    let ml_api = Host::ml_api().await;
    let (world, print_id) = watching(&rest, Some(&ml_api), Some(PRINTER_ID), &token).await;

    until_status(&world, print_id, "available").await;
    reads(&rest, 3).await;
    let detector = until_status(&world, print_id, "available").await;
    assert!(detector.get("scoring_unread").is_none(), "{detector}");
    assert!(
        ml_api
            .received()
            .iter()
            .any(|request| request.path() == "/hc/"),
        "the detector's health check was never asked"
    );

    ml_api.health("503 Service Unavailable");
    let detector = until_status(&world, print_id, "unavailable").await;
    assert!(detector["since"].is_string(), "{detector}");
    assert_eq!(context_detector(&world, print_id).await, detector);

    let quiet = Host::rest(&token).await;
    let (unchecked, unchecked_print) = watching(&quiet, None, Some(PRINTER_ID), &token).await;
    reads(&quiet, 3).await;
    assert_eq!(
        status_detector(&unchecked, unchecked_print).await,
        json!({ "status": "unchecked", "since": null })
    );
}

/// A read that fails is recorded as a port failure against the observation it
/// was read for, and the scoring state stands until its evidence is two
/// intervals old; then the status says why the record is not being read.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_read_is_a_port_failure_and_scoring_stands_until_its_evidence_is_old() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let (world, print_id) = watching(&rest, None, Some(PRINTER_ID), &token).await;
    let scoring = keep_scoring(&rest);
    until_status(&world, print_id, "scoring_nothing_flagged").await;
    scoring.store(false, Ordering::SeqCst);

    // The printer this server reads is gone from `Obico`: every read now fails.
    rest.answer(|answer| answer["id"] = json!(18));
    let history = world.operation_url("/v1/prints/{print_id}/history", print_id);
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let failure = loop {
        let (_, answer) = world.get(&history).await;
        if let Some(found) = answer["events"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|event| {
                event["kind"] == "port_failure"
                    && event["payload"]["site"] == "detector_scoring_read"
            })
        {
            break found.clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no failed read was recorded"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let detail = failure["payload"]["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("404"), "{failure}");
    if started.elapsed() < Duration::from_secs(INTERVAL_S) {
        assert_eq!(
            status_detector(&world, print_id).await["status"],
            "scoring_nothing_flagged",
            "the last scoring state did not stand through a failed read"
        );
    }
    let after = until_status(&world, print_id, "unchecked").await;
    let unread = after["scoring_unread"].as_str().unwrap_or_default();
    assert!(
        unread.contains("failed") && unread.contains("404"),
        "{after}"
    );
}

/// Answers whose `current_print` is an earlier print, or another, do not
/// make the active print read a scoring state, changing or not.
#[tokio::test(flavor = "multi_thread")]
async fn a_record_of_another_or_an_earlier_print_scores_nothing_here() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let ml_api = Host::ml_api().await;
    let (world, print_id) = watching(&rest, Some(&ml_api), Some(PRINTER_ID), &token).await;
    until_status(&world, print_id, "available").await;
    let scoring = keep_scoring(&rest);

    for (id, file, started) in [
        (4210, "benchy.gcode", "2026-03-01T09:00:00Z"),
        (5000, "calibration-cube.gcode", "2026-03-01T12:00:00Z"),
    ] {
        rest.answer(|answer| {
            answer["current_print"]["id"] = json!(id);
            answer["current_print"]["filename"] = json!(file);
            answer["current_print"]["started_at"] = json!(started);
        });
        reads(&rest, 4).await;
        let detector = status_detector(&world, print_id).await;
        assert_eq!(detector["status"], "available", "{id}: {detector}");
    }
    rest.answer(|answer| answer["current_print"] = Value::Null);
    reads(&rest, 4).await;
    assert_eq!(
        status_detector(&world, print_id).await["status"],
        "available"
    );
    scoring.store(false, Ordering::SeqCst);
}

/// `Obico` refuses any credential but the bearer `access_token`, as the double
/// does: a server holding another is refused `403`, records the failed read,
/// and never reads a scoring state.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_credential_is_a_failed_read_and_scores_nothing() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let (world, print_id) = watching(&rest, None, Some(PRINTER_ID), &minted("another-token")).await;
    let scoring = keep_scoring(&rest);

    let failure = until_port_failure(
        &world,
        &world.operation_url("/v1/prints/{print_id}/history", print_id),
        "detector_scoring_read",
    )
    .await;
    assert!(
        failure["payload"]["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("403"),
        "{failure}"
    );
    reads(&rest, 3).await;
    let detector = status_detector(&world, print_id).await;
    assert_eq!(detector["status"], "unchecked", "{detector}");
    scoring.store(false, Ordering::SeqCst);
}

/// With no printer configured, nothing is read and the status says why, until
/// `Obico` posts a body naming its printer; from then on that printer's record
/// is read, and the print reads scoring.
#[tokio::test(flavor = "multi_thread")]
async fn the_printer_is_learned_from_what_obico_posts() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let (world, print_id) = watching(&rest, None, None, &token).await;
    let scoring = keep_scoring(&rest);
    tokio::time::sleep(Duration::from_secs(3 * INTERVAL_S)).await;

    let detector = status_detector(&world, print_id).await;
    assert_eq!(detector["status"], "unchecked", "{detector}");
    let unread = detector["scoring_unread"].as_str().unwrap_or_default();
    assert!(
        unread.contains("no Obico printer is known yet"),
        "{detector}"
    );
    assert_eq!(
        printer_reads(&rest),
        0,
        "a printer was read before one was known"
    );

    let notification: Value = printobserver_types::serde_json::from_str(include_str!(
        "../../../printobserver-obico/samples/obico/printer-notification-about-a-print.json"
    ))
    .expect("the committed sample is JSON");
    post(&world, &notification).await;
    until_status(&world, print_id, "scoring_nothing_flagged").await;
    assert!(printer_reads(&rest) > 0);
    scoring.store(false, Ordering::SeqCst);
}

/// The printer's own credential, carried in every answer the double serves,
/// appears nowhere the server writes: not in its state directory — the store,
/// its events and its port failures — nor in any answer it gives.
#[tokio::test(flavor = "multi_thread")]
async fn the_printers_own_credential_is_written_nowhere() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let ml_api = Host::ml_api().await;
    ml_api.health("503 Service Unavailable");
    let (world, print_id) = watching(&rest, Some(&ml_api), Some(PRINTER_ID), &token).await;
    let scoring = keep_scoring(&rest);
    until_status(&world, print_id, "scoring_nothing_flagged").await;
    scoring.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(300)).await;
    // A body this server cannot read whole, so a failed read is recorded too.
    rest.answer(|answer| answer["normalized_p"] = json!("not a score"));
    until_port_failure(
        &world,
        &world.operation_url("/v1/prints/{print_id}/history", print_id),
        "detector_scoring_read",
    )
    .await;
    rest.answer(|answer| answer["normalized_p"] = json!(0.0));
    reads(&rest, 2).await;

    let mut answers = Vec::new();
    for path in [
        world.operation_url("/v1/prints/{print_id}/status", print_id),
        world.operation_url("/v1/prints/{print_id}/context", print_id),
        world.operation_url("/v1/prints/{print_id}/history", print_id),
        world.at("/v1/history"),
        world.at(&format!("/v1/events?print_id={print_id}&limit=1000")),
        world.at("/v1/prints"),
        world.at("/v1/printer/recovery"),
    ] {
        let (_, answer) = world.get(&path).await;
        answers.push(answer.to_string());
    }
    assert!(
        answers
            .iter()
            .any(|answer| answer.contains("detector_scoring_read")),
        "no failed read was recorded to look for the credential in"
    );
    for answer in &answers {
        assert!(
            !answer.contains(AUTH_TOKEN_SENTINEL),
            "an answer carried the printer's credential"
        );
    }
    assert_eq!(
        files_carrying(&world.state_dir(), AUTH_TOKEN_SENTINEL),
        Vec::<std::path::PathBuf>::new(),
        "the state directory carries the printer's credential"
    );
}

/// The reads the detector's status costs, recorded for the
/// `obico-reads-per-print-hour` budget: one active print watched over several
/// observation intervals, every request the REST double received counted by
/// endpoint, and the rounds told apart by the interval between them. It
/// asserts no bound on the hourly figure: the budget's own check is the judge.
#[tokio::test(flavor = "multi_thread")]
async fn the_reads_per_print_hour_are_recorded() {
    let token = minted("obico-token");
    let rest = Arc::new(Host::rest(&token).await);
    let ml_api = Host::ml_api().await;
    let (world, print_id) = watching(&rest, Some(&ml_api), Some(PRINTER_ID), &token).await;
    let scoring = keep_scoring(&rest);
    until_status(&world, print_id, "scoring_nothing_flagged").await;
    let counted_from = rest.received().len();
    reads(&rest, 8).await;
    scoring.store(false, Ordering::SeqCst);

    let counted: Vec<_> = rest.received().into_iter().skip(counted_from).collect();
    let mut by_endpoint = std::collections::BTreeMap::<String, u32>::new();
    for request in &counted {
        let method = request.line.split(' ').next().unwrap_or_default();
        *by_endpoint
            .entry(format!("{method} {}", request.path()))
            .or_default() += 1;
    }
    // A round's requests are made together, and rounds an interval apart.
    let gap = Duration::from_millis(500);
    let mut rounds = 0_u32;
    let mut last: Option<Instant> = None;
    for request in &counted {
        if last.is_none_or(|previous| request.at.duration_since(previous) >= gap) {
            rounds += 1;
        }
        last = Some(request.at);
    }
    let requests = u32::try_from(counted.len()).expect("a count");
    let per_interval = f64::from(requests) / f64::from(rounds);
    let default_interval_s = printobserver_server::DEFAULT_OBSERVATION_INTERVAL_S;
    let intervals_per_hour = u32::try_from(3600 / default_interval_s).expect("a count");
    let per_hour = per_interval * f64::from(intervals_per_hour);
    let per_interval_by_endpoint: std::collections::BTreeMap<String, f64> = by_endpoint
        .iter()
        .map(|(endpoint, count)| (endpoint.clone(), f64::from(*count) / f64::from(rounds)))
        .collect();
    let telemetry = json!({
        "figures": { "requests_per_hour": per_hour },
        "detail": {
            "requests_per_interval": per_interval,
            "requests_per_interval_by_endpoint": per_interval_by_endpoint,
            "requests_by_endpoint": by_endpoint,
            "intervals_counted": rounds,
            "test_interval_s": INTERVAL_S,
            "default_interval_s": default_interval_s,
            "intervals_per_hour": intervals_per_hour,
        },
    });
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("telemetry");
    std::fs::create_dir_all(&directory).expect("the telemetry directory is creatable");
    std::fs::write(
        directory.join("obico-reads.json"),
        format!(
            "{}\n",
            printobserver_types::serde_json::to_string_pretty(&telemetry)
                .expect("the telemetry renders")
        ),
    )
    .expect("the telemetry is writable");
    assert!(rounds >= 6, "only {rounds} rounds were counted");
}
