//! A print's camera judged on its own checks' frames: a covered first layer
//! retried rather than failed, a clearly found one passing, a paused check
//! left unjudged, a camera moved since its calibration shown the plain frame
//! for it, every judgement recorded against the stored frame it read, a camera
//! that moves again refused until a fresh correction is taken, and a pass at
//! the first layer's own height correcting the drawing of an unmoved camera.
//!
//! The server is the real composition over the scripted `OctoPrint` and a
//! scripted camera, as the composite-check journeys run it, configured with
//! the stale `core-one-2026-09-27.json` the 2026-11-26 two-colour print ran
//! with. The frames are that print's own, as plain copies in
//! `printobserver-panels`' fixtures; its slice is the core's plain copy, with
//! its two `M600` lines made comments of the same length so that no planned
//! change interposes a check of its own and every byte offset stays the
//! print's. Each check is held open while the journey moves the job on and
//! the camera answers the next frame, so every check reads exactly the frame
//! and position the journey means.

use core::time::Duration;
use std::path::PathBuf;

use printobserver_octoprint::standin::JobScript;
use printobserver_server::{PrintAlignment, Running};
use printobserver_types::PrintId;
use printobserver_types::serde_json::{Value, json};

use crate::camera_host::{Scripted, ScriptedHost};
use crate::composite_checks::{
    Camera, FILE, Journey, Started, adopted, block_under, calibration_at, committed,
    expected_composite, frame, index_of, sha256, slice,
};
use crate::lean_checks::handed;
use crate::world::{OPERATOR, presenting};

/// The operation beside the action vocabulary these journeys and `aligning`'s
/// drive: the operator's recheck of a print's camera.
pub const DRIVEN_THERE: [&str; 1] = [printobserver_server::RECHECK_CAMERA];

/// The installer whose template the journeys run.
const INSTALLER: &str = "scripts/install.sh";

/// The recorded print the frames are of.
const CASE: &str = "two-colour-planned-change";

/// The seconds between two checks: long enough that a check released is
/// held again before the next one starts.
const INTERVAL_S: i64 = 2;

/// The covered first layer: the toolhead over the part at 13:14.
const COVERED: (&str, u64) = ("131408-periodic_observation.jpg", 24_738);

/// A check while the print was paused, its bed lowered.
const PAUSED: (&str, u64) = ("131609-periodic_observation.jpg", 27_748);

/// The first layer clearly found at 13:44, at a shift of about +4,-20 px.
const FOUND: (&str, u64) = ("134401-periodic_observation.jpg", 51_973);

/// The later frames, each a different picture of the part.
const LATER: [(&str, u64); 2] = [
    ("152603-filament_change_planned_resumed.jpg", 62_152),
    ("152804-periodic_observation.jpg", 74_248),
];

/// A position past the second colour change with the nozzle over the part's
/// front-left corner, where the toolhead covers it whatever the frame.
const COVERED_LATER: u64 = 62_653;

/// The two landmarks found agreeing on every frame of the print.
const AGREEING: [&str; 2] = ["frame-screws-right-front", "frame-label-right"];

/// The print's slice with its colour changes made comments.
fn box_slice() -> Vec<u8> {
    let bytes = slice("two-colour-box.gcode");
    let text = bytes
        .split(|byte| *byte == b'\n')
        .map(|line| {
            if line == b"M600" {
                b";600".to_vec()
            } else {
                line.to_vec()
            }
        })
        .collect::<Vec<_>>()
        .join(&b'\n');
    assert_eq!(text.len(), bytes.len(), "every offset stays the print's");
    text
}

/// One of the print's frames.
fn box_frame(file: &str) -> Vec<u8> {
    frame(CASE, file)
}

/// A frame decoded, changed and encoded again.
fn changed(bytes: &[u8], change: impl FnOnce(&mut image::RgbImage)) -> Vec<u8> {
    let mut decoded = image::load_from_memory(bytes).expect("a frame").to_rgb8();
    change(&mut decoded);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
        .encode_image(&decoded)
        .expect("a frame encodes");
    out
}

