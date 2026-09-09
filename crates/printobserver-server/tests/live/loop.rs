//! The whole loop, walked once against the machine and the harness.
//!
//! One walk rather than several tests, because every step acts on one shared
//! machine and the order they run in is part of what is proven: an adjustment
//! needs a running print, a resume needs a paused one, and a cancel ends the
//! print the steps before it needed. A runner free to interleave them would be
//! proving something else.

use printobserver_types::serde_json::{Value, json};
use printobserver_types::{EventKind, PrintId};

use crate::composition::{Composed, SECRET, SESSION};
use crate::http_host::image_host;
use crate::scripted::Scripted;
use crate::waiting::until;

/// The file the scripted environment uploads and this walk starts.
const HOLD_FILE: &str = "hold.gcode";

/// Obico's own identifier for the print this walk's alerts are about.
const OBICO_PRINT: i64 = 4211;

/// The bytes this walk's own image host serves.
fn snapshot_bytes() -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8, 0xff];
    bytes.extend(b"printobserver-integration-snapshot".repeat(8));
    bytes
}

/// The committed `Obico` failure alert, naming an image host this walk serves.
fn alert_body(image_url: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../printobserver-types/samples/obico/failure-alert.json");
    let mut sample: Value = printobserver_types::serde_json::from_str(
        &std::fs::read_to_string(&path).expect("the committed sample reads"),
    )
    .expect("the committed sample is JSON");
    sample["print"]["id"] = json!(OBICO_PRINT);
    sample["img_url"] = json!(image_url);
    sample.to_string()
}

/// One body carrying a reason and an operator, plus whatever else is given.
fn body(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({ "reason": "the integration tier is asking", "actor": "operator" });
    let object = body.as_object_mut().expect("the body is an object");
    for (name, value) in extra {
        object.insert((*name).to_owned(), value.clone());
    }
    body
}

/// Deliver one alert to the real ingress, and wait its handling out.
async fn deliver(world: &Composed, image_url: &str) {
    let mut completions = world.server.completions();
    let status = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(alert_body(image_url))
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(
        status,
        reqwest::StatusCode::ACCEPTED,
        "the real ingress did not take the committed sample"
    );
    completions.changed().await.expect("the handling finishes");
}

/// The print this walk's alerts opened.
async fn print_of(world: &Composed) -> PrintId {
    world
        .store
        .print_by_obico_id(OBICO_PRINT)
        .await
        .expect("the print reads")
        .expect("the alert opened a print")
        .id
}

/// One status read of the print.
async fn status(world: &Composed, print_id: PrintId) -> Value {
    let (code, answer) = world.get(&world.operation_url("status", print_id)).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{answer}");
    answer
}

/// Wait until the machine reports one connection state.
async fn until_state(world: &Composed, print_id: PrintId, wanted: &str) {
    until(&format!("the machine to report {wanted}"), || async {
        let seen = status(world, print_id).await["printer"]["connection"].clone();
        if seen == json!(wanted) {
            None
        } else {
            Some(seen.to_string())
        }
    })
    .await;
}

/// Ask for one action, and answer what the server said.
async fn act(world: &Composed, print_id: PrintId, name: &str, body: &Value) -> (u16, Value) {
    let (code, answer) = world.post(&world.operation_url(name, print_id), body).await;
    (code.as_u16(), answer)
}

/// Start the hold print, and wait until the machine says it is printing.
async fn hold_the_print_running(world: &Composed, print_id: PrintId) {
    let state = status(world, print_id).await["printer"]["connection"].clone();
    if state == json!("paused") {
        let (code, answer) = act(world, print_id, "cancel", &body(&[])).await;
        assert_eq!(code, 200, "the paused print was not cancelled: {answer}");
        until_state(world, print_id, "operational").await;
    }
    if status(world, print_id).await["printer"]["connection"] != json!("printing") {
        let (code, answer) = act(
            world,
            print_id,
            "start_print",
            &body(&[("file_name", json!(HOLD_FILE)), ("manifest", manifest())]),
        )
        .await;
        assert_eq!(code, 200, "the hold print did not start: {answer}");
    }
    until_state(world, print_id, "printing").await;
}

/// The manifest a start is bounded by, naming the file it is about.
fn manifest() -> Value {
    json!({
        "file_name": HOLD_FILE,
        "material": "PLA",
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": "0.20mm QUALITY",
        "allowed": { "feedrate": { "min": 0.8, "max": 1.2 } },
        "metadata": {},
    })
}

