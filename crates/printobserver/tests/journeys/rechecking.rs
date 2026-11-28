//! A print's camera judged again from the command line, and every look saying
//! which image it was shown and why.
//!
//! The supervisor runs the 2026-11-26 two-colour box's slice on a stood-in
//! machine, calibrated by the stale `core-one-2026-09-27.json` the print ran
//! with, its camera a host on the loopback address showing whichever of that
//! print's recorded frames — plain copies from `printobserver-panels`'
//! fixtures — the journey puts in front of it. Everything is asked and read
//! back through this program's own `recheck-camera`, `look`, `status` and
//! `history`.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use printobserver_types::serde_json::{Value, json};

use crate::machine::Reports;
use crate::world::{Printer, TWO_COLOUR_BOX, World, committed_skill};

use super::running;

/// The reason every recheck here gives.
const REASON: &str = "the camera was nudged while the bed was being cleared";

/// The covered first layer, the toolhead over the part at 13:14.
const COVERED: (&str, u64) = ("131408-periodic_observation.jpg", 24_738);

/// The first layer clearly found at 13:44.
const FOUND: (&str, u64) = ("134401-periodic_observation.jpg", 51_973);

/// A later frame, at 15:28.
const LATER: (&str, u64) = ("152804-periodic_observation.jpg", 74_248);

/// The repository's root.
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One of the two-colour print's recorded frames.
fn frame(file: &str) -> Vec<u8> {
    std::fs::read(
        root()
            .join("crates/printobserver-panels/tests/fixtures/frames/two-colour-planned-change")
            .join(file),
    )
    .expect("the recorded frame is there")
}

