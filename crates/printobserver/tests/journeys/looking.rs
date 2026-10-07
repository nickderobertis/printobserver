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
//! a look inside it, and every one in which the agent acts, because the agent
//! acts with the credential its turn was minted and with no other — are
//! `watching.rs`'s, because the harness they stand in is a shell script.

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

/// A look with a camera answers the camera's frame, at a path whose bytes are
/// that frame, and keeps it in the print's history as the look's own image.
#[test]
fn a_look_answers_the_cameras_frame_and_keeps_it() {
    let world = World::open(STOOD_IN);
    let look = running::read(
        &world,
        &["look", "--print-id", &world.print_id(), "--wait-s", "0"],
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
    let kept = events_of(&world, &world.print_id(), "camera_look");
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
    let look = running::read(&world, &["look", "--print-id", &world.print_id()]);

    assert!(look.get("frame").is_none(), "{look}");
    assert!(look.get("image_path").is_none(), "{look}");
    assert!(look["printer"].is_object(), "{look}");
    assert_eq!(events_of(&world, &world.print_id(), "camera_look").len(), 1);
    assert!(events_of(&world, &world.print_id(), "port_failure").is_empty());
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
        &["look", "--print-id", &world.print_id(), "--wait-s", "91"],
    );

    assert_eq!(
        ran.code,
        Some(i32::from(Exit::Refused.status())),
        "{}",
        ran.said()
    );
    assert!(ran.said().contains("90"), "{}", ran.said());
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(events_of(&world, &world.print_id(), "camera_look").is_empty());
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
        let look = running::read(&world, &["look", "--print-id", &world.print_id()]);

        assert!(look.get("frame").is_none(), "{look}");
        let failures = events_of(&world, &world.print_id(), "port_failure");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0]["payload"]["site"], "camera_look");
        assert_eq!(failures[0]["payload"]["event_id"], look["event"]["id"]);
        let looked = events_of(&world, &world.print_id(), "camera_look");
        assert_eq!(looked.len(), 1);
        assert_eq!(looked[0]["id"], look["event"]["id"]);
        assert!(looked[0].get("image").is_none());
        assert_eq!(failing.received().len(), 1);
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
        !events_of(&world, &world.print_id(), "port_failure").is_empty()
    });
    std::thread::sleep(Duration::from_secs(23));

    assert_eq!(reports(&world), Some(Reports::Paused));
    assert!(
        actions_asked(&world, &world.print_id()).is_empty(),
        "something was asked of a print nobody adjusted"
    );
    assert!(
        api.received().is_empty(),
        "Obico was told about a pause nobody handled"
    );
    let look = running::read(&world, &["look", "--print-id", &world.print_id()]);
    assert_eq!(look["detector_paused"], true, "{look}");
}