/// The whole loop.
pub async fn walk(instance: &Scripted) {
    let host = image_host(snapshot_bytes()).await;
    let world = Composed::open(instance).await;

    // An alert reaches the real ingress; the print and its image are stored and
    // a session opens through the real harness.
    deliver(&world, &host.url()).await;
    let print_id = print_of(&world).await;
    let recorded = history(&world, print_id).await;
    assert!(
        recorded.contains(&EventKind::ObicoFailureAlert),
        "the alert was not recorded: {recorded:?}"
    );
    assert!(
        recorded.contains(&EventKind::SupervisionSessionOpened),
        "no session opened through the harness: {recorded:?}"
    );
    let opened = status(&world, print_id).await;
    assert_eq!(
        opened["session"]["session_name"],
        json!(SESSION),
        "the session is not the one the harness reported"
    );
    stored_image_is_a_path(&world, print_id).await;

    hold_the_print_running(&world, print_id).await;

    bounded_and_executed(&world, print_id).await;
    paused_and_resumed(&world, print_id).await;
    cancelled_and_started(&world, print_id).await;

    // Every step is accounted for in the history the API reads afterwards.
    let after = history(&world, print_id).await;
    for wanted in [
        EventKind::ObicoFailureAlert,
        EventKind::SupervisionSessionOpened,
        EventKind::AgentAssessment,
    ] {
        assert!(
            after.contains(&wanted),
            "the history does not account for {wanted:?}: {after:?}"
        );
    }

    // Restart, and a second alert continues the first alert's session.
    let world = world.restart().await;
    assert!(
        world.server.reconciliation().adopted.contains(&print_id),
        "the restart did not adopt the print it was watching: {:?}",
        world.server.reconciliation()
    );
    deliver(&world, &host.url()).await;
    let continued = status(&world, print_id).await;
    assert_eq!(
        continued["session"]["session_name"],
        json!(SESSION),
        "the second alert did not continue the first alert's session"
    );
    let reconciled = history(&world, print_id).await;
    assert!(
        reconciled.contains(&EventKind::StartupReconciliation),
        "the restart recorded none of what it adopted: {reconciled:?}"
    );

    // Leave the environment as the bring-up recipe left it.
    hold_the_print_running(&world, print_id).await;
    world.server.stop().await;
}

/// The image the alert brought is a path on this host whose contents are what
/// the record declares.
async fn stored_image_is_a_path(world: &Composed, print_id: PrintId) {
    let image = world
        .store
        .history(printobserver_store_api::HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: None,
        })
        .await
        .expect("the history reads")
        .into_iter()
        .find_map(|event| event.image)
        .expect("the alert's image was stored");
    let url = format!(
        "http://{}{}",
        world.server.address(),
        printobserver_server::operation("image")
            .expect("image is served")
            .full_path()
            .replace("{image_id}", &image.id.to_string())
    );
    let (code, answer) = world.get(&url).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{answer}");
    let path = std::path::PathBuf::from(
        answer["path"]
            .as_str()
            .unwrap_or_else(|| panic!("the image answer carries no path: {answer}")),
    );
    assert!(path.is_absolute() && path.exists(), "{}", path.display());
    assert_eq!(
        digest_at(&path),
        answer["record"]["sha256"].as_str().expect("a digest"),
        "what is at the path this server answered is not what the record declares"
    );
}