/// A frame with the two landmarks found agreeing on every frame of that print
/// painted over, as something standing in front of them would hide them.
fn landmarks_hidden(bytes: &[u8]) -> Vec<u8> {
    let mut decoded = image::load_from_memory(bytes).expect("a frame").to_rgb8();
    for (u, v) in calibration_landmarks() {
        for y in v.saturating_sub(240)..(v + 240).min(decoded.height()) {
            for x in u.saturating_sub(240)..(u + 240).min(decoded.width()) {
                decoded.put_pixel(x, y, image::Rgb([128, 128, 128]));
            }
        }
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
        .encode_image(&decoded)
        .expect("a frame encodes");
    out
}

/// The committed calibration's JSON.
pub fn committed() -> Value {
    printobserver_types::serde_json::from_slice(
        &std::fs::read(root().join("tests/real-prints/cameras/core-one-2026-09-27.json"))
            .expect("the committed calibration"),
    )
    .expect("JSON")
}

/// Where the calibration places the two agreeing landmarks, in pixels.
fn calibration_landmarks() -> Vec<(u32, u32)> {
    committed()["landmarks"]
        .as_array()
        .expect("landmarks")
        .iter()
        .filter(|landmark| {
            ["frame-screws-right-front", "frame-label-right"]
                .contains(&landmark["name"].as_str().unwrap_or_default())
        })
        .map(|landmark| {
            let at = |axis: usize| {
                let value = landmark["center_px"][axis].as_f64().expect("a centre");
                u32::try_from(format!("{value:.0}").parse::<i64>().expect("a pixel"))
                    .expect("on the frame")
            };
            (at(0), at(1))
        })
        .collect()
}

/// A camera on the loopback address answering every request with the frame
/// a journey last put in front of it.
pub struct Camera {
    address: SocketAddr,
    showing: Arc<Mutex<Vec<u8>>>,
}

impl Camera {
    /// Start one showing `frame`.
    pub fn showing(frame: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the bound address");
        let showing = Arc::new(Mutex::new(frame));
        let shown = Arc::clone(&showing);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut head = Vec::new();
                let mut byte = [0_u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                    head.push(byte[0]);
                }
                let body = shown.lock().expect("the frame shown").clone();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        Self { address, showing }
    }

    /// Put `frame` in front of it.
    pub fn shows(&self, frame: Vec<u8>) {
        *self.showing.lock().expect("the frame shown") = frame;
    }

    /// Where it answers.
    pub fn url(&self) -> String {
        format!("http://{}/api/frame.jpeg", self.address)
    }
}

/// Judge the print's camera again as the operator.
fn recheck(world: &World, print_id: &str) -> Value {
    running::read(
        world,
        &[
            "recheck-camera",
            "--print-id",
            print_id,
            "--actor",
            "operator",
            "--reason",
            REASON,
        ],
    )
}

/// The look the operator takes now.
fn look(world: &World, print_id: &str) -> Value {
    running::read(world, &["look", "--print-id", print_id, "--wait-s", "0"])
}

/// The payloads of one kind the print's history holds, oldest first.
fn recorded(world: &World, print_id: &str, kind: &str) -> Vec<Value> {
    let read = running::read(
        world,
        &["history", "--print-id", print_id, "--limit", "500"],
    );
    let mut events = read["events"].as_array().expect("events").clone();
    events.sort_by_key(|event| event["id"].as_str().unwrap_or_default().to_owned());
    events
        .into_iter()
        .filter(|event| event["kind"] == kind)
        .map(|event| event["payload"].clone())
        .collect()
}

/// The SHA-256 of some bytes, as the image store names them.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// `printobserver recheck-camera` judges the print again on a fresh frame it
/// stores — a latched failure included — records each judgement against that
/// frame and answers the stated fields, while the machine is sent nothing;
/// and each `look` says, in its own fields, which image it was shown and why:
/// an inconclusive alignment, a calibration the camera has moved from, a camera
/// moved again, landmarks hidden, and a paused print.
pub fn a_recheck_judges_a_fresh_frame_and_each_look_says_which_image_and_why() {
    let camera = Camera::showing(frame(COVERED.0));
    let held = tempfile::TempDir::new().expect("a directory for the calibration");
    let calibration = held.path().join("calibration.json");
    let mut written = committed();
    written["camera_snapshot_url"] = json!(camera.url());
    std::fs::write(&calibration, written.to_string()).expect("the calibration is written");
    let url = camera.url();
    let world = World::configured(TWO_COLOUR_BOX, &committed_skill(), None, |document| {
        document["camera"] = json!({
            "snapshot_url": url,
            "calibration": calibration.display().to_string(),
        });
    });
    let Printer::StoodIn(machine) = &world.printer else {
        panic!("the {TWO_COLOUR_BOX} world is not a stood-in machine");
    };
    world.wants(Reports::Printing);
    machine.stands_at.store(COVERED.1, Ordering::SeqCst);
    let print_id = world.print_id();
    let commanded = machine.commanded.load(Ordering::SeqCst);

    let status = running::read(&world, &["status", "--print-id", &print_id]);
    assert_eq!(
        status["last_check_image"],
        Value::Null,
        "no check has been shown an image yet: {status}"
    );

    the_covered_layer_and_then_the_found_one_are_judged(&world, &camera, machine, &print_id);
    a_moved_camera_hidden_landmarks_and_a_pause_are_each_said(&world, &camera, machine, &print_id);
    assert_eq!(
        machine.commanded.load(Ordering::SeqCst),
        commanded,
        "a recheck and a look send the machine nothing"
    );
    the_judgements_are_recorded_against_their_frames(&world, &print_id);
}

/// A recheck at 13:14 finds the toolhead over the part; one at 13:44, over a
/// failure an earlier judgement latched, finds the first layer and corrects
/// the view; each look after says so.
fn the_covered_layer_and_then_the_found_one_are_judged(
    world: &World,
    camera: &Camera,
    machine: &crate::machine::Machine,
    print_id: &str,
) {
    let covered = recheck(world, print_id);
    assert_eq!(covered["alignment"]["verdict"], "pending", "{covered}");
    assert_eq!(covered["view_correction"], Value::Null, "{covered}");
    assert_eq!(covered["image_kind"], "whole_frame", "{covered}");
    assert_eq!(
        covered["image_fallback"]["code"], "alignment_inconclusive",
        "{covered}"
    );
    let kept = std::fs::read(covered["frame_path"].as_str().expect("a frame path"))
        .expect("the fresh frame is stored");
    assert_eq!(kept, frame(COVERED.0), "the fresh frame is the camera's");
    let looked = look(world, print_id);
    assert_eq!(looked["image_kind"], "whole_frame", "{looked}");
    assert_eq!(
        looked["image_fallback"]["code"], "alignment_inconclusive",
        "{looked}"
    );

    // A failure an earlier judgement latched is judged again all the same.
    std::fs::write(
        world
            .root
            .path()
            .join("state")
            .join("alignment")
            .join(format!("{print_id}.json")),
        br#"{"verdict":"failed","reason":{"failure":"stream"}}"#,
    )
    .expect("a latched failure");
    camera.shows(frame(FOUND.0));
    machine.stands_at.store(FOUND.1, Ordering::SeqCst);
    let found = recheck(world, print_id);
    assert_eq!(found["alignment"]["verdict"], "passed", "{found}");
    assert_eq!(found["view_correction"]["model"], "translation", "{found}");
    // The layer passes, but the landmarks show the camera has moved since its
    // calibration: nothing is drawn on its frames, and the answer says why.
    assert_eq!(found["image_kind"], "whole_frame", "{found}");
    assert_eq!(
        found["image_fallback"]["code"], "calibration_stale",
        "{found}"
    );
    let looked = look(world, print_id);
    assert_eq!(
        looked["image_fallback"]["code"], "calibration_stale",
        "{looked}"
    );
}

/// A camera moved again, landmarks hidden and a paused print are each what a
/// look says.
fn a_moved_camera_hidden_landmarks_and_a_pause_are_each_said(
    world: &World,
    camera: &Camera,
    machine: &crate::machine::Machine,
    print_id: &str,
) {
    // A frame of the camera where its calibration had it, which this print's
    // correction was not taken against: the camera moved again.
    camera.shows(
        std::fs::read(root().join(
            "crates/printobserver-panels/tests/fixtures/frames/jar-top-petg-healthy/agent-031-obs-125703-p43.2.jpg",
        ))
        .expect("a frame of the camera where it was calibrated"),
    );
    machine.stands_at.store(LATER.1, Ordering::SeqCst);
    let moved = look(world, print_id);
    assert_eq!(
        moved["image_fallback"]["code"], "view_refit_rejected",
        "{moved}"
    );

    camera.shows(frame(LATER.0));
    let refound = recheck(world, print_id);
    assert_eq!(refound["alignment"]["verdict"], "passed", "{refound}");
    camera.shows(landmarks_hidden(&frame(LATER.0)));
    let hidden = look(world, print_id);
    assert_eq!(
        hidden["image_fallback"]["code"], "landmarks_not_found",
        "{hidden}"
    );

    world.wants(Reports::Paused);
    let paused = look(world, print_id);
    assert_eq!(paused["image_fallback"]["code"], "paused", "{paused}");
    assert_eq!(paused["image_kind"], "whole_frame", "{paused}");
    // A recheck while the printer reports the print paused judges the view and
    // leaves the first layer alone: its bed stands off the plan's height.
    let rechecked_paused = recheck(world, print_id);
    assert_eq!(
        rechecked_paused["image_fallback"]["code"], "paused",
        "{rechecked_paused}"
    );
    assert_eq!(
        rechecked_paused["alignment"]["verdict"], "passed",
        "the verdict the last recheck left stands: {rechecked_paused}"
    );
}

/// Every judgement the journey asked for is in the print's history, naming
/// the frame it read.
fn the_judgements_are_recorded_against_their_frames(world: &World, print_id: &str) {
    let judged = recorded(world, print_id, "first_layer_alignment_judged");
    let verdicts: Vec<&Value> = judged.iter().map(|one| &one["verdict"]).collect();
    assert_eq!(
        verdicts,
        [&json!("inconclusive"), &json!("passed"), &json!("passed")],
        "none for the recheck while paused: {judged:#?}"
    );
    assert_eq!(judged[0]["frame_sha256"], json!(sha256(&frame(COVERED.0))));
    assert_eq!(judged[1]["frame_sha256"], json!(sha256(&frame(FOUND.0))));
    let views = recorded(world, print_id, "camera_view_judged");
    assert!(
        views
            .iter()
            .any(|view| view["outcome"] == "view_refit_rejected"
                && view["view_correction"].is_object()),
        "the moved camera is recorded against the correction it broke: {views:#?}"
    );
    assert!(
        views
            .iter()
            .any(|view| view["outcome"] == "landmarks_not_found"),
        "{views:#?}"
    );
}

/// `printobserver recheck-camera` against a supervisor configured with no
/// camera is refused in the supervisor's own words, naming the setting to add:
/// it exits `refused`, the supervisor answers `503` with no record because
/// nothing was decided, no judgement is recorded, and the machine is sent
/// nothing.
pub fn a_recheck_with_no_camera_configured_is_refused_naming_the_setting() {
    let world = World::configured(TWO_COLOUR_BOX, &committed_skill(), None, |document| {
        document
            .as_object_mut()
            .expect("the configuration is a table")
            .remove("camera");
    });
    let Printer::StoodIn(machine) = &world.printer else {
        panic!("the {TWO_COLOUR_BOX} world is not a stood-in machine");
    };
    world.wants(Reports::Printing);
    let print_id = world.print_id();
    let commanded = machine.commanded.load(Ordering::SeqCst);

    let ran = running::command(
        &world,
        &[
            "recheck-camera",
            "--print-id",
            &print_id,
            "--actor",
            "operator",
            "--reason",
            REASON,
            "--json",
        ],
    );
    assert_eq!(
        ran.code,
        Some(7),
        "a recheck with no camera exits `refused`: {}",
        ran.said()
    );
    let answered: Value = printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("the refusal is a document ({error}): {}", ran.said()));
    assert_eq!(
        answered,
        json!({
            "error": "no camera is configured to judge: set the server's `[camera] snapshot_url`"
        }),
        "the refusal is the supervisor's own, carrying no record"
    );
    assert!(
        ran.err.contains("the supervisor answered 503"),
        "the refusal says the status it came under: {}",
        ran.said()
    );
    assert!(
        recorded(&world, &print_id, "camera_view_judged").is_empty()
            && recorded(&world, &print_id, "first_layer_alignment_judged").is_empty(),
        "nothing was judged"
    );
    assert_eq!(
        machine.commanded.load(Ordering::SeqCst),
        commanded,
        "a refused recheck sends the machine nothing"
    );
}