/// The same picture in other bytes: one pixel changed, as two frames of a
/// still scene differ.
fn touched(bytes: &[u8]) -> Vec<u8> {
    changed(bytes, |frame| {
        frame.put_pixel(0, 0, image::Rgb([255, 0, 0]));
    })
}

/// A frame moved `by` pixels right, its uncovered edge grey: the camera
/// knocked a little further out of place.
fn shifted(bytes: &[u8], by: u32) -> Vec<u8> {
    changed(bytes, |frame| {
        let mut moved =
            image::RgbImage::from_pixel(frame.width(), frame.height(), image::Rgb([90, 90, 90]));
        image::imageops::replace(&mut moved, frame, i64::from(by), 0);
        *frame = moved;
    })
}

/// A frame with the agreeing landmarks' regions painted `grey`, each one's
/// patch and the whole search around it, as something standing in front of
/// them would hide them.
fn painted_over(bytes: &[u8], grey: u8) -> Vec<u8> {
    let calibration = committed();
    let centres: Vec<[u32; 2]> = calibration["landmarks"]
        .as_array()
        .expect("landmarks")
        .iter()
        .filter(|landmark| AGREEING.contains(&landmark["name"].as_str().unwrap_or_default()))
        .map(|landmark| {
            let at = |axis: usize| {
                let value = landmark["center_px"][axis].as_f64().expect("a centre");
                format!("{value:.0}")
                    .parse::<u32>()
                    .expect("a pixel of the frame")
            };
            [at(0), at(1)]
        })
        .collect();
    assert_eq!(centres.len(), 2);
    changed(bytes, |frame| {
        let reach = 240;
        for [u, v] in centres {
            for y in v.saturating_sub(reach)..(v + reach).min(frame.height()) {
                for x in u.saturating_sub(reach)..(u + reach).min(frame.width()) {
                    frame.put_pixel(x, y, image::Rgb([grey, grey, grey]));
                }
            }
        }
    })
}

/// The job printing at `position`, or paused there.
fn job_at(journey: &Journey, position: u64, paused: bool) {
    let size = i64::try_from(box_slice().len()).expect("a size");
    let mut job = JobScript::printing(
        FILE,
        size,
        i64::try_from(position).expect("a position"),
        600,
    );
    if paused {
        "Paused".clone_into(&mut job.state);
        journey.octoprint.reports("Paused", job);
    } else {
        journey.octoprint.reports("Printing", job);
    }
}

/// A journey's checks, each held open until the journey lets it go.
struct Checks<'a> {
    journey: &'a Journey,
    hold: PathBuf,
    count: usize,
}

impl<'a> Checks<'a> {
    /// Hold the first check when it comes.
    fn holding(journey: &'a Journey) -> Self {
        let hold = journey.root.path().join("hold");
        std::fs::write(&hold, b"").expect("the hold is writable");
        Self {
            journey,
            hold,
            count: 0,
        }
    }

    /// The next check, its job at `position` — printing, or paused — and
    /// what its harness was handed. The check before it is let go once the
    /// job has moved, and this one is held as it comes.
    async fn next(&mut self, position: u64, paused: bool) -> Value {
        job_at(self.journey, position, paused);
        if self.count > 0 {
            std::fs::remove_file(&self.hold).expect("the hold is removable");
            tokio::time::sleep(Duration::from_millis(400)).await;
            std::fs::write(&self.hold, b"").expect("the hold is writable");
        }
        self.count += 1;
        let seen = self.journey.runs(self.count).await;
        seen[self.count - 1].clone()
    }

    /// The next check, the job already moved where it is meant to be: the
    /// check before it is let go, and this one held as it comes.
    async fn next_moved(&mut self) -> Value {
        if self.count > 0 {
            std::fs::remove_file(&self.hold).expect("the hold is removable");
            tokio::time::sleep(Duration::from_millis(400)).await;
            std::fs::write(&self.hold, b"").expect("the hold is writable");
        }
        self.count += 1;
        let seen = self.journey.runs(self.count).await;
        seen[self.count - 1].clone()
    }

