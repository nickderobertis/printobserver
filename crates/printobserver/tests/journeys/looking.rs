//! A fresh look, and a print the detector paused, from the command line.
//!
//! Every journey here starts the real supervisor with the one command that
//! starts one, drives it with this program's own commands, and reads what
//! happened back through them. What stands on the far side of the
//! supervisor's own HTTP client is a socket on the loopback address: the
//! camera a look takes its frame from, and the `Obico` API a handled detection
//! is acknowledged to.
//!
//! The journeys that need a supervision turn running — an event handed to it,
//! a look inside it — are `watching.rs`'s, because the harness they stand in is
//! a shell script.

use std::io::{Read as _, Write as _};
use std::time::{Duration, Instant};

use printobserver::failure::Exit;
use printobserver_server::{INGRESS_PATH, TOKEN_PARAM};
use printobserver_types::serde_json::{Value, json};

use crate::host::{Answer, Host};
use crate::machine::Reports;
use crate::world::{FRAME_PATH, SECRET, STOOD_IN, World, committed_skill};

use super::running;

/// The token every journey here configures `Obico`'s API with, which nothing
/// the supervisor writes or prints may carry.
pub const OBICO_TOKEN: &str = "an-obico-token-nothing-may-print";

/// `Obico`'s own identifier for the printer the committed alert is about.
pub const OBICO_PRINTER: i64 = 17;

/// The actor a command run as the agent names, in a session no turn opened.
pub const AGENT: &str = r#"{"agent":{"session_name":"a-journey-acting-as-the-agent"}}"#;

/// How long a journey waits for something the supervisor does on its own.
pub const PATIENCE: Duration = Duration::from_secs(60);

/// Wait until a condition holds, or fail naming what never happened.
pub fn wait_for(what: &str, within: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("{what} did not happen within {within:?}");
}

/// One `Obico` failure alert about a print, as the producer posts it, naming
/// the frame this world's camera answers as its image.
pub fn alert(world: &World, obico_print: i64, file: &str, warning: bool, paused: bool) -> Value {
    let mut alert: Value = printobserver_types::serde_json::from_str(include_str!(
        "../../../printobserver-obico/samples/obico/failure-alert.json"
    ))
    .expect("the committed sample is JSON");
    alert["print"]["id"] = json!(obico_print);
    alert["print"]["filename"] = json!(file);
    alert["event"]["is_warning"] = json!(warning);
    alert["event"]["print_paused"] = json!(paused);
    alert["img_url"] = json!(world.camera.url(FRAME_PATH));
    alert
}

/// Post one alert to the supervisor's own ingress, and require it was taken.
pub fn post(world: &World, alert: &Value) {
    let body = alert.to_string();
    let address = world.proxy.address;
    let mut stream = std::net::TcpStream::connect(address).expect("the supervisor is reachable");
    write!(
        stream,
        "POST {INGRESS_PATH}?{TOKEN_PARAM}={SECRET} HTTP/1.1\r\nHost: {address}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("the alert is written");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    assert!(
        answer.lines().next().unwrap_or_default().contains("202"),
        "the ingress did not take the alert: {answer}"
    );
}

