//! Which image a check of a print is shown: the three-panel check image once
//! it can be trusted, and the whole frame with the reason otherwise.
//!
//! The server here is the real composition, configured from each committed
//! install template with the committed camera calibration under `[camera]`:
//! the `OctoPrint` adapter against the scripted `OctoPrint` serving a recorded
//! case's own slice (a plain copy from `printobserver-core`'s fixtures), a
//! scripted camera answering that case's recorded frames (plain copies from
//! `printobserver-panels`' fixtures) in the order they are asked for, and the
//! real supervisor port and `OneHarness` engine with only the paid provider
//! process replaced by this crate's responder over `OneHarness`'s published
//! mock harness. What a check was shown is read where the harness process
//! received it: the image attached to its standard input, and the situation's
//! `image` line in its prompt.
//!
//! A print's first-layer alignment is decided by the printer watch on its own
//! clock, so each journey that needs it decided runs the server once to decide
//! it, with no scheduled check due, and starts it again over the same state —
//! where the verdict stands while the print is open — before its checks run.

use core::fmt::Write as _;
use core::time::Duration;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_core::store::Stores;
use printobserver_core::{GcodeIndex, IndexBuilder};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint};
use printobserver_panels::{BedPose, CameraCalibration, CheckImageInputs, compose_check_image};
use printobserver_printer_api::GcodeSink as _;
use printobserver_server::{PrintAlignment, Running, Server, ServerConfig};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{PrintId, Timestamp};
use sha2::Digest as _;
use tempfile::TempDir;

use crate::camera_host::{Scripted, ScriptedHost};
use crate::installed_detector::INSTALLERS;
use crate::lean_checks::{
    Requests, answering, configured, copy_tree, handed, is_the_whole_frame_scaled, marked_frame,
    marked_frame_of, scripted_agent,
};
use crate::world::{OPERATOR, SECRET, committed_skill, presenting};

/// The file the scripted instance runs.
const FILE: &str = "jar-top.gcode";

/// How long a journey waits for something the server does on its own clock.
const PATIENCE: Duration = Duration::from_secs(90);

/// The recorded case the frames are of.
const PETG: &str = "jar-top-petg-healthy";

/// Its healthy first layer, which the alignment is decided on.
const FIRST_LAYER: &str = "agent-007-obs-121230-p01.3.jpg";

/// Three later frames of it, each a different picture.
const FRAMES: [&str; 3] = [
    "agent-018-obs-123246-p24.5.jpg",
    "agent-027-obs-124858-p38.9.jpg",
    "agent-028-obs-125100-p39.7.jpg",
];

/// The repository's root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A recorded case's frame, from the panels crate's plain fixtures.
fn frame(case: &str, file: &str) -> Vec<u8> {
    std::fs::read(
        root()
            .join("crates/printobserver-panels/tests/fixtures/frames")
            .join(case)
            .join(file),
    )
    .expect("the recorded frame is there")
}

/// A recorded case's slice, from the core's plain fixtures.
fn slice(name: &str) -> Vec<u8> {
    std::fs::read(
        root()
            .join("crates/printobserver-core/tests/fixtures/real-prints")
            .join(name),
    )
    .expect("the recorded slice is there")
}

/// The committed calibration's JSON.
fn committed() -> Value {
    printobserver_types::serde_json::from_slice(
        &std::fs::read(root().join("tests/real-prints/cameras/core-one-2026-09-27.json"))
            .expect("the committed calibration"),
    )
    .expect("JSON")
}

/// A slice's index, built by the real builder spooling it under `spool`, as
/// the server spools a print's file.
fn index_of(bytes: &[u8], spool: &Path) -> GcodeIndex {
    let mut builder = IndexBuilder::spooling(spool.to_path_buf()).expect("the spool is writable");
    let _ = builder.take(bytes);
    builder.finish()
}

/// A file position inside the first layer at or above `z`.
fn position_at(index: &GcodeIndex, bytes: usize, z: f64) -> u64 {
    let size = u64::try_from(bytes).expect("a size");
    (0..size)
        .step_by(500)
        .find(|position| {
            index
                .layer_at(*position)
                .and_then(|layer| index.layer_z_mm(layer))
                .is_some_and(|held| held >= z - 1e-6)
        })
        .expect("a layer at that height")
        + 100
}

/// A file position at or past `from` whose layer lays the part itself, or
/// whose layer lays its supports alone, as `part` asks.
fn position_laying(index: &GcodeIndex, bytes: usize, from: u64, part: bool) -> u64 {
    let size = u64::try_from(bytes).expect("a size");
    (from..size)
        .step_by(997)
        .find(|position| {
            index.object_at(*position).is_some()
                && index.layer_at(*position).is_some_and(|layer| {
                    index
                        .layer_toolpath(layer)
                        .is_ok_and(|path| path.carries_object_extrusion() == part)
                })
        })
        .expect("such a layer")
}

/// The committed calibration with its bed moved along the camera's X by 25
/// mm, which puts the printed first layer far past the alignment threshold.
fn moved_calibration(calibration: &Value) -> Value {
    let base = CameraCalibration::from_json(
        &printobserver_types::serde_json::to_vec(calibration).expect("JSON"),
    )
    .expect("the committed calibration is valid");
    let mut translation = base.bed_pose.translation_mm;
    translation[0] += 25.0;
    let moved = base.with_bed_pose(BedPose {
        rotation_rodrigues: base.bed_pose.rotation_rodrigues,
        translation_mm: translation,
    });
    let mut changed = calibration.clone();
    changed["bed_pose"]["translation_mm"] = json!(moved.bed_pose.translation_mm);
    changed
}