    /// Let the last check go.
    fn release(self) {
        let _ = std::fs::remove_file(&self.hold);
    }
}

/// The situation a check's prompt carried.
fn situation(seen: &Value) -> Value {
    let (prompt, _) = handed(seen);
    block_under(&prompt, "## The situation when this turn began")
}

/// What a whole frame's image line says of a rise judged by eye on it, where
/// the frame is shown for the camera's own sake rather than the print's.
const UNSEEN_RISE_BOUND: &str = "; on this frame a part that is not plainly higher is no grounds to pause on its own until the plan's rise since the frame it is compared against exceeds 5 mm";

/// Whether a check's image line bounds the rise judged by eye on its frame.
fn bounds_the_unseen_rise(situation: &Value) -> bool {
    situation["image"]
        .as_str()
        .is_some_and(|note| note.ends_with(UNSEEN_RISE_BOUND))
}

/// Every event of a print's history, oldest first.
async fn history(server: &Running, print_id: PrintId) -> Vec<Value> {
    let body: Value = presenting(OPERATOR)
        .get(format!(
            "{}/prints/{print_id}/history?limit=1000",
            server.base_url()
        ))
        .send()
        .await
        .expect("the server answers")
        .json()
        .await
        .expect("JSON");
    let mut events = body["events"].as_array().expect("events").clone();
    events.sort_by_key(|event| event["id"].as_str().unwrap_or_default().to_owned());
    events
}

/// The payloads of one kind, oldest first.
fn payloads(events: &[Value], kind: &str) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind)
        .map(|event| event["payload"].clone())
        .collect()
}

/// The bytes a record's `frame_sha256` resolves to through the image store,
/// once it is held to name a frame stored with one of the print's events.
fn resolved(journey: &Journey, events: &[Value], payload: &Value) -> Vec<u8> {
    let sha = payload["frame_sha256"].as_str().expect("a frame named");
    assert!(
        events.iter().any(|event| event["image"]["sha256"] == sha),
        "{sha} is stored with none of the print's events"
    );
    std::fs::read(
        journey
            .root
            .path()
            .join("state")
            .join("images")
            .join(&sha[..2])
            .join(sha),
    )
    .unwrap_or_else(|error| panic!("{sha} resolves to no stored frame: {error}"))
}

/// The server over a journey's state, its camera answering `frames` in turn
/// and calibrated by the stale committed calibration.
async fn started(journey: &Journey, frames: &[Vec<u8>]) -> (Started, ScriptedHost) {
    let host = ScriptedHost::answering(frames.iter().cloned().map(Scripted::frame).collect()).await;
    let camera = Camera {
        host,
        calibration: Some(committed()),
        snapshot: true,
    };
    let started = journey.start(&camera, INTERVAL_S, &json!([])).await;
    (started, camera.host)
}