/// The print's events, newest first, read back through this program.
pub fn history(world: &World, print: &str) -> Vec<Value> {
    running::read(world, &["history", "--print-id", print, "--limit", "200"])["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The print's events of one kind, oldest first.
pub fn events_of(world: &World, print: &str, kind: &str) -> Vec<Value> {
    let mut found: Vec<Value> = history(world, print)
        .into_iter()
        .filter(|event| event["kind"] == kind)
        .collect();
    found.reverse();
    found
}

/// The actions the print's history records being asked for, oldest first, as
/// the action each asked for.
pub fn actions_asked(world: &World, print: &str) -> Vec<Value> {
    events_of(world, print, "action_requested")
        .into_iter()
        .map(|event| event["payload"]["action"].clone())
        .collect()
}

/// What the machine reports it is doing, read through this program.
pub fn reports(world: &World) -> Option<Reports> {
    world.reported_state()
}

/// The configuration a journey about the detector's pause runs under: the
/// agent may adjust the fan and acknowledge, the system may resume when
/// `system_resumes`, and `Obico`'s API is the host given.
pub fn detector_configuration(obico: &Host, system_resumes: bool) -> impl FnOnce(&mut Value) {
    let url = obico.url("");
    move |document: &mut Value| {
        let actions = &mut document["safety"]["actions"];
        actions["agent"] = json!(["set_fan_percent", "acknowledge_failure"]);
        if system_resumes {
            actions["system"]
                .as_array_mut()
                .expect("the system's grants are a list")
                .push(json!("resume"));
        }
        document["obico"] = json!({ "url": url, "access_token": OBICO_TOKEN });
    }
}

/// `Obico`'s API, answering every request with `status`.
pub fn obico(status: &'static str) -> Host {
    Host::answering(Answer {
        status,
        content_type: "application/json",
        body: b"{}".to_vec(),
    })
}

/// Whether `Obico` was asked to acknowledge the alert on the committed
/// printer, with the overwrite `FAILED` and the configured token.
pub fn acknowledged(obico: &Host) -> bool {
    obico.received().iter().any(|head| {
        head.starts_with(&format!(
            "POST /api/v1/printers/{OBICO_PRINTER}/acknowledge_alert/?alert_overwrite=FAILED "
        )) && head
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {OBICO_TOKEN}"))
    })
}

/// A look with a camera answers the camera's frame, at a path whose bytes are
/// that frame, and keeps it in the print's history as the look's own image.
#[test]
fn a_look_answers_the_cameras_frame_and_keeps_it() {
    let world = World::open(STOOD_IN);
    let look = running::read(
        &world,
        &["look", "--print-id", &world.print_id, "--wait-s", "0"],
    );

    assert_eq!(look["frame"]["sha256"], World::frame_digest(), "{look}");
    let path = look["image_path"].as_str().expect("the frame's path");
    assert_eq!(
        std::fs::read(path).expect("the frame's path opens"),
        crate::world::FRAME_BYTES
    );
    assert_eq!(look["event"]["kind"], "camera_look");
    assert_eq!(look["event"]["payload"]["waited_s"], 0);
    assert_eq!(look["detector_paused"], false);
    assert!(
        look["printer"].is_object() && look["job"].is_object(),
        "{look}"
    );
    let kept = events_of(&world, &world.print_id, "camera_look");
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["id"], look["event"]["id"]);
    assert_eq!(kept[0]["image"]["sha256"], World::frame_digest());
    assert_eq!(
        world.camera.received().len(),
        1,
        "the camera was not asked exactly once"
    );
}

/// With no camera configured a look carries the printer and no frame, and
/// records no failure: there was no camera to fail.
#[test]
fn a_look_with_no_camera_carries_no_frame() {
    let world = World::configured(STOOD_IN, &committed_skill(), None, |document| {
        document
            .as_object_mut()
            .expect("the configuration is a table")
            .remove("camera");
    });
    let look = running::read(&world, &["look", "--print-id", &world.print_id]);

    assert!(look.get("frame").is_none(), "{look}");
    assert!(look.get("image_path").is_none(), "{look}");
    assert!(look["printer"].is_object(), "{look}");
    assert_eq!(events_of(&world, &world.print_id, "camera_look").len(), 1);
    assert!(events_of(&world, &world.print_id, "port_failure").is_empty());
    assert!(world.camera.received().is_empty());
}

/// A wait above ninety seconds is refused rather than shortened, and nothing
/// is looked at or written down.
#[test]
fn a_wait_above_ninety_is_refused() {
    let world = World::open(STOOD_IN);
    let started = Instant::now();
    let ran = running::command(
        &world,
        &["look", "--print-id", &world.print_id, "--wait-s", "91"],
    );

    assert_eq!(
        ran.code,
        Some(i32::from(Exit::Refused.status())),
        "{}",
        ran.said()
    );
    assert!(ran.said().contains("90"), "{}", ran.said());
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(events_of(&world, &world.print_id, "camera_look").is_empty());
    assert!(world.camera.received().is_empty());
}

/// A camera that fails, or answers something that is not an image, is
/// recorded as a port failure at the camera against the look — which is
/// already in the history — and the look answers with no frame.
#[test]
fn a_camera_that_gives_no_frame_is_recorded_against_the_look() {
    for camera in [
        Answer {
            status: "500 Internal Server Error",
            content_type: "text/plain",
            body: b"the camera is not ready".to_vec(),
        },
        Answer {
            status: "200 OK",
            content_type: "text/html",
            body: b"<html>a login page, not a frame</html>".to_vec(),
        },
    ] {
        let failing = Host::answering(camera);
        let url = failing.url(FRAME_PATH);
        let world = World::configured(STOOD_IN, &committed_skill(), None, |document| {
            document["camera"] = json!({ "snapshot_url": url });
        });
        let look = running::read(&world, &["look", "--print-id", &world.print_id]);

        assert!(look.get("frame").is_none(), "{look}");
        let failures = events_of(&world, &world.print_id, "port_failure");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0]["payload"]["site"], "camera_look");
        assert_eq!(failures[0]["payload"]["event_id"], look["event"]["id"]);
        let looked = events_of(&world, &world.print_id, "camera_look");
        assert_eq!(looked.len(), 1);
        assert_eq!(looked[0]["id"], look["event"]["id"]);
        assert!(looked[0].get("image").is_none());
        assert_eq!(failing.received().len(), 1);
    }
}