/// A frame moved `by` pixels right, its uncovered edge black: what a camera
/// knocked out of place shows, inside every landmark's search radius.
fn shifted(bytes: &[u8], by: u32) -> Vec<u8> {
    let source = image::load_from_memory(bytes).expect("a frame").to_rgb8();
    let moved = image::RgbImage::from_fn(source.width(), source.height(), |x, y| {
        if x < by {
            image::Rgb([0, 0, 0])
        } else {
            *source.get_pixel(x - by, y)
        }
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode_image(&moved)
        .expect("the frame encodes");
    jpeg
}

/// A frame's SHA-256, lowercase hexadecimal, as the store names one.
fn sha256(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The three-panel check image of `current` at `position`, composed here from
/// the index's own answers: the active object at that position, the layer
/// there read from the spool, the nearest object layer below when that layer
/// lays supports alone, and `previous`.
fn expected_composite(
    index: &GcodeIndex,
    position: u64,
    current: &[u8],
    previous: Option<(&[u8], Timestamp)>,
    url: &str,
    calibration: &CameraCalibration,
) -> Vec<u8> {
    let active_object = index.object_at(position).expect("an object is laid there");
    let layer = index.layer_at(position).expect("a layer there");
    let toolpath = index.layer_toolpath(layer).expect("the spool holds it");
    let below = (!toolpath.carries_object_extrusion()).then(|| {
        index
            .layer_toolpath(
                index
                    .object_layer_at_or_below(layer)
                    .expect("a part layer below"),
            )
            .expect("the spool holds it")
    });
    compose_check_image(&CheckImageInputs {
        current_jpeg: current,
        // Panel 1's label names no time, so the current frame's is not drawn.
        current_at: Timestamp::now(),
        camera_snapshot_url: url,
        previous,
        outlines: index.object_outlines(),
        active_object,
        toolpath: &toolpath,
        object_layer_below: below.as_ref(),
        print_z_mm: index.layer_z_mm(layer).expect("a height"),
        calibration,
    })
    .expect("the composite composes")
    .jpeg
}

/// One section of a prompt: the JSON block under its heading.
fn block_under(prompt: &str, heading: &str) -> Value {
    let (_, after) = prompt
        .split_once(heading)
        .unwrap_or_else(|| panic!("the prompt carries no {heading}:\n{prompt}"));
    let (_, block) = after.split_once("```json\n").expect("a block");
    let (text, _) = block.split_once("\n```").expect("the block closes");
    printobserver_types::serde_json::from_str(text).expect("the block is JSON")
}

/// What one check was handed: its event, its situation's image line and
/// frame capture time, and its attached image's bytes.
struct Handed {
    event: Value,
    note: String,
    frame_captured_at: Option<Timestamp>,
    image: Vec<u8>,
}

/// Read what one run's harness process was handed.
fn read_handed(seen: &Value) -> Handed {
    let (prompt, image) = handed(seen);
    let situation = block_under(&prompt, "## The situation when this turn began");
    Handed {
        event: block_under(&prompt, "## The event"),
        note: situation["image"]
            .as_str()
            .unwrap_or_else(|| panic!("the situation says nothing of its image:\n{prompt}"))
            .to_owned(),
        frame_captured_at: printobserver_types::serde_json::from_value(
            situation["frame_captured_at"].clone(),
        )
        .ok(),
        image,
    }
}

/// Where the camera is and what it is calibrated by, for one start.
struct Camera {
    host: ScriptedHost,
    calibration: Option<Value>,
    /// Whether `[camera] snapshot_url` names it; an alert's frame is fetched
    /// from it either way.
    snapshot: bool,
}

/// One server over a journey's state, and what watches it.
struct Started {
    server: Running,
    requests: Arc<Requests>,
}

/// One journey: its state, its scripted instance, and its slice.
struct Journey {
    root: TempDir,
    octoprint: ScriptedOctoPrint,
    size: i64,
    installer: &'static str,
}

impl Journey {
    /// A journey over `installer`'s template, the scripted instance running
    /// `bytes` at `position`.
    fn new(installer: &'static str, bytes: &[u8], position: u64) -> Self {
        let root = TempDir::new().expect("a journey's own root");
        copy_tree(
            committed_skill().parent().expect("the skill's directory"),
            &root
                .path()
                .join("state")
                .join("skills")
                .join("printobserver"),
        );
        let octoprint = ScriptedOctoPrint::start();
        octoprint.holds_file(FILE, bytes.to_vec(), 1_000.0);
        let size = i64::try_from(bytes.len()).expect("a file's size");
        let journey = Self {
            root,
            octoprint,
            size,
            installer,
        };
        journey.at(position);
        journey
    }

    /// The job reports `position`.
    fn at(&self, position: u64) {
        self.octoprint.reports(
            "Printing",
            JobScript::printing(
                FILE,
                self.size,
                i64::try_from(position).expect("a position"),
                600,
            ),
        );
    }

    /// Start the server, checking every `interval_s` seconds, its camera
    /// `camera` and its agent issuing `actions` in each turn.
    async fn start(&self, camera: &Camera, interval_s: i64, actions: &Value) -> Started {
        let path = configured(
            self.installer,
            &self.root,
            &self.octoprint,
            &camera.host.url(),
            interval_s,
        );
        if !camera.snapshot {
            let mut document: toml::Value =
                toml::from_str(&std::fs::read_to_string(&path).expect("the configuration"))
                    .expect("a document");
            document["camera"]
                .as_table_mut()
                .expect("a camera table")
                .remove("snapshot_url");
            std::fs::write(&path, toml::to_string(&document).expect("it renders"))
                .expect("the configuration is writable");
        }
        if let Some(calibration) = &camera.calibration {
            let mut written = calibration.clone();
            written["camera_snapshot_url"] = Value::String(camera.host.url());
            let file = self.root.path().join("calibration.json");
            std::fs::write(
                &file,
                printobserver_types::serde_json::to_vec(&written).expect("JSON"),
            )
            .expect("the calibration is writable");
            let mut document: toml::Value =
                toml::from_str(&std::fs::read_to_string(&path).expect("the configuration"))
                    .expect("a document");
            crate::world::set(
                &mut document,
                "camera.calibration",
                toml::Value::String(file.display().to_string()),
            );
            std::fs::write(&path, toml::to_string(&document).expect("it renders"))
                .expect("the configuration is writable");
        }
        let mut config = ServerConfig::load(&path)
            .unwrap_or_else(|error| panic!("{}'s configuration reads: {error}", self.installer));
        // The watch's poll is a minimum of five seconds in a file; the
        // alignment it decides is the same at any poll.
        config.printer_poll = Duration::from_millis(200);
        let (agent, requests) = scripted_agent(
            &config,
            self.root.path(),
            &[answering("continue", "the part is where its plan is")],
            actions,
        );
        let server = Server::start_with(
            config.clone(),
            printobserver_server::Ports {
                printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone())),
                stores: Stores::of(Arc::new(
                    SqliteStore::open(&config.state_dir).expect("the store opens"),
                )),
                vision: Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("built")),
                agent: Arc::new(agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{}'s server starts: {error}", self.installer));
        Started { server, requests }
    }

    /// What each run's harness process was handed, in order.
    fn seen(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("seen.json.lines"))
            .unwrap_or_default()
            .lines()
            .map(|line| printobserver_types::serde_json::from_str(line).expect("a document"))
            .collect()
    }

    /// Wait until `count` runs have been handed their prompt.
    async fn runs(&self, count: usize) -> Vec<Value> {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let seen = self.seen();
            if seen.len() >= count {
                return seen;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{}: {} of {count} checks ran",
                self.installer,
                seen.len()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// The print the scripted job is, once the server has adopted it.
async fn adopted(server: &Running) -> PrintId {
    let client = presenting(OPERATOR);
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let body: Value = client
            .get(format!("{}/prints", server.base_url()))
            .send()
            .await
            .expect("the server answers")
            .json()
            .await
            .expect("JSON");
        if let Some(active) = body["active"].as_str() {
            return active.parse().expect("a print id");
        }
        assert!(tokio::time::Instant::now() < deadline, "no job was adopted");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Wait until a print's alignment is decided, and answer it.
async fn decided(server: &Running, print_id: PrintId) -> PrintAlignment {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let verdict = server.print_alignment(print_id);
        if verdict != PrintAlignment::Pending {
            return verdict;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no alignment was ever decided"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Post one failure alert about the running job, its frame the camera's, as
/// Obico does.
async fn alert(server: &Running, camera_url: &str) {
    let mut alert = crate::world::failure_alert(4211, camera_url);
    alert["print"]["filename"] = json!(FILE);
    let status = reqwest::Client::new()
        .post(format!("{}?token={SECRET}", server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(alert.to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
}

/// When the frame of a check's or a look's event was captured, as its payload
/// says.
fn captured_at(event: &Value) -> Timestamp {
    printobserver_types::serde_json::from_value(event["payload"]["frame_captured_at"].clone())
        .expect("the event says when its frame was captured")
}

/// Decide a print's alignment over `first_layer` at `position` with no check
/// due, then answer the journey's camera calibration and the print.
async fn aligned(
    journey: &Journey,
    calibration: &Value,
    first_layer: Vec<u8>,
    position: u64,
) -> (PrintId, PrintAlignment) {
    journey.at(position);
    let deciding = Camera {
        host: ScriptedHost::answering(vec![Scripted::frame(first_layer)]).await,
        calibration: Some(calibration.clone()),
        snapshot: true,
    };
    let started = journey.start(&deciding, 3600, &json!([])).await;
    let print_id = adopted(&started.server).await;
    let verdict = decided(&started.server, print_id).await;
    started.server.stop().await;
    (print_id, verdict)
}

/// The committed calibration, its stream the camera at `url`, as the server
/// holds it.
fn calibration_at(calibration: &Value, url: &str) -> CameraCalibration {
    let mut calibrated = calibration.clone();
    calibrated["camera_snapshot_url"] = Value::String(url.to_owned());
    CameraCalibration::from_json(
        &printobserver_types::serde_json::to_vec(&calibrated).expect("JSON"),
    )
    .expect("the committed calibration is valid")
}

/// Where a journey's print stands in its slice.
struct Plan<'a> {
    index: &'a GcodeIndex,
    position: u64,
}

/// The print's first scheduled check, held open until it is read so no further
/// check moves the print's previous frame along, and a look the operator takes
/// while it is: answers when the check's frame was captured.
///
/// The look is the operator's because a turn handed a check's image is allowed
/// none: it decides on the image in hand.
async fn scheduled_check_and_look(
    journey: &Journey,
    plan: &Plan<'_>,
    calibration: &Value,
    frames: &[Vec<u8>; 3],
    print_id: PrintId,
) -> Timestamp {
    let installer = journey.installer;
    journey.at(plan.position);
    let hold = journey.root.path().join("hold");
    std::fs::write(&hold, b"").expect("the hold is writable");
    let checking = Camera {
        host: ScriptedHost::answering(vec![
            Scripted::frame(frames[0].clone()),
            Scripted::frame(frames[1].clone()),
        ])
        .await,
        calibration: Some(calibration.clone()),
        snapshot: true,
    };
    let url = checking.host.url();
    let calibrated = calibration_at(calibration, &url);
    let started = journey.start(&checking, 1, &json!([])).await;
    assert_eq!(adopted(&started.server).await, print_id, "{installer}");
    let seen = journey.runs(1).await;
    let scheduled = read_handed(&seen[0]);
    assert_eq!(
        scheduled.event["kind"], "periodic_observation",
        "{installer}"
    );
    assert_eq!(
        scheduled.event["image"]["sha256"],
        json!(sha256(&frames[0])),
        "{installer}: the scheduled check's frame is the one taken before its turn"
    );
    let decoded = image::load_from_memory(&scheduled.image)
        .expect("an image")
        .to_rgb8();
    assert_eq!(decoded.dimensions(), (1024, 1848), "{installer}");
    assert_eq!(
        scheduled.image,
        expected_composite(
            plan.index,
            plan.position,
            &frames[0],
            None,
            &url,
            &calibrated
        ),
        "{installer}: the scheduled check is its own frame composed at the job's position, panel 3 the first check's placeholder"
    );
    let scheduled_at = captured_at(&scheduled.event);
    assert_eq!(
        scheduled.note,
        format!(
            "the three-panel check image, 1024x1848: panel 1 this check's own frame, captured \
             {scheduled_at}, with this layer's planned toolpath drawn on it, panel 2 a close-up \
             of the part being printed, panel 3 empty, no earlier check's frame being at hand; \
             the camera does not shift in regular operation and the drawing already accounts \
             for the bed's travel, so a part off its drawn walls in panel 1 has moved unless \
             there is strong evidence otherwise, and no later frame undoes that"
        ),
        "{installer}: the check's note names its own frame and when it was captured"
    );

    let look = &operator_look(&started.server, print_id).await;
    assert_eq!(
        look["event"]["image"]["sha256"],
        json!(sha256(&frames[1])),
        "{installer}"
    );
    let looked = std::fs::read(look["image_path"].as_str().expect("a path"))
        .expect("the look's image is readable");
    assert_eq!(
        looked,
        expected_composite(
            plan.index,
            plan.position,
            &frames[1],
            Some((&frames[0], scheduled_at)),
            &url,
            &calibrated,
        ),
        "{installer}: the look is its own frame composed, the last check's frame as panel 3"
    );
    assert_eq!(
        look["image_note"],
        json!(
            "the three-panel check image, 1024x1848: panel 1 this layer's planned toolpath \
             drawn on the frame, panel 2 a close-up of the part being printed, panel 3 the \
             previous check's frame"
        ),
        "{installer}: {look}"
    );
    started.server.stop().await;
    std::fs::remove_file(&hold).expect("the hold is removable");
    scheduled_at
}

/// The look the operator takes at one print, as the server answers it.
async fn operator_look(server: &Running, print_id: PrintId) -> Value {
    presenting(OPERATOR)
        .get(format!("{}/prints/{print_id}/look", server.base_url()))
        .send()
        .await
        .expect("the server answers")
        .json()
        .await
        .expect("JSON")
}

/// An alert's check after a restart, its frame the third: composed with the
/// first check's frame, captured at `scheduled_at`, as the previous.
async fn alert_check_after_a_restart(
    journey: &Journey,
    plan: &Plan<'_>,
    calibration: &Value,
    frames: &[Vec<u8>; 3],
    (print_id, scheduled_at): (PrintId, Timestamp),
) {
    let installer = journey.installer;
    journey.at(plan.position);
    let alerting = Camera {
        host: ScriptedHost::answering(vec![Scripted::frame(frames[2].clone())]).await,
        calibration: Some(calibration.clone()),
        snapshot: true,
    };
    let url = alerting.host.url();
    let started = journey.start(&alerting, 3600, &json!([])).await;
    assert_eq!(adopted(&started.server).await, print_id, "{installer}");
    alert(&started.server, &url).await;
    let seen = journey.runs(2).await;
    let alerted = read_handed(&seen[1]);
    assert_eq!(alerted.event["kind"], "obico_failure_alert", "{installer}");
    assert_eq!(
        alerted.event["image"]["sha256"],
        json!(sha256(&frames[2])),
        "{installer}: the alert's check is shown the alert's own frame"
    );
    assert_eq!(
        alerted.image,
        expected_composite(
            plan.index,
            plan.position,
            &frames[2],
            Some((&frames[0], scheduled_at)),
            &url,
            &calibration_at(calibration, &url),
        ),
        "{installer}: the alert's check draws the part layer below its support-only layer, the scheduled check's frame as panel 3"
    );
    let alerted_at = alerted
        .frame_captured_at
        .expect("the alert's situation says when its frame was captured");
    assert_eq!(
        alerted.note,
        format!(
            "the three-panel check image, 1024x1848: panel 1 this check's own frame, captured \
             {alerted_at}, with this layer's planned toolpath drawn on it, panel 2 a close-up of \
             the part being printed, panel 3 the previous check's frame, captured \
             {scheduled_at}; the camera does not shift in regular operation and the drawing \
             already accounts for the bed's travel, so a part off its drawn walls in panel 1, or \
             moved since panel 3, has moved unless there is strong evidence otherwise, and no \
             later frame undoes that; panel 1 is the printer's camera's own frame, taken as the \
             detector's alert arrived, not the detector's picture"
        ),
        "{installer}: the alert's check names its own frame and the previous one, with when each was captured"
    );
    assert_eq!(
        started.requests.all().len(),
        1,
        "{installer}: the restarted port built the alert's run alone"
    );
    started.server.stop().await;
}

/// A print whose alignment passed is shown the three-panel image at every
/// check: its first scheduled check composed of its own frame at the job's
/// position with an empty panel 3; the operator's look during it composed of
/// the look's frame with that check's frame as the previous; and an alert's
/// check, on a layer laying supports alone, composed of the alert's frame with
/// the part layer below in grey and that scheduled check's frame as the
/// previous — the record of which stands through a restart.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_whose_alignment_passed_is_shown_the_three_panel_image() {
    let bytes = slice("jar-top-petg.gcode");
    let calibration = committed();
    let frames = FRAMES.map(|file| frame(PETG, file));
    for installer in INSTALLERS {
        let spool = TempDir::new().expect("a spool directory");
        let index = index_of(&bytes, &spool.path().join("index.gcode"));
        let past_first = position_at(&index, bytes.len(), 0.6);
        let walls = Plan {
            index: &index,
            position: position_laying(
                &index,
                bytes.len(),
                position_at(&index, bytes.len(), 5.0),
                true,
            ),
        };
        let supports = Plan {
            index: &index,
            position: position_laying(&index, bytes.len(), past_first, false),
        };
        let journey = Journey::new(installer, &bytes, past_first);
        let (print_id, verdict) =
            aligned(&journey, &calibration, frame(PETG, FIRST_LAYER), past_first).await;
        assert!(
            matches!(verdict, PrintAlignment::Passed { .. }),
            "{installer}: the healthy first layer did not pass: {verdict:?}"
        );
        let scheduled_at =
            scheduled_check_and_look(&journey, &walls, &calibration, &frames, print_id).await;
        alert_check_after_a_restart(
            &journey,
            &supports,
            &calibration,
            &frames,
            (print_id, scheduled_at),
        )
        .await;
    }
}

/// The detector's picture of an alert: 1280x720 of the camera's 1920x1080
/// stream, as Obico's is, which no calibration of the stream composes.
const DETECTOR_PICTURE: (u32, u32) = (1280, 720);

/// One alert about the print, its picture the detector's own at `picture_url`,
/// with the camera answering `camera`; answers what its check was handed.
async fn alert_with_detector_picture(
    journey: &Journey,
    calibration: &Value,
    camera: Scripted,
    picture_url: &str,
    (print_id, runs): (PrintId, usize),
) -> (Handed, String) {
    let installer = journey.installer;
    let alerting = Camera {
        host: ScriptedHost::answering(vec![camera]).await,
        calibration: Some(calibration.clone()),
        snapshot: true,
    };
    let url = alerting.host.url();
    let started = journey.start(&alerting, 3600, &json!([])).await;
    assert_eq!(adopted(&started.server).await, print_id, "{installer}");
    alert(&started.server, picture_url).await;
    let seen = journey.runs(runs).await;
    let alerted = read_handed(&seen[runs - 1]);
    started.server.stop().await;
    assert_eq!(alerted.event["kind"], "obico_failure_alert", "{installer}");
    (alerted, url)
}

/// A detector's alert on a print whose alignment passed is shown the
/// three-panel image of the server's own camera frame, taken as the alert
/// arrived, whatever the detector's picture is: Obico's is 1280x720 of a
/// 1920x1080 stream, which no calibration of that stream composes. The event
/// keeps the detector's picture, and the image line says panel 1 is the
/// camera's frame and when it was captured. Where the camera gives no frame,
/// the check is shown the detector's picture whole, saying why.
#[tokio::test(flavor = "multi_thread")]
async fn an_alert_is_shown_the_cameras_own_frame_composed_whatever_the_detectors_picture() {
    let bytes = slice("jar-top-petg.gcode");
    let calibration = committed();
    let camera_frame = frame(PETG, FRAMES[1]);
    let picture = marked_frame_of(DETECTOR_PICTURE);
    for installer in INSTALLERS {
        let spool = TempDir::new().expect("a spool directory");
        let index = index_of(&bytes, &spool.path().join("index.gcode"));
        let past_first = position_at(&index, bytes.len(), 0.6);
        let walls = laying_walls(&index, bytes.len());
        let journey = Journey::new(installer, &bytes, past_first);
        let (print_id, verdict) =
            aligned(&journey, &calibration, frame(PETG, FIRST_LAYER), past_first).await;
        assert!(
            passed(&verdict),
            "{installer}: the healthy first layer did not pass: {verdict:?}"
        );
        journey.at(walls);
        let detector = ScriptedHost::answering(vec![Scripted::frame(picture.clone())]).await;

        let (alerted, url) = alert_with_detector_picture(
            &journey,
            &calibration,
            Scripted::frame(camera_frame.clone()),
            &detector.url(),
            (print_id, 1),
        )
        .await;
        assert_eq!(
            alerted.event["image"]["sha256"],
            json!(sha256(&picture)),
            "{installer}: the alert's event keeps the detector's own picture"
        );
        assert_eq!(
            alerted.image,
            expected_composite(
                &index,
                walls,
                &camera_frame,
                None,
                &url,
                &calibration_at(&calibration, &url),
            ),
            "{installer}: the alert's check is the three-panel image of the camera's own frame"
        );
        let captured = alerted
            .frame_captured_at
            .expect("the alert's situation says when its frame was captured");
        assert_eq!(
            alerted.note,
            format!(
                "the three-panel check image, 1024x1848: panel 1 this check's own frame, captured \
                 {captured}, with this layer's planned toolpath drawn on it, panel 2 a close-up \
                 of the part being printed, panel 3 empty, no earlier check's frame being at \
                 hand; the camera does not shift in regular operation and the drawing already \
                 accounts for the bed's travel, so a part off its drawn walls in panel 1 has \
                 moved unless there is strong evidence otherwise, and no later frame undoes \
                 that; panel 1 is the printer's camera's own frame, taken as the detector's \
                 alert arrived, not the detector's picture"
            ),
            "{installer}: the alert's image line names the camera's frame and when it was captured"
        );

        let (fallen_back, _) = alert_with_detector_picture(
            &journey,
            &calibration,
            Scripted::refusing("404 Not Found", "no such stream"),
            &detector.url(),
            (print_id, 2),
        )
        .await;
        is_the_whole_frame_scaled(
            &fallen_back.image,
            &format!("{installer}: an alert the camera gave no frame for"),
        );
        assert!(
            fallen_back.note.starts_with(
                "the whole camera frame, scaled to fit 1280x720 and uncropped, because "
            ) && fallen_back.note.contains("resolution")
                && fallen_back.note.contains(
                    "; the detector's own picture, because the camera gave no frame of its own \
                     as the alert arrived: "
                ),
            "{installer}: the alert's check is shown the detector's picture whole, saying why: {}",
            fallen_back.note
        );
    }
}

/// One reason a check is shown the whole frame, and how a journey brings it
/// about.
struct WholeFrameCase {
    /// What the case is.
    name: &'static str,
    /// The slice the job runs.
    slice: &'static str,
    /// The calibration configured, when one is.
    calibration: Option<fn(&Value) -> Value>,
    /// What the alignment is decided over before any check — a recorded
    /// case's first-layer frame, at a position of the slice — and the verdict
    /// it must reach; none when no alignment is decided first.
    aligned_over: Option<Aligned>,
    /// Where the job stands when the check runs, from the slice's index.
    position: fn(&GcodeIndex, usize) -> u64,
    /// The frame the camera answers the check with.
    frame: fn() -> Vec<u8>,
    /// What the image line must say beside the whole frame.
    says: &'static str,
    /// The size the frame is shown at, and whether it is the marked frame,
    /// which a check shown it whole keeps a mark in every corner of.
    shown: ((u32, u32), bool),
    /// What the image line begins with.
    shown_as: &'static str,
    /// What is done to the state directory, once the print is known, before
    /// the check runs.
    before_check: Option<fn(&Path, PrintId)>,
    /// Whether the check is an alert's rather than a scheduled one.
    by_alert: bool,
    /// Whether `[camera] snapshot_url` is configured.
    snapshot: bool,
}

/// The whole frame's line, and every other field a case leaves as it is.
const WHOLE: WholeFrameCase = WholeFrameCase {
    name: "",
    slice: "jar-top-petg.gcode",
    calibration: Some(as_committed),
    aligned_over: None,
    position: laying_walls,
    frame: marked_frame,
    says: "",
    shown: ((1280, 720), true),
    shown_as: "the whole camera frame, scaled to fit 1280x720",
    before_check: None,
    by_alert: false,
    snapshot: true,
};

/// A slice labelling one object and no layer.
const NO_LAYERS: &[u8] = b"G90\nM83\nM486 S0\nG1 X10 Y10\nG1 X60 Y10 E2\nG1 X60 Y60 E2\nM486 S-1\n";

/// Stand a directory where the print's spool is written, so the restarted
/// server's index of it has no spool to read a layer's toolpath from.
fn spool_unwritable(state: &Path, print_id: PrintId) {
    let spool = state.join("gcode-spool").join(format!("{print_id}.gcode"));
    let _ = std::fs::remove_file(&spool);
    std::fs::create_dir_all(&spool).expect("a directory where the spool goes");
}

/// Stand a file where check images are written, so none can be.
fn check_images_unwritable(state: &Path, _: PrintId) {
    std::fs::write(
        state.join(printobserver_server::CHECK_IMAGES_DIRECTORY),
        b"",
    )
    .expect("a file where check images go");
}

/// What a case's alignment is decided over, and the verdict it must reach.
struct Aligned {
    case: &'static str,
    frame: &'static str,
    at: fn(&GcodeIndex, usize) -> u64,
    reaches: fn(&PrintAlignment) -> bool,
}

/// Whether a verdict passed.
const fn passed(verdict: &PrintAlignment) -> bool {
    matches!(verdict, PrintAlignment::Passed { .. })
}

/// Whether a verdict failed.
const fn failed(verdict: &PrintAlignment) -> bool {
    matches!(verdict, PrintAlignment::Failed { .. })
}

/// Whether a verdict is unavailable.
const fn unavailable(verdict: &PrintAlignment) -> bool {
    matches!(verdict, PrintAlignment::Unavailable)
}

/// The healthy first layer of the PETG jar top, just past it.
const PETG_FIRST_LAYER: fn(fn(&PrintAlignment) -> bool) -> Aligned = |reaches| Aligned {
    case: PETG,
    frame: FIRST_LAYER,
    at: past_first_layer,
    reaches,
};

/// The committed calibration, as it is.
fn as_committed(calibration: &Value) -> Value {
    calibration.clone()
}

/// The position just past the slice's first layer.
fn past_first_layer(index: &GcodeIndex, bytes: usize) -> u64 {
    position_at(index, bytes, 0.6)
}

/// A position inside the slice's first object layer, before it is down.
fn before_first_layer_is_down(index: &GcodeIndex, _: usize) -> u64 {
    index.first_object_layer().expect("a first layer").1 - 50
}

/// A position laying the part, above the first layer.
fn laying_walls(index: &GcodeIndex, bytes: usize) -> u64 {
    position_laying(index, bytes, position_at(index, bytes, 5.0), true)
}

/// The position just before the end of a file.
fn at_its_end(_: &GcodeIndex, bytes: usize) -> u64 {
    u64::try_from(bytes).expect("a size") - 2
}

/// A slice labelling no extrusion type, so no object of it is ever laid.
const UNLABELLED: &[u8] = b";LAYER_CHANGE\n;Z:0.2\nG90\nM83\nG1 X10 Y10\nG1 X60 Y10 E2\nG1 X60 Y60 E2\n;LAYER_CHANGE\n;Z:0.4\nG1 X10 Y60 E2\n";

/// Every case of a check of a print shown the whole frame.
fn whole_frame_cases() -> Vec<WholeFrameCase> {
    let mut cases = plan_and_storage_cases();
    cases.extend(configuration_and_frame_cases());
    cases
}

/// Every case of a check of a print shown the whole frame for where it stands
/// in its plan, or for what the state directory will not hold.
fn plan_and_storage_cases() -> Vec<WholeFrameCase> {
    vec![
        WholeFrameCase {
            name: "before any object is laid",
            position: |_, _| 200,
            says: "names no extruded object laid at this point",
            ..WHOLE
        },
        WholeFrameCase {
            name: "a G-code laying an object on no layer",
            slice: "no layers",
            position: at_its_end,
            says: "names no layer at this point",
            ..WHOLE
        },
        WholeFrameCase {
            name: "a spool the layer's toolpath cannot be read from",
            aligned_over: Some(PETG_FIRST_LAYER(passed)),
            frame: || frame(PETG, FRAMES[0]),
            says: "the layer's planned toolpath could not be read from the print's spool",
            shown: ((1280, 720), false),
            before_check: Some(spool_unwritable),
            ..WHOLE
        },
        WholeFrameCase {
            name: "no camera snapshot address, an alert's frame",
            says: "no camera snapshot address is configured",
            by_alert: true,
            snapshot: false,
            ..WHOLE
        },
        WholeFrameCase {
            name: "a check image that cannot be written",
            aligned_over: Some(PETG_FIRST_LAYER(passed)),
            frame: marked_frame,
            says: "check image could not be made",
            shown: ((1920, 1080), false),
            shown_as: "the frame as the camera took it",
            before_check: Some(check_images_unwritable),
            ..WHOLE
        },
    ]
}

/// Every case of a check of a print shown the whole frame for what its
/// configuration, its alignment or its frame is.
fn configuration_and_frame_cases() -> Vec<WholeFrameCase> {
    vec![
        WholeFrameCase {
            name: "no calibration",
            slice: "jar-top-petg.gcode",
            calibration: None,
            aligned_over: None,
            position: laying_walls,
            frame: marked_frame,
            says: "no camera calibration is configured",
            shown: ((1280, 720), true),
            ..WHOLE
        },
        WholeFrameCase {
            name: "a G-code labelling no extrusion",
            slice: "",
            calibration: Some(as_committed),
            aligned_over: Some(Aligned {
                at: at_its_end,
                ..PETG_FIRST_LAYER(unavailable)
            }),
            position: at_its_end,
            frame: marked_frame,
            says: "lays no object to align",
            shown: ((1280, 720), true),
            ..WHOLE
        },
        WholeFrameCase {
            name: "alignment pending",
            slice: "jar-top-petg.gcode",
            calibration: Some(as_committed),
            aligned_over: None,
            position: before_first_layer_is_down,
            frame: marked_frame,
            says: "first-layer alignment is pending",
            shown: ((1280, 720), true),
            ..WHOLE
        },
        WholeFrameCase {
            name: "alignment failed on its offset",
            slice: "jar-top-petg.gcode",
            calibration: Some(moved_calibration),
            aligned_over: Some(PETG_FIRST_LAYER(failed)),
            position: laying_walls,
            frame: || frame(PETG, FRAMES[0]),
            says: "first-layer alignment failed: the printed first layer lies",
            shown: ((1280, 720), false),
            ..WHOLE
        },
        WholeFrameCase {
            name: "alignment failed on the frame's resolution",
            slice: "fan-cut-test.gcode",
            calibration: Some(as_committed),
            aligned_over: Some(Aligned {
                case: "fan-cut-bridge",
                frame: "01-healthy-before-fault-p41.jpg",
                at: |index, bytes| position_at(index, bytes, 1.0),
                reaches: failed,
            }),
            position: |index, bytes| position_at(index, bytes, 1.0),
            frame: || frame("fan-cut-bridge", "01-healthy-before-fault-p41.jpg"),
            says: "the camera's frames are 1280x720 and the calibration's 1920x1080",
            shown: ((1280, 720), false),
            ..WHOLE
        },
        WholeFrameCase {
            name: "a camera moved past the drift tolerance",
            slice: "jar-top-petg.gcode",
            calibration: Some(as_committed),
            aligned_over: Some(PETG_FIRST_LAYER(passed)),
            position: laying_walls,
            frame: || shifted(&frame(PETG, FRAMES[0]), 20),
            says: "the camera has moved since it was calibrated",
            shown: ((1280, 720), false),
            ..WHOLE
        },
        WholeFrameCase {
            name: "too few landmarks visible to verify drift",
            slice: "jar-top-petg.gcode",
            calibration: Some(as_committed),
            aligned_over: Some(PETG_FIRST_LAYER(passed)),
            position: laying_walls,
            frame: marked_frame,
            says: "fewer than two of the calibration's landmarks are visible",
            shown: ((1280, 720), true),
            ..WHOLE
        },
    ]
}

/// Every check of a print that cannot be shown a trustworthy three-panel
/// image is shown the whole frame, scaled whole to 1280x720 and never
/// cropped, its prompt's image line naming why; and a look at the printer,
/// which names no print — a start's bed check — answers the same, saying so.
#[tokio::test(flavor = "multi_thread")]
async fn every_other_check_is_shown_the_whole_frame_with_the_reason() {
    let calibration = committed();
    for installer in INSTALLERS {
        for case in whole_frame_cases() {
            let bytes = match case.slice {
                "" => UNLABELLED.to_vec(),
                "no layers" => NO_LAYERS.to_vec(),
                named => slice(named),
            };
            let spool = TempDir::new().expect("a spool directory");
            let index = index_of(&bytes, &spool.path().join("index.gcode"));
            let position = (case.position)(&index, bytes.len());
            let configured_calibration = case.calibration.map(|edit| edit(&calibration));
            let journey = Journey::new(installer, &bytes, position);
            let mut print_id = None;
            if let Some(over) = &case.aligned_over {
                let (aligned_print, verdict) = aligned(
                    &journey,
                    configured_calibration.as_ref().expect("a calibration"),
                    frame(over.case, over.frame),
                    (over.at)(&index, bytes.len()),
                )
                .await;
                assert!(
                    (over.reaches)(&verdict),
                    "{installer}, {}: the alignment was {verdict:?}",
                    case.name
                );
                print_id = Some(aligned_print);
                journey.at(position);
            }
            if let Some(prepare) = case.before_check {
                prepare(
                    &journey.root.path().join("state"),
                    print_id.expect("the print is known once its alignment is"),
                );
            }
            let camera = Camera {
                host: ScriptedHost::answering(vec![Scripted::frame((case.frame)())]).await,
                calibration: configured_calibration,
                snapshot: case.snapshot,
            };
            let interval = if case.by_alert { 3600 } else { 1 };
            let started = journey.start(&camera, interval, &json!([])).await;
            if case.by_alert {
                adopted(&started.server).await;
                alert(&started.server, &camera.host.url()).await;
            }
            let seen = journey.runs(1).await;
            let checked = read_handed(&seen[0]);
            let what = format!("{installer}, {}: {}", case.name, checked.note);
            assert!(checked.note.starts_with(case.shown_as), "{what}");
            assert!(checked.note.contains(case.says), "{what}");
            let (size, marked) = case.shown;
            if marked {
                is_the_whole_frame_scaled(&checked.image, &what);
            } else {
                let decoded = image::load_from_memory(&checked.image)
                    .expect("an image")
                    .to_rgb8();
                assert_eq!(decoded.dimensions(), size, "{what}");
            }
            started.server.stop().await;
        }
    }
}

/// A look at the printer names no print, so it answers the whole frame
/// scaled, saying there is no plan to draw — the bed check before a start.
#[tokio::test(flavor = "multi_thread")]
async fn a_look_at_the_printer_answers_the_whole_frame_saying_no_print_is_named() {
    let bytes = slice("jar-top-petg.gcode");
    for installer in INSTALLERS {
        let journey = Journey::new(installer, &bytes, 0);
        journey.octoprint.reports("Operational", JobScript::idle());
        let camera = Camera {
            host: ScriptedHost::answering(vec![Scripted::frame(marked_frame_of((1920, 1080)))])
                .await,
            calibration: Some(committed()),
            snapshot: true,
        };
        let started = journey.start(&camera, 3600, &json!([])).await;
        let look: Value = presenting(OPERATOR)
            .get(format!("{}/look", started.server.base_url()))
            .send()
            .await
            .expect("the server answers")
            .json()
            .await
            .expect("JSON");
        let note = look["image_note"].as_str().unwrap_or_default();
        assert!(
            note.starts_with("the whole camera frame, scaled to fit 1280x720")
                && note.contains("no print is named"),
            "{installer}: {look}"
        );
        let shown = std::fs::read(look["image_path"].as_str().expect("a path"))
            .expect("the look's image is readable");
        is_the_whole_frame_scaled(&shown, installer);
        started.server.stop().await;
    }
}

/// Wait until a condition holds.
async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} never happened"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A previous-check record that cannot be read back, or that names a frame
/// gone from the store or one that no longer decodes, leaves panel 3 empty rather than the check without an
/// image; and the record goes once the print closes, and is swept at the next
/// start when one is left behind.
#[tokio::test(flavor = "multi_thread")]
async fn a_lost_previous_frame_empties_panel_3_and_the_record_goes_with_the_print() {
    let bytes = slice("jar-top-petg.gcode");
    let calibration = committed();
    let frames = FRAMES.map(|file| frame(PETG, file));
    let installer = INSTALLERS[0];
    let spool = TempDir::new().expect("a spool directory");
    let index = index_of(&bytes, &spool.path().join("index.gcode"));
    let past_first = position_at(&index, bytes.len(), 0.6);
    let walls = position_laying(
        &index,
        bytes.len(),
        position_at(&index, bytes.len(), 5.0),
        true,
    );
    let journey = Journey::new(installer, &bytes, past_first);
    let (print_id, verdict) =
        aligned(&journey, &calibration, frame(PETG, FIRST_LAYER), past_first).await;
    assert!(
        matches!(verdict, PrintAlignment::Passed { .. }),
        "{verdict:?}"
    );
    journey.at(walls);
    let record = journey
        .root
        .path()
        .join("state")
        .join("previous-check")
        .join(format!("{print_id}.json"));
    std::fs::write(&record, b"not a record").expect("a record is corrupted");

    let camera = Camera {
        host: ScriptedHost::answering(vec![
            Scripted::frame(frames[0].clone()),
            Scripted::frame(frames[1].clone()),
            // The store keeps a frame by its bytes, so the third is another.
            Scripted::frame(frames[2].clone()),
        ])
        .await,
        calibration: Some(calibration.clone()),
        snapshot: true,
    };
    let url = camera.host.url();
    // Each alert's check is composed of the camera's own frame, the next of
    // the script, whatever the detector's picture is.
    let detector =
        ScriptedHost::answering(vec![Scripted::frame(marked_frame_of(DETECTOR_PICTURE))]).await;
    let calibrated = calibration_at(&calibration, &url);
    let started = journey.start(&camera, 3600, &json!([])).await;
    assert_eq!(adopted(&started.server).await, print_id);
    alert(&started.server, &detector.url()).await;
    let first = read_handed(&journey.runs(1).await[0]);
    assert_eq!(
        first.image,
        expected_composite(&index, walls, &frames[0], None, &url, &calibrated),
        "a record that cannot be read back leaves panel 3 empty"
    );
    assert!(first.note.contains("panel 3 empty"), "{}", first.note);

    // The record now names the first alert's stored frame; take that away.
    let named: Value =
        printobserver_types::serde_json::from_slice(&std::fs::read(&record).expect("a record"))
            .expect("the record is JSON");
    std::fs::remove_file(named["frame"].as_str().expect("a frame path"))
        .expect("the stored frame is removable");
    alert(&started.server, &detector.url()).await;
    let second = read_handed(&journey.runs(2).await[1]);
    assert_eq!(
        second.image,
        expected_composite(&index, walls, &frames[1], None, &url, &calibrated),
        "a previous frame gone from the store leaves panel 3 empty"
    );
    assert!(second.note.contains("panel 3 empty"), "{}", second.note);

    // The record now names the second alert's frame; make it no image.
    let named: Value =
        printobserver_types::serde_json::from_slice(&std::fs::read(&record).expect("a record"))
            .expect("the record is JSON");
    std::fs::write(
        named["frame"].as_str().expect("a frame path"),
        b"these bytes are no image",
    )
    .expect("the stored frame is writable");
    alert(&started.server, &detector.url()).await;
    let third = read_handed(&journey.runs(3).await[2]);
    assert_eq!(
        third.image,
        expected_composite(&index, walls, &frames[2], None, &url, &calibrated),
        "a previous frame that no longer decodes leaves panel 3 empty"
    );

    journey.octoprint.reports("Operational", JobScript::idle());
    until("the closed print's record is swept", || !record.exists()).await;
    started.server.stop().await;

    std::fs::write(&record, b"{}").expect("a record is left behind");
    let restarted = journey.start(&camera, 3600, &json!([])).await;
    until("a closed print's record is swept at start", || {
        !record.exists()
    })
    .await;
    restarted.server.stop().await;
}

/// Run `printobserver-server-compose` with these arguments.
fn compose(arguments: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_printobserver-server-compose"))
        .args(arguments)
        .output()
        .expect("the composer runs")
}

/// What the composer's journeys hand it: a configuration with and without a
/// calibration or a camera address, a slice, a frame that composes and one
/// that cannot, and where the image goes.
struct Composing {
    journey: Journey,
    _host: ScriptedHost,
    _spool: TempDir,
    url: String,
    bare: PathBuf,
    config: PathBuf,
    no_stream: PathBuf,
    gcode: PathBuf,
    good_frame: PathBuf,
    blind_frame: PathBuf,
    out: PathBuf,
    index: GcodeIndex,
    position: String,
}

impl Composing {
    /// Lay the composer's inputs out under a journey's own root.
    fn new() -> Self {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let bytes = slice("jar-top-petg.gcode");
        let journey = Journey::new(INSTALLERS[0], &bytes, 0);
        let host = runtime.block_on(ScriptedHost::answering(vec![Scripted::frame(Vec::new())]));
        let url = host.url();
        let root = journey.root.path().to_path_buf();
        copy_tree(
            committed_skill().parent().expect("the skill's directory"),
            &root.join("state").join("skills").join("printobserver"),
        );
        let bare = configured(INSTALLERS[0], &journey.root, &journey.octoprint, &url, 3600);
        let mut calibrated = committed();
        calibrated["camera_snapshot_url"] = Value::String(url.clone());
        let calibration_file = root.join("calibration.json");
        std::fs::write(
            &calibration_file,
            printobserver_types::serde_json::to_vec(&calibrated).expect("JSON"),
        )
        .expect("written");
        let with_calibration = |snapshot: bool| {
            let mut document: toml::Value =
                toml::from_str(&std::fs::read_to_string(&bare).expect("the configuration"))
                    .expect("a document");
            let camera = document["camera"].as_table_mut().expect("a camera table");
            camera.insert(
                "calibration".to_owned(),
                toml::Value::String(calibration_file.display().to_string()),
            );
            if !snapshot {
                camera.remove("snapshot_url");
            }
            let path = root.join(format!("calibrated-{snapshot}.toml"));
            std::fs::write(&path, toml::to_string(&document).expect("it renders"))
                .expect("written");
            path
        };
        let config = with_calibration(true);
        let no_stream = with_calibration(false);
        let gcode = root.join("print.gcode");
        std::fs::write(&gcode, &bytes).expect("the slice");
        let good_frame = root.join("frame.jpg");
        std::fs::write(&good_frame, frame(PETG, FRAMES[0])).expect("a frame");
        let blind_frame = root.join("blind.jpg");
        std::fs::write(&blind_frame, marked_frame()).expect("a frame");
        let spool = TempDir::new().expect("a spool directory");
        let index = index_of(&bytes, &spool.path().join("index.gcode"));
        let position = laying_walls(&index, bytes.len()).to_string();
        Self {
            journey,
            _host: host,
            _spool: spool,
            url,
            bare,
            config,
            no_stream,
            gcode,
            good_frame,
            blind_frame,
            out: root.join("check-image.jpg"),
            index,
            position,
        }
    }

    /// Run the composer over these inputs, at the walls' position.
    fn run(&self, config: &Path, gcode: &Path, frame: &Path, out: &Path) -> std::process::Output {
        let text = |path: &Path| path.to_str().expect("a path").to_owned();
        compose(&[
            "--config",
            &text(config),
            "--gcode",
            &text(gcode),
            "--frame",
            &text(frame),
            "--position",
            &self.position,
            "--out",
            &text(out),
        ])
    }
}

/// The composer writes the check image a print's first check at a file
/// position is shown, and says its size and the image line.
#[test]
fn the_composer_writes_the_image_a_first_check_is_shown() {
    let inputs = Composing::new();
    let done = inputs.run(
        &inputs.config,
        &inputs.gcode,
        &inputs.good_frame,
        &inputs.out,
    );
    assert_eq!(
        done.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let said: Value = printobserver_types::serde_json::from_slice(&done.stdout).expect("JSON");
    let written = std::fs::read(&inputs.out).expect("the image is written");
    assert_eq!(said["width"], 1024);
    assert_eq!(said["height"], 1848);
    assert_eq!(said["bytes"], written.len());
    assert!(
        said["note"]
            .as_str()
            .is_some_and(|note| note.starts_with(printobserver_server::COMPOSITE_NOTE)),
        "{said}"
    );
    assert_eq!(
        written,
        expected_composite(
            &inputs.index,
            inputs.position.parse().expect("a position"),
            &frame(PETG, FRAMES[0]),
            None,
            &inputs.url,
            &calibration_at(&committed(), &inputs.url),
        ),
        "the composer's image is the one a print's first check at that position is shown"
    );
}

/// Arguments the composer cannot read exit 2, and a configuration, file or
/// frame it cannot use exits 1, each naming why and printing nothing.
#[test]
fn the_composer_refuses_what_it_cannot_compose_naming_why() {
    for (arguments, said) in [
        (vec![], "--config is required"),
        (vec!["--colour", "red"], "is no option"),
        (vec!["--out", "a", "--out", "b"], "is given twice"),
        (vec!["--out", ""], "takes a value"),
        (
            vec![
                "--config",
                "c",
                "--gcode",
                "g",
                "--frame",
                "f",
                "--position",
                "x",
                "--out",
                "o",
            ],
            "is no count of bytes",
        ),
    ] {
        let done = compose(&arguments);
        assert_eq!(done.status.code(), Some(2), "{arguments:?}");
        assert!(done.stdout.is_empty(), "{arguments:?} printed an answer");
        let stderr = String::from_utf8_lossy(&done.stderr);
        assert!(stderr.contains(said), "{arguments:?}: {stderr}");
    }
    let inputs = Composing::new();
    let root = inputs.journey.root.path();
    let missing = root.join("absent");
    let elsewhere = root.join("no-such-directory").join("image.jpg");
    let (config, gcode, good, out) = (
        &inputs.config,
        &inputs.gcode,
        &inputs.good_frame,
        &inputs.out,
    );
    for ((config, gcode, frame, out), said) in [
        ((&missing, gcode, good, out), "absent"),
        (
            (&inputs.bare, gcode, good, out),
            "names no camera.calibration",
        ),
        (
            (&inputs.no_stream, gcode, good, out),
            "names no camera.snapshot_url",
        ),
        ((config, &missing, good, out), "is unreadable"),
        ((config, gcode, &missing, out), "is unreadable"),
        (
            (config, gcode, &inputs.blind_frame, out),
            "no check image: fewer than two",
        ),
        ((config, gcode, good, &elsewhere), "cannot be written"),
    ] {
        let done = inputs.run(config, gcode, frame, out);
        assert_eq!(done.status.code(), Some(1), "{said}");
        assert!(done.stdout.is_empty(), "{said}: it printed an answer");
        let stderr = String::from_utf8_lossy(&done.stderr);
        assert!(stderr.contains(said), "{said}: {stderr}");
    }
}

/// The probe refuses an image it could not attach — none there, or a file
/// that is no image — before it reads any configuration or runs a turn.
#[test]
fn the_probe_refuses_an_image_it_cannot_attach() {
    let root = TempDir::new().expect("a journey's own root");
    let not_an_image = root.path().join("notes.txt");
    std::fs::write(&not_an_image, b"no picture here").expect("written");
    let signature_only = root.path().join("signature.jpg");
    std::fs::write(&signature_only, [0xff, 0xd8, 0xff, 0xe0]).expect("written");
    let absent = root.path().join("absent.jpg");
    for (image, said) in [
        (&absent, "absent.jpg is unreadable"),
        (&not_an_image, "no JPEG image"),
        (&signature_only, "no JPEG image"),
    ] {
        let done = std::process::Command::new(env!("CARGO_BIN_EXE_printobserver-server-probe"))
            .args([
                "--config",
                "missing.toml",
                "--session",
                "s",
                "--prompt",
                "p",
                "--image",
                image.to_str().expect("a path"),
            ])
            .output()
            .expect("the probe runs");
        assert_eq!(done.status.code(), Some(1), "{image:?}");
        assert!(done.stdout.is_empty(), "it printed an answer");
        let stderr = String::from_utf8_lossy(&done.stderr);
        assert!(
            stderr.contains(said) && !stderr.contains("missing.toml"),
            "{stderr}"
        );
    }
}