/// A covered first layer is inconclusive and judged again on the next check's
/// own frame; a check while the printer reports the print paused is not judged
/// and is shown the whole frame for it; the clearly found layer passes, but
/// the landmarks show the camera has moved since its calibration, so every
/// check is shown the plain frame saying so rather than an outline drawn off
/// the part. Every record names
/// the stored frame it read, a change notice is written only when the image
/// changes, and the camera is asked for nothing beyond the checks' own frames.
#[tokio::test(flavor = "multi_thread")]
async fn a_covered_first_layer_is_judged_again_and_a_moved_camera_is_never_drawn_on() {
    let frames = [
        box_frame(COVERED.0),
        box_frame(PAUSED.0),
        box_frame(FOUND.0),
        box_frame(LATER[1].0),
    ];
    let journey = Journey::new(INSTALLER, &box_slice(), COVERED.1);
    let mut checks = Checks::holding(&journey);
    job_at(&journey, COVERED.1, false);
    let (started, camera) = started(&journey, &frames).await;
    let server = &started.server;
    let print_id = adopted(server).await;

    let first = situation(&checks.next(COVERED.1, false).await);
    assert_eq!(first["image_kind"], "whole_frame", "{first}");
    assert_eq!(
        first["image_fallback"]["code"], "alignment_inconclusive",
        "{first}"
    );
    assert!(bounds_the_unseen_rise(&first), "{first}");
    assert_eq!(server.print_alignment(print_id), PrintAlignment::Pending);

    let paused = situation(&checks.next(PAUSED.1, true).await);
    assert_eq!(paused["image_fallback"]["code"], "paused", "{paused}");
    assert!(
        !bounds_the_unseen_rise(&paused),
        "a paused check's whole frame keeps the skill's own rule: {paused}"
    );

    // The layer is clearly found and the alignment passes, but the
    // landmarks show the camera has moved since its calibration: nothing is
    // drawn on its frames, and the check says why.
    let found = situation(&checks.next(FOUND.1, false).await);
    assert_eq!(found["image_kind"], "whole_frame", "{found}");
    assert_eq!(
        found["image_fallback"]["code"], "calibration_stale",
        "{found}"
    );
    assert!(bounds_the_unseen_rise(&found), "{found}");
    let PrintAlignment::Passed {
        offset_px,
        correction_px,
    } = server.print_alignment(print_id)
    else {
        panic!("the clearly found layer did not pass");
    };
    assert!(
        (offset_px - 4.0_f64.hypot(20.0)).abs() <= 3.0,
        "13:44's layer is found about +4,-20 px off, not {offset_px:.1} px"
    );
    assert!(
        correction_px[0].hypot(correction_px[1]) < 1e-9,
        "a pass 6.6 mm above its first layer corrects by nothing, not {correction_px:?}"
    );

    let later = situation(&checks.next(LATER[1].1, false).await);
    assert_eq!(
        later["image_fallback"]["code"], "calibration_stale",
        "{later}"
    );
    // Read while the last check is still held, so no check after it adds to
    // what is counted.
    let events = history(server, print_id).await;
    let fetched = camera.exchanges().len();
    the_records_name_the_frames_they_read(&journey, &events, &frames, correction_px);
    assert_eq!(
        fetched,
        frames.len(),
        "the camera was asked for each check's own frame and nothing else"
    );

    let status: Value = presenting(OPERATOR)
        .get(format!("{}/prints/{print_id}/status", server.base_url()))
        .send()
        .await
        .expect("the server answers")
        .json()
        .await
        .expect("JSON");
    assert_eq!(
        status["last_check_image"]["kind"], "whole_frame",
        "{status}"
    );
    assert_eq!(
        status["last_check_image"]["fallback"]["code"], "calibration_stale",
        "{status}"
    );
    checks.release();
    started.server.stop().await;
}