/// An agent's adjustment while the detector's pause holds the print is applied
/// at once; twenty seconds after its last such adjustment — a second one moves
/// the resume on — the system resumes the print through the policy and
/// acknowledges the alert to `Obico`, and nothing the supervisor wrote or
/// printed carries `Obico`'s token.
#[test]
fn an_adjustment_under_the_detectors_pause_earns_a_resume_after_the_grace() {
    let api = obico("200 OK");
    let world = World::configured(
        STOOD_IN,
        &committed_skill(),
        None,
        detector_configuration(&api, true),
    );
    world.wants(Reports::Paused);
    post(
        &world,
        &alert(&world, 4211, crate::machine::RUNNING_FILE, false, true),
    );
    wait_for("the alert's turn to be over", PATIENCE, || {
        !events_of(&world, &world.print_id, "port_failure").is_empty()
    });

    let adjusted = Instant::now();
    let ran = running::command(
        &world,
        &[
            "set-fan-percent",
            "--print-id",
            &world.print_id,
            "--percent",
            "80",
            "--reason",
            "more cooling for the overhang",
            "--actor",
            AGENT,
        ],
    );
    assert_eq!(ran.code, Some(0), "{}", ran.said());
    let look = running::read(&world, &["look", "--print-id", &world.print_id]);
    assert_eq!(look["detector_paused"], true, "{look}");

    std::thread::sleep(Duration::from_secs(10));
    assert_eq!(
        reports(&world),
        Some(Reports::Paused),
        "resumed inside the grace"
    );
    let again = Instant::now();
    let ran = running::command(
        &world,
        &[
            "set-fan-percent",
            "--print-id",
            &world.print_id,
            "--percent",
            "90",
            "--reason",
            "still more cooling for the overhang",
            "--actor",
            AGENT,
        ],
    );
    assert_eq!(ran.code, Some(0), "{}", ran.said());

    wait_for("the print to be resumed", PATIENCE, || {
        reports(&world) == Some(Reports::Printing)
    });
    assert!(
        again.elapsed() >= Duration::from_secs(19),
        "the print was resumed {:?} after the second adjustment, before its grace",
        again.elapsed()
    );
    assert!(adjusted.elapsed() >= Duration::from_secs(29));
    let resumes: Vec<Value> = actions_asked(&world, &world.print_id)
        .into_iter()
        .filter(|action| action["action"] == "resume")
        .collect();
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    assert_eq!(resumes[0]["actor"], "system");
    wait_for("Obico to be told", PATIENCE, || acknowledged(&api));

    let history = Value::Array(history(&world, &world.print_id)).to_string();
    assert!(
        !history.contains(OBICO_TOKEN),
        "the history carries the token"
    );
    assert!(
        !world.said_so_far().contains(OBICO_TOKEN),
        "the supervisor printed the token"
    );
}