/// An adjustment the policy bounds, and one the real machine executes.
async fn bounded_and_executed(world: &Composed, print_id: PrintId) {
    // The policy bounds it: the manifest narrows the feedrate to 0.8..1.2, so a
    // value the envelope alone would allow is refused, carrying its reason, the
    // value asked for and the range allowed.
    let (code, refused) = act(
        world,
        print_id,
        "set_feedrate_factor",
        &body(&[("factor", json!(1.4))]),
    )
    .await;
    assert_eq!(
        code, 409,
        "a value outside the bounds was accepted: {refused}"
    );
    let bounds = &refused["record"]["decision"]["rejected"]["out_of_bounds"];
    assert_eq!(bounds["adjustable"], json!("feedrate"), "{refused}");
    assert_eq!(bounds["requested"], json!(1.4), "{refused}");
    assert!(!bounds["allowed"].is_null(), "{refused}");

    // The bed target is one OctoPrint reports, so its effect is read back.
    let (code, accepted) = act(
        world,
        print_id,
        "set_bed_target_c",
        &body(&[("target_c", json!(61.0))]),
    )
    .await;
    assert_eq!(code, 200, "the bed target was not accepted: {accepted}");
    until(
        "the machine to report the bed target it was given",
        || async {
            let seen = status(world, print_id).await["printer"]["bed"]["target_c"]["value"].clone();
            if seen == json!(61.0) {
                None
            } else {
                Some(seen.to_string())
            }
        },
    )
    .await;

    // The tool target likewise.
    let (code, accepted) = act(
        world,
        print_id,
        "set_tool_target_c",
        &body(&[("tool", json!(0)), ("target_c", json!(205.0))]),
    )
    .await;
    assert_eq!(code, 200, "the tool target was not accepted: {accepted}");
    until(
        "the machine to report the tool target it was given",
        || async {
            let seen =
                status(world, print_id).await["printer"]["tools"][0]["target_c"]["value"].clone();
            if seen == json!(205.0) {
                None
            } else {
                Some(seen.to_string())
            }
        },
    )
    .await;

    // The three OctoPrint reports nothing about: the observable is that the
    // real instance accepted the request, which it answers only for a request
    // it understood.
    for (name, value) in [
        ("set_feedrate_factor", ("factor", json!(1.1))),
        ("set_flowrate_factor", ("factor", json!(1.05))),
        ("set_fan_percent", ("percent", json!(55.0))),
    ] {
        let (code, answer) = act(world, print_id, name, &body(&[value])).await;
        assert_eq!(code, 200, "`{name}` was not accepted: {answer}");
        assert_eq!(
            answer["record"]["outcome"],
            json!("succeeded"),
            "`{name}` reached the real instance and it did not accept it: {answer}"
        );
    }

    // The optional duration is applied: the intervention carries an expiry
    // derived from it.
    let (code, bounded) = act(
        world,
        print_id,
        "set_feedrate_factor",
        &body(&[("factor", json!(1.05)), ("duration_s", json!(600))]),
    )
    .await;
    assert_eq!(code, 200, "{bounded}");
    assert!(
        !bounded["intervention"].is_null(),
        "a bounded adjustment opened no intervention: {bounded}"
    );
}

/// The machine pauses and resumes through the API.
async fn paused_and_resumed(world: &Composed, print_id: PrintId) {
    let (code, answer) = act(world, print_id, "pause", &body(&[])).await;
    assert_eq!(code, 200, "the print was not paused: {answer}");
    until_state(world, print_id, "paused").await;

    let (code, answer) = act(world, print_id, "resume", &body(&[])).await;
    assert_eq!(code, 200, "the print was not resumed: {answer}");
    until_state(world, print_id, "printing").await;
}

/// The machine cancels, starts, and stops on an acknowledgement.
async fn cancelled_and_started(world: &Composed, print_id: PrintId) {
    let (code, answer) = act(world, print_id, "cancel", &body(&[])).await;
    assert_eq!(code, 200, "the print was not cancelled: {answer}");
    until_state(world, print_id, "operational").await;

    let (code, answer) = act(
        world,
        print_id,
        "start_print",
        &body(&[("file_name", json!(HOLD_FILE)), ("manifest", manifest())]),
    )
    .await;
    assert_eq!(code, 200, "the print was not started: {answer}");
    until_state(world, print_id, "printing").await;

    // Acknowledging with `stop` is the one disposition that asks the machine
    // for something, and stopping a print is cancelling it.
    let event_id = printobserver_types::EventId::new();
    let (code, answer) = act(
        world,
        print_id,
        "acknowledge_failure",
        &body(&[
            ("event_id", json!(event_id)),
            ("disposition", json!("stop")),
        ]),
    )
    .await;
    assert_eq!(code, 200, "the acknowledgement was not accepted: {answer}");
    until_state(world, print_id, "operational").await;
}

/// Every kind the print's history carries.
async fn history(world: &Composed, print_id: PrintId) -> Vec<EventKind> {
    let (code, answer) = world.get(&world.operation_url("history", print_id)).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{answer}");
    answer["events"]
        .as_array()
        .expect("a history is a list")
        .iter()
        .filter_map(|event| printobserver_types::serde_json::from_value(event["kind"].clone()).ok())
        .collect()
}

/// The digest of what is at one path, lowercase hexadecimal.
fn digest_at(path: &std::path::Path) -> String {
    use sha2::{Digest as _, Sha256};
    let bytes = std::fs::read(path).expect("the image reads");
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