/// Each record of the first journey's checks, naming the stored frame it read:
/// one alignment judgement for the covered check and one for the pass, one
/// refit, a view judgement for every judged check, and a change notice for
/// each change of image.
fn the_records_name_the_frames_they_read(
    journey: &Journey,
    events: &[Value],
    frames: &[Vec<u8>; 4],
    correction_px: [f64; 2],
) {
    let judged = payloads(events, "first_layer_alignment_judged");
    assert_eq!(
        judged.len(),
        2,
        "one judgement for the covered check, none for the paused one, one for the pass, and none after: {judged:#?}"
    );
    assert_eq!(judged[0]["verdict"], "inconclusive");
    assert_eq!(judged[0]["reason"], "covered");
    assert_eq!(judged[0]["view_correction"], Value::Null);
    assert_eq!(resolved(journey, events, &judged[0]), frames[0]);
    assert_eq!(judged[1]["verdict"], "passed");
    assert_eq!(judged[1]["reason"], "clearly_found");
    assert_eq!(
        judged[1]["correction_px"],
        json!(correction_px),
        "the pass records the correction it put in force"
    );
    assert!(judged[1]["match_strength"].as_f64().expect("a strength") >= 15.0);
    assert_eq!(resolved(journey, events, &judged[1]), frames[2]);

    let refits = payloads(events, "camera_view_refit");
    assert_eq!(refits.len(), 1, "{refits:#?}");
    assert_eq!(resolved(journey, events, &refits[0]), frames[2]);
    assert_eq!(refits[0]["view_correction"]["model"], "translation");

    let views = payloads(events, "camera_view_judged");
    let outcomes: Vec<&Value> = views.iter().map(|view| &view["outcome"]).collect();
    assert_eq!(
        outcomes,
        [
            &json!("view_refit_rejected"),
            &json!("refit"),
            &json!("refit")
        ],
        "one per judged check, none for the paused one: {views:#?}"
    );
    for (view, frame) in views.iter().zip([&frames[0], &frames[2], &frames[3]]) {
        assert_eq!(&resolved(journey, events, view), frame);
    }

    let changes = payloads(events, "check_image_changed");
    let kinds: Vec<(Value, Value)> = changes
        .iter()
        .map(|change| (change["kind"].clone(), change["fallback"]["code"].clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            (json!("whole_frame"), json!("alignment_inconclusive")),
            (json!("whole_frame"), json!("paused")),
            (json!("whole_frame"), json!("calibration_stale")),
        ],
        "a notice for each change and none for the unchanged last check"
    );
    for (change, frame) in changes.iter().zip(frames) {
        assert_eq!(&resolved(journey, events, change), frame);
    }
}