/// An acknowledgement `Obico` refuses is recorded as a port failure against
/// the detection's own event, saying what `Obico` answered and never the token.
#[test]
fn an_acknowledgement_obico_refuses_is_recorded_against_the_detection() {
    let api = obico("403 Forbidden");
    let world = World::configured(
        STOOD_IN,
        &committed_skill(),
        None,
        detector_configuration(&api, true),
    );
    world.wants(Reports::Paused);
    post(
        &world,
        &alert(&world, 4211, crate::machine::RUNNING_FILE, false, true),
    );
    wait_for("the alert to be written down", PATIENCE, || {
        !events_of(&world, &world.print_id, "obico_failure_alert").is_empty()
    });
    let detection = events_of(&world, &world.print_id, "obico_failure_alert")
        .pop()
        .expect("the alert")["id"]
        .clone();
    let ran = running::command(
        &world,
        &[
            "set-fan-percent",
            "--print-id",
            &world.print_id,
            "--percent",
            "80",
            "--reason",
            "more cooling for the overhang",
            "--actor",
            AGENT,
        ],
    );
    assert_eq!(ran.code, Some(0), "{}", ran.said());

    let refused = || {
        events_of(&world, &world.print_id, "port_failure")
            .into_iter()
            .find(|failure| failure["payload"]["site"] == "detector_acknowledgement")
    };
    wait_for(
        "the refused acknowledgement to be recorded",
        PATIENCE,
        || refused().is_some(),
    );
    let failure = refused().expect("recorded");
    assert_eq!(failure["payload"]["event_id"], detection);
    let detail = failure["payload"]["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("403"), "{detail}");
    assert!(!detail.contains(OBICO_TOKEN), "{detail}");
    assert_eq!(reports(&world), Some(Reports::Printing));
    assert!(
        !world.said_so_far().contains(OBICO_TOKEN),
        "the supervisor printed the token"
    );
}

/// While the print is paused the agent's minimum interval does not hold its
/// adjustments back; cancelling still waits on it, and once the print is
/// moving an adjustment does too.
#[test]
fn adjustments_to_a_paused_print_skip_the_interval_and_nothing_else_does() {
    let world = World::configured(STOOD_IN, &committed_skill(), None, |document| {
        document["safety"]["agent_min_interval_s"] = json!(300);
        document["safety"]["actions"]["agent"] =
            json!(["set_fan_percent", "set_feedrate_factor", "pause", "cancel"]);
    });
    let asking = |command: &str, value: &[&str]| {
        let mut given = vec![
            command,
            "--print-id",
            &world.print_id,
            "--reason",
            "a journey about the interval",
            "--actor",
            AGENT,
        ];
        given.extend_from_slice(value);
        running::command(&world, &given)
    };
    world.wants(Reports::Paused);
    for (command, value) in [
        ("set-fan-percent", ["--percent", "70"]),
        ("set-feedrate-factor", ["--factor", "0.9"]),
    ] {
        let ran = asking(command, &value);
        assert_eq!(
            ran.code,
            Some(0),
            "`{command}` on a paused print: {}",
            ran.said()
        );
    }
    let cancelled = asking("cancel", &[]);
    assert_eq!(
        cancelled.code,
        Some(i32::from(Exit::Rejected.status())),
        "{}",
        cancelled.said()
    );
    assert!(
        cancelled.said().contains("min_interval_not_elapsed"),
        "{}",
        cancelled.said()
    );

    world.wants(Reports::Printing);
    for (command, value) in [
        ("set-fan-percent", vec!["--percent", "60"]),
        ("pause", vec![]),
    ] {
        let ran = asking(command, &value);
        assert_eq!(
            ran.code,
            Some(i32::from(Exit::Rejected.status())),
            "`{command}` on a moving print did not wait on the interval: {}",
            ran.said()
        );
        assert!(
            ran.said().contains("min_interval_not_elapsed"),
            "{}",
            ran.said()
        );
    }
}

/// With no adjustment asked for, the detector's pause holds past the grace an
/// adjustment would have earned: nothing resumes the print and `Obico` is told
/// nothing.
#[test]
fn without_an_adjustment_the_detectors_pause_holds_past_the_grace() {
    let api = obico("200 OK");
    let world = World::configured(
        STOOD_IN,
        &committed_skill(),
        None,
        detector_configuration(&api, true),
    );
    world.wants(Reports::Paused);
    post(
        &world,
        &alert(&world, 4211, crate::machine::RUNNING_FILE, false, true),
    );
    wait_for("the alert's turn to be over", PATIENCE, || {
        !events_of(&world, &world.print_id, "port_failure").is_empty()
    });
    std::thread::sleep(Duration::from_secs(23));

    assert_eq!(reports(&world), Some(Reports::Paused));
    assert!(
        actions_asked(&world, &world.print_id).is_empty(),
        "something was asked of a print nobody adjusted"
    );
    assert!(
        api.received().is_empty(),
        "Obico was told about a pause nobody handled"
    );
    let look = running::read(&world, &["look", "--print-id", &world.print_id]);
    assert_eq!(look["detector_paused"], true, "{look}");
}

/// A detector that reports the pause again after the agent adjusted the print
/// does not undo the adjustment: the resume it earned still comes, and the
/// newer detection is the one acknowledged — to its own printer, and with a
/// refusal recorded against its own event.
#[test]
fn a_repeated_detection_keeps_the_resume_and_is_the_one_acknowledged() {
    let api = obico("403 Forbidden");
    let world = World::configured(
        STOOD_IN,
        &committed_skill(),
        None,
        detector_configuration(&api, true),
    );
    world.wants(Reports::Paused);
    post(
        &world,
        &alert(&world, 4211, crate::machine::RUNNING_FILE, false, true),
    );
    wait_for("the alert's turn to be over", PATIENCE, || {
        !events_of(&world, &world.print_id, "port_failure").is_empty()
    });
    let ran = running::command(
        &world,
        &[
            "set-fan-percent",
            "--print-id",
            &world.print_id,
            "--percent",
            "80",
            "--reason",
            "more cooling for the overhang",
            "--actor",
            AGENT,
        ],
    );
    assert_eq!(ran.code, Some(0), "{}", ran.said());
    let mut again = alert(&world, 4211, crate::machine::RUNNING_FILE, false, true);
    again["printer"]["id"] = json!(OBICO_PRINTER + 1);
    post(&world, &again);

    let refused = || {
        events_of(&world, &world.print_id, "port_failure")
            .into_iter()
            .find(|failure| failure["payload"]["site"] == "detector_acknowledgement")
    };
    wait_for(
        "the refused acknowledgement to be recorded",
        PATIENCE,
        || refused().is_some(),
    );
    assert_eq!(reports(&world), Some(Reports::Printing));
    let resumes = actions_asked(&world, &world.print_id)
        .into_iter()
        .filter(|action| action["action"] == "resume")
        .count();
    assert_eq!(resumes, 1);
    let newer = events_of(&world, &world.print_id, "obico_failure_alert")
        .pop()
        .expect("the newer detection")["id"]
        .clone();
    assert_eq!(refused().expect("recorded")["payload"]["event_id"], newer);
    let heads = api.received();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(
        heads[0].starts_with(&format!(
            "POST /api/v1/printers/{}/acknowledge_alert/",
            OBICO_PRINTER + 1
        )),
        "{}",
        heads[0]
    );
}