/// Checks drawn through one correction, then checks whose landmarks are
/// painted over, each leave exactly one view record naming the very frame it
/// read — different bytes check to check — with its own outcome and reason,
/// and a change notice is written only at each transition.
#[tokio::test(flavor = "multi_thread")]
async fn every_judgement_is_one_record_naming_its_own_frame() {
    let later = box_frame(LATER[1].0);
    let frames = vec![
        box_frame(FOUND.0),
        box_frame(LATER[0].0),
        later.clone(),
        touched(&later),
        painted_over(&later, 120),
        painted_over(&later, 128),
        painted_over(&later, 136),
    ];
    let positions = [
        FOUND.1, LATER[0].1, LATER[1].1, LATER[1].1, LATER[1].1, LATER[1].1, LATER[1].1,
    ];
    let journey = Journey::new(INSTALLER, &box_slice(), FOUND.1);
    let mut checks = Checks::holding(&journey);
    let (started, _camera) = started(&journey, &frames).await;
    let server = &started.server;
    let print_id = adopted(server).await;
    let mut situations = Vec::new();
    for position in positions {
        situations.push(situation(&checks.next(position, false).await));
    }
    let events = history(server, print_id).await;
    checks.release();

    for (index, situation) in situations.iter().enumerate() {
        let expected = if index < 4 {
            "calibration_stale"
        } else {
            "landmarks_not_found"
        };
        assert_eq!(
            situation["image_fallback"]["code"], expected,
            "check {index}: {situation}"
        );
        assert_eq!(
            bounds_the_unseen_rise(situation),
            index < 4,
            "check {index}: only a stale camera's whole frame bounds the rise: {situation}"
        );
    }
    for situation in &situations[4..] {
        assert_eq!(
            situation["image_fallback"]["code"], "landmarks_not_found",
            "{situation}"
        );
    }

    let views = payloads(&events, "camera_view_judged");
    assert_eq!(
        views.len(),
        frames.len(),
        "one record per check: {views:#?}"
    );
    let mut named = std::collections::BTreeSet::new();
    for (index, (view, frame)) in views.iter().zip(&frames).enumerate() {
        assert_eq!(
            &resolved(&journey, &events, view),
            frame,
            "check {index}'s record names the frame it read"
        );
        assert_eq!(view["frame_sha256"], json!(sha256(frame)));
        named.insert(view["frame_sha256"].as_str().expect("a sha").to_owned());
        if index < 4 {
            assert_eq!(view["outcome"], "refit", "check {index}: {view}");
            assert_eq!(
                view["reason"],
                json!(printobserver_server::CALIBRATION_STALE)
            );
        } else {
            assert_eq!(
                view["outcome"], "landmarks_not_found",
                "check {index}: {view}"
            );
            assert!(
                view["reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
            );
        }
    }
    assert_eq!(
        named.len(),
        frames.len(),
        "each check read a frame of its own"
    );
    assert_eq!(payloads(&events, "camera_view_refit").len(), 1);
    let changes = payloads(&events, "check_image_changed");
    let kinds: Vec<(Value, Value)> = changes
        .iter()
        .map(|change| (change["kind"].clone(), change["fallback"]["code"].clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            (json!("whole_frame"), json!("calibration_stale")),
            (json!("whole_frame"), json!("landmarks_not_found")),
        ],
        "one notice per transition, none for a repeat"
    );
    assert_eq!(resolved(&journey, &events, &changes[1]), frames[4]);
    started.server.stop().await;
}

/// A camera that moves again after the correction was taken is refused, and
/// the alignment goes back to pending; the next frame, covered, is judged
/// again and stays pending; the clearly found one after it installs a fresh
/// correction and landmark reference, recorded as a new refit naming that
/// frame, the checks after it shown the plain frame for the stale calibration
/// as before the camera moved again.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_moved_again_is_refused_until_a_fresh_correction_is_taken() {
    let moved = shifted(&box_frame(LATER[0].0), 8);
    let refound = shifted(&box_frame(LATER[1].0), 8);
    let frames = vec![
        box_frame(FOUND.0),
        moved.clone(),
        touched(&moved),
        refound.clone(),
        touched(&refound),
    ];
    let journey = Journey::new(INSTALLER, &box_slice(), FOUND.1);
    let mut checks = Checks::holding(&journey);
    let (started, _camera) = started(&journey, &frames).await;
    let server = &started.server;
    let print_id = adopted(server).await;

    let first = situation(&checks.next(FOUND.1, false).await);
    assert_eq!(
        first["image_fallback"]["code"], "calibration_stale",
        "{first}"
    );
    let rejected = situation(&checks.next(LATER[0].1, false).await);
    assert_eq!(
        rejected["image_fallback"]["code"], "view_refit_rejected",
        "{rejected}"
    );
    assert_eq!(
        server.print_alignment(print_id),
        PrintAlignment::Pending,
        "the alignment goes back to pending to take a fresh correction"
    );
    let covered = situation(&checks.next(COVERED_LATER, false).await);
    assert_eq!(
        covered["image_fallback"]["code"], "alignment_inconclusive",
        "{covered}"
    );
    let refound_at = situation(&checks.next(LATER[1].1, false).await);
    assert_eq!(
        refound_at["image_fallback"]["code"], "calibration_stale",
        "{refound_at}"
    );
    let after = situation(&checks.next(LATER[1].1, false).await);
    assert_eq!(
        after["image_fallback"]["code"], "calibration_stale",
        "{after}"
    );
    let events = history(server, print_id).await;
    checks.release();

    let views = payloads(&events, "camera_view_judged");
    let outcomes: Vec<&Value> = views.iter().map(|view| &view["outcome"]).collect();
    assert_eq!(
        outcomes,
        [
            &json!("refit"),
            &json!("view_refit_rejected"),
            &json!("view_refit_rejected"),
            &json!("refit"),
            &json!("refit"),
        ],
        "{views:#?}"
    );
    let judged = payloads(&events, "first_layer_alignment_judged");
    let verdicts: Vec<(Value, Value)> = judged
        .iter()
        .map(|judged| (judged["verdict"].clone(), judged["reason"].clone()))
        .collect();
    assert_eq!(
        verdicts,
        [
            (json!("passed"), json!("clearly_found")),
            (json!("inconclusive"), json!("covered")),
            (json!("passed"), json!("clearly_found")),
        ],
        "no judgement on the refused frame itself: {judged:#?}"
    );
    let refits = payloads(&events, "camera_view_refit");
    assert_eq!(refits.len(), 2, "{refits:#?}");
    assert_eq!(resolved(&journey, &events, &refits[1]), frames[3]);
    let reference = |refit: &Value, name: &str| -> f64 {
        refit["view_correction"]["landmarks"]
            .as_array()
            .expect("a reference")
            .iter()
            .find(|landmark| landmark["name"] == name)
            .and_then(|landmark| landmark["offset_px"][0].as_f64())
            .unwrap_or_else(|| panic!("{name} is in the reference: {refit}"))
    };
    for name in AGREEING {
        let moved_by = reference(&refits[1], name) - reference(&refits[0], name);
        assert!(
            (moved_by - 8.0).abs() <= 2.0,
            "the fresh reference has {name} where the moved camera put it: {moved_by:.1} px further"
        );
    }
    started.server.stop().await;
}

/// The recorded jar top whose first layer a frame shows from one layer above
/// it, and a later frame of it.
const JAR: &str = "jar-top-wrong-material-first-layer-detach";

/// Its frame one layer above its first, and the position it was taken at.
const JAR_FIRST_LAYER: (&str, u64) = ("agent-007-obs-182248-p00.3.jpg", 13_018);

/// A frame of it 1.2 mm higher, and its position.
const JAR_LATER: (&str, u64) = ("agent-009-obs-182450-p03.1.jpg", 143_566);

/// A pass on a frame one layer above the first layer puts its found shift in
/// force as the print's correction, and a later check is drawn through it:
/// its image is the three-panel image composed through that translation.
#[tokio::test(flavor = "multi_thread")]
async fn a_pass_at_the_first_layers_own_height_corrects_every_later_drawing() {
    let bytes = slice("jar-top-first-layer-detach.gcode");
    let frames = [frame(JAR, JAR_FIRST_LAYER.0), frame(JAR, JAR_LATER.0)];
    let journey = Journey::new(INSTALLER, &bytes, JAR_FIRST_LAYER.1);
    let mut checks = Checks::holding(&journey);
    let size = i64::try_from(bytes.len()).expect("a size");
    let at = |position: u64| {
        journey.octoprint.reports(
            "Printing",
            JobScript::printing(
                FILE,
                size,
                i64::try_from(position).expect("a position"),
                600,
            ),
        );
    };
    at(JAR_FIRST_LAYER.1);
    let (started, camera) = started(&journey, &frames).await;
    let server = &started.server;
    let print_id = adopted(server).await;

    let first = checks_at(&mut checks, &at, JAR_FIRST_LAYER.1).await;
    assert_eq!(situation(&first)["image_kind"], "three_panel");
    let PrintAlignment::Passed { correction_px, .. } = server.print_alignment(print_id) else {
        panic!("the healthy first layer did not pass");
    };
    assert!(
        correction_px[0].hypot(correction_px[1]) > 1.0,
        "a pass one layer above the first layer corrects by its found shift, not {correction_px:?}"
    );
    let (prompt, _) = handed(&first);
    let event = block_under(&prompt, "## The event");
    let captured_at: printobserver_types::Timestamp =
        printobserver_types::serde_json::from_value(event["payload"]["frame_captured_at"].clone())
            .expect("the check's frame was captured at a time");

    let later = checks_at(&mut checks, &at, JAR_LATER.1).await;
    let (_, image) = handed(&later);
    let spool = tempfile::TempDir::new().expect("a spool directory");
    let index = index_of(&bytes, &spool.path().join("index.gcode"));
    let url = camera.url();
    assert_eq!(
        image,
        expected_composite(
            &index,
            JAR_LATER.1,
            &frames[1],
            Some((&frames[0], captured_at)),
            &url,
            &calibration_at(&committed(), &url),
            Some(correction_px),
        ),
        "the later check is drawn through the correction the pass put in force"
    );
    checks.release();
    started.server.stop().await;
}

/// The next check, its job at `position`, through a mover of the journey's own.
async fn checks_at(checks: &mut Checks<'_>, at: &impl Fn(u64), position: u64) -> Value {
    at(position);
    checks.next_moved().await
}
