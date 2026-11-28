//! Each print's first-layer alignment, as the running service judges it on a
//! frame — here the operator's recheck of the camera, which judges a fresh
//! frame at once — and the checks read it through
//! [`Running::print_alignment`]. A scheduled check's own frame is judged the
//! same way, which `camera_view` drives.
//!
//! The service is the real composition over the recording printer, which
//! serves a recorded case's own slice (a plain copy from
//! `printobserver-core`'s fixtures) and reports the job's file position, and a
//! scripted camera answering one of that case's recorded frames (a plain copy
//! from `printobserver-panels`' fixtures). The calibration is the committed
//! one, its `camera_snapshot_url` set to the scripted camera's address, since
//! that is the stream the journey's frames come from.
//!
//! [`Running::print_alignment`]: printobserver_server::Running::print_alignment

use core::time::Duration;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_core::{GcodeIndex, IndexBuilder};
use printobserver_panels::{BedPose, CameraCalibration};
use printobserver_printer_api::{GcodeSink as _, PrinterState};
use printobserver_server::{AlignmentFailure, PrintAlignment, Running};
use printobserver_types::PrintId;
use printobserver_types::serde_json::{self, Value};
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::camera_host::{Scripted, ScriptedHost};
use crate::printer::RecordingPrinter;
use crate::world::{World, set};

/// The operation beside the action vocabulary these journeys drive, as
/// `composite_checks`' do: the operator's recheck of a print's camera. Each of
/// them reaches it at [`recheck_url`].
pub const DRIVEN_THERE: [&str; 1] = [printobserver_server::RECHECK_CAMERA];

/// Where the operator's recheck of one print's camera is served, as the server
/// declares the operation [`DRIVEN_THERE`] names.
pub fn recheck_url(server: &Running, print_id: PrintId) -> String {
    let [recheck] = DRIVEN_THERE;
    let declared =
        printobserver_server::operation(recheck).unwrap_or_else(|| panic!("`{recheck}` is served"));
    format!(
        "{}{}",
        server.base_url(),
        declared.path.replace("{print_id}", &print_id.to_string())
    )
}

/// The file the printer runs, as every journey here names it.
const FILE: &str = "plate.gcode";

/// How long a journey waits for something the service does on its own clock.
const PATIENCE: Duration = Duration::from_secs(60);

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
    serde_json::from_slice(
        &std::fs::read(root().join("tests/real-prints/cameras/core-one-2026-09-27.json"))
            .expect("the committed calibration"),
    )
    .expect("JSON")
}

/// A slice's index, built by the real builder with no spool.
fn index_of(bytes: &[u8]) -> GcodeIndex {
    let mut builder = IndexBuilder::new();
    let _ = builder.take(bytes);
    builder.finish()
}

/// The committed calibration with its bed moved along the camera's X until
/// the slice's first object outline moves 49 px in the frame: just past the
/// smallest failed projection measured, 48.7 px.
fn moved_calibration(calibration: Value, bytes: &[u8]) -> Value {
    let base = CameraCalibration::from_json(&serde_json::to_vec(&calibration).expect("JSON"))
        .expect("the committed calibration is valid");
    let index = index_of(bytes);
    let points: Vec<[f64; 3]> = index.object_outlines()[0]
        .hull_mm
        .iter()
        .map(|[x, y]| [f64::from(*x), f64::from(*y), 0.0])
        .collect();
    let size = [100_000, 100_000];
    let at = |shift: f64| {
        let mut translation = base.bed_pose.translation_mm;
        translation[0] += shift;
        base.with_bed_pose(BedPose {
            rotation_rodrigues: base.bed_pose.rotation_rodrigues,
            translation_mm: translation,
        })
    };
    let moved = |calibration: &CameraCalibration| {
        let (a, b) = (
            base.project(&points, 0.6, size),
            calibration.project(&points, 0.6, size),
        );
        let moves: Vec<f64> = a
            .iter()
            .zip(&b)
            .filter_map(|(p, q)| Some((p.as_ref()?, q.as_ref()?)))
            .map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]))
            .collect();
        moves.iter().sum::<f64>() / f64::from(u32::try_from(moves.len()).expect("a count"))
    };
    let (mut low, mut high) = (0.0, 40.0);
    for _ in 0..60 {
        let middle = f64::midpoint(low, high);
        if moved(&at(middle)) < 49.0 {
            low = middle;
        } else {
            high = middle;
        }
    }
    let mut changed = calibration;
    changed["bed_pose"]["translation_mm"][0] =
        serde_json::json!(base.bed_pose.translation_mm[0] + low);
    changed
}

/// A file position inside the first layer at or above `z`.
fn position_at(index: &GcodeIndex, bytes: usize, z: f64) -> i64 {
    let size = u64::try_from(bytes).expect("a size");
    let found = (0..size)
        .step_by(500)
        .find(|position| {
            index
                .layer_at(*position)
                .and_then(|layer| index.layer_z_mm(layer))
                .is_some_and(|held| held >= z - 1e-6)
        })
        .expect("a layer at that height");
    i64::try_from(found + 100).expect("a position")
}

/// One journey's service, camera and calibration file.
struct Aligning {
    /// The service.
    world: World,
    /// The camera; dropped with the journey.
    camera: ScriptedHost,
    /// Where the calibration file is; dropped with the journey.
    _calibration: TempDir,
}

/// Start the service over a slice, a camera answering one frame, and the
/// committed calibration as `calibration` edits it — or none.
async fn start(
    bytes: &[u8],
    frame: Vec<Scripted>,
    calibration: Option<fn(Value, &[u8]) -> Value>,
) -> Aligning {
    let printer = RecordingPrinter::printing();
    printer.holds_gcode(FILE, bytes);
    printer.at_byte(0);
    let camera = ScriptedHost::answering(frame).await;
    let held = TempDir::new().expect("a directory for the calibration");
    let path = held.path().join("calibration.json");
    if let Some(edit) = calibration {
        let mut written = committed();
        written["camera_snapshot_url"] = Value::String(camera.url());
        let written = edit(written, bytes);
        std::fs::write(&path, serde_json::to_vec(&written).expect("JSON")).expect("written");
    }
    let url = camera.url();
    let world = World::tuned(
        printer,
        StandInAgent::new(),
        |document| {
            let mut table =
                toml::Table::from_iter([("snapshot_url".to_owned(), toml::Value::String(url))]);
            if calibration.is_some() {
                table.insert(
                    "calibration".to_owned(),
                    toml::Value::String(path.display().to_string()),
                );
            }
            set(document, "camera", toml::Value::Table(table));
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(3600),
            );
        },
        |config| config.printer_poll = Duration::from_millis(200),
        |stores| stores,
    )
    .await;
    Aligning {
        world,
        camera,
        _calibration: held,
    }
}

/// The committed calibration, its stream the journey's camera.
fn as_committed(calibration: Value, _: &[u8]) -> Value {
    calibration
}

/// The print the service adopted for the running job.
async fn adopted(world: &World) -> PrintId {
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        if let Some(print) = world
            .stores
            .prints
            .open_prints()
            .await
            .expect("the prints read")
            .first()
        {
            return print.id;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the running job was never adopted"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Judge a print's camera again as the operator, and answer the status and
/// the answer.
async fn recheck(world: &World, print_id: PrintId) -> (reqwest::StatusCode, Value) {
    let response = world
        .client
        .post(recheck_url(&world.server, print_id))
        .json(&serde_json::json!({ "actor": "operator", "reason": "the camera was nudged" }))
        .send()
        .await
        .expect("the server answers");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// Judge a print's camera again, and answer the verdict the recheck left.
async fn decided(world: &World, print_id: PrintId) -> PrintAlignment {
    let (status, answer) = recheck(world, print_id).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    let verdict = world.server.print_alignment(print_id);
    assert_eq!(
        serde_json::to_value(&verdict).expect("a verdict renders"),
        answer["alignment"],
        "the recheck answers the verdict it left"
    );
    verdict
}

/// The first-layer judgements a print's history records, oldest first.
async fn judgements(world: &World, print_id: PrintId) -> Vec<Value> {
    let mut events: Vec<printobserver_types::EventRecord> = world
        .stores
        .events
        .history(printobserver_core::store::HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: Some(1000),
        })
        .await
        .expect("the history reads");
    events.sort_by_key(|event| event.id);
    events
        .iter()
        .filter(|event| event.kind().as_str() == "first_layer_alignment_judged")
        .map(|event| event.body.payload.clone())
        .collect()
}

/// Wait until a condition holds.
async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + PATIENCE;
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "{what} never happened"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The verdict file and the spool of one print.
fn kept(world: &World, print_id: PrintId) -> (PathBuf, PathBuf) {
    let state = world.root.path().join("state");
    (
        state.join("alignment").join(format!("{print_id}.json")),
        state.join("gcode-spool").join(format!("{print_id}.gcode")),
    )
}

/// A print reads `Pending` until its first object layer is down — an earlier
/// release's failure on its offset among it — then `Passed` with its offset and
/// its correction; the verdict and the spool stand through a restart while it
/// is open, and are gone once it closes — and a spool or verdict left for a
/// closed print is swept at the next start.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_is_judged_once_its_first_layer_is_down_and_keeps_its_verdict_while_open() {
    let bytes = slice("jar-top-petg.gcode");
    let index = index_of(&bytes);
    let (_, first_ends) = index.first_object_layer().expect("a first object layer");
    let journey = start(
        &bytes,
        vec![Scripted::frame(frame(
            "jar-top-petg-healthy",
            "agent-007-obs-121230-p01.3.jpg",
        ))],
        Some(as_committed),
    )
    .await;
    let printer = Arc::clone(&journey.world.printer);
    printer.at_byte(i64::try_from(first_ends).expect("a position") - 50);
    let print_id = adopted(&journey.world).await;
    let (verdict_file, spool) = kept(&journey.world, print_id);
    assert_eq!(
        decided(&journey.world, print_id).await,
        PrintAlignment::Pending,
        "a print whose first layer is not down is pending, whatever its frame shows"
    );
    until("the spool is written", || spool.exists()).await;
    assert!(
        judgements(&journey.world, print_id).await.is_empty(),
        "nothing is judged of a first layer that is not down"
    );

    std::fs::write(
        &verdict_file,
        br#"{"verdict":"failed","reason":{"failure":"offset","offset_px":59.7}}"#,
    )
    .expect("an earlier release's verdict");
    assert_eq!(
        journey.world.server.print_alignment(print_id),
        PrintAlignment::Pending,
        "an earlier release's failure on its offset is judged again"
    );

    std::fs::write(&verdict_file, b"not a verdict").expect("a verdict file is corrupted");
    assert_eq!(
        journey.world.server.print_alignment(print_id),
        PrintAlignment::Pending,
        "a verdict that cannot be read back is no verdict"
    );
    printer.at_byte(position_at(&index, bytes.len(), 0.6));
    let verdict = decided(&journey.world, print_id).await;
    let PrintAlignment::Passed {
        offset_px,
        correction_px,
    } = verdict
    else {
        panic!("the healthy first layer did not pass: {verdict:?}");
    };
    // Judged at Z 0.6, more than one layer above the first, where the search
    // scores the part's body as much as the layer: the pass corrects by
    // nothing. `camera_view` drives a pass that corrects.
    assert!(offset_px > 0.0, "the layer was found {offset_px:.1} px off");
    assert!(
        correction_px[0].hypot(correction_px[1]) < 1e-9,
        "a pass this far above its first layer corrects by nothing, not {correction_px:?}"
    );
    assert!(
        offset_px <= 30.0,
        "offset {offset_px:.1} px against a threshold of 30 px"
    );
    assert_eq!(
        printer.gcode_reads(FILE),
        1,
        "the index and the first layer's toolpath came from one read of the file"
    );

    let Aligning {
        world,
        camera: _camera,
        _calibration,
    } = journey;
    let world = Box::pin(world.restart()).await;
    assert_eq!(
        world.server.print_alignment(print_id),
        PrintAlignment::Passed {
            offset_px,
            correction_px
        },
        "the verdict stands through a restart while the print is open"
    );
    assert!(
        spool.exists(),
        "the open print's spool stands through the restart"
    );

    printer.runs_job(FILE, PrinterState::Operational, None);
    until(
        "the print's verdict and spool are gone once it closes",
        || !verdict_file.exists() && !spool.exists(),
    )
    .await;

    std::fs::write(&verdict_file, b"{\"verdict\":\"pending\"}").expect("a stray verdict");
    std::fs::write(&spool, b"G1 X0").expect("a stray spool");
    let world = Box::pin(world.restart()).await;
    assert!(
        !verdict_file.exists(),
        "a closed print's verdict is swept at start"
    );
    assert!(!spool.exists(), "a closed print's spool is swept at start");
    drop(world);
}

/// A sweep leaves an open print's record that is still being written: each is
/// written beside its file and renamed over it, and the watch sweeps after
/// every read, so a sweep landing between the two removed the half it had not
/// renamed yet — and the last check's frame was lost, its next check showing
/// none as the previous (`test (macos-aarch64)`, run 44992224273).
#[tokio::test(flavor = "multi_thread")]
async fn a_sweep_leaves_an_open_prints_record_part_way_through_its_write() {
    let bytes = slice("jar-top-petg.gcode");
    let journey = start(
        &bytes,
        vec![Scripted::frame(frame(
            "jar-top-petg-healthy",
            "agent-007-obs-121230-p01.3.jpg",
        ))],
        Some(as_committed),
    )
    .await;
    let print_id = adopted(&journey.world).await;
    let state = journey.world.root.path().join("state");
    let writing: Vec<PathBuf> = [
        "alignment",
        "previous-check",
        "checked-frames",
        "camera-view",
    ]
    .iter()
    .map(|directory| {
        let directory = state.join(directory);
        std::fs::create_dir_all(&directory).expect("the record directory");
        let part = directory.join(format!("{print_id}.json.part"));
        std::fs::write(&part, b"{").expect("a record part-way through its write");
        part
    })
    .collect();
    let closed = state
        .join("previous-check")
        .join(format!("{}.json", printobserver_types::PrintId::new()));
    std::fs::write(&closed, b"{}").expect("a closed print's record");
    let closed_frames = state
        .join("checked-frames")
        .join(format!("{}.json", printobserver_types::PrintId::new()));
    std::fs::write(&closed_frames, b"[]").expect("a closed print's checked frames");

    until("a sweep removes a closed print's records", || {
        !closed.exists() && !closed_frames.exists()
    })
    .await;

    for part in &writing {
        assert!(
            part.exists(),
            "{} was swept while its print was open",
            part.display()
        );
    }
}

/// A calibration whose projection is off by 49 px is inconclusive, not
/// clearly found, and stays pending to be judged again; a camera of another
/// resolution fails naming it, and stays failed through a restart; and with no
/// calibration, or a slice laying no object, a print is unavailable.
#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_projection_is_inconclusive_a_wrong_resolution_fails_and_nothing_to_check_is_unavailable()
 {
    let bytes = slice("jar-top-petg.gcode");
    let index = index_of(&bytes);
    let past = position_at(&index, bytes.len(), 0.6);
    let petg = vec![Scripted::frame(frame(
        "jar-top-petg-healthy",
        "agent-007-obs-121230-p01.3.jpg",
    ))];

    let moved = start(&bytes, petg.clone(), Some(moved_calibration)).await;
    moved.world.printer.at_byte(past);
    let print_id = adopted(&moved.world).await;
    assert_eq!(
        decided(&moved.world, print_id).await,
        PrintAlignment::Pending
    );
    let judged = judgements(&moved.world, print_id).await;
    assert_eq!(judged.len(), 1, "{judged:#?}");
    assert_eq!(judged[0]["verdict"], "inconclusive");
    assert_eq!(judged[0]["reason"], "not_clearly_found");
    assert!(
        judged[0]["offset_px"].as_f64().expect("an offset") > 30.0,
        "{judged:#?}"
    );
    drop(moved);

    let fan = slice("fan-cut-test.gcode");
    let small = start(
        &fan,
        vec![Scripted::frame(frame(
            "fan-cut-bridge",
            "01-healthy-before-fault-p41.jpg",
        ))],
        Some(as_committed),
    )
    .await;
    let fan_index = index_of(&fan);
    small
        .world
        .printer
        .at_byte(position_at(&fan_index, fan.len(), 1.0));
    let print_id = adopted(&small.world).await;
    let failed = PrintAlignment::Failed {
        reason: AlignmentFailure::Resolution {
            expected: [1920, 1080],
            got: [1280, 720],
        },
    };
    assert_eq!(decided(&small.world, print_id).await, failed);
    let judged = judgements(&small.world, print_id).await;
    assert_eq!(judged[0]["verdict"], "failed");
    assert_eq!(
        judged[0]["reason"],
        "the camera's frames are 1280x720 and the calibration's 1920x1080"
    );
    let Aligning {
        world,
        camera: _camera,
        _calibration,
    } = small;
    let world = Box::pin(world.restart()).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        world.server.print_alignment(print_id),
        failed,
        "a failed print stays failed"
    );
    drop(world);

    let uncalibrated = start(&bytes, petg.clone(), None).await;
    uncalibrated.world.printer.at_byte(past);
    let print_id = adopted(&uncalibrated.world).await;
    assert_eq!(
        uncalibrated.world.server.print_alignment(print_id),
        PrintAlignment::Unavailable
    );
    let (status, answer) = recheck(&uncalibrated.world, print_id).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(
        answer["image_fallback"]["code"], "no_calibration",
        "with no calibration there is nothing to judge a frame against: {answer}"
    );
    assert!(
        std::path::Path::new(answer["frame_path"].as_str().expect("a path")).is_file(),
        "the fresh frame is kept all the same: {answer}"
    );
    assert!(judgements(&uncalibrated.world, print_id).await.is_empty());
    drop(uncalibrated);

    let unlabelled = b";LAYER_CHANGE\n;Z:0.2\nG90\nM83\nG1 X10 Y10\nG1 X60 Y10 E2\nG1 X60 Y60 E2\n;LAYER_CHANGE\n;Z:0.4\nG1 X10 Y60 E2\n";
    let bare = start(unlabelled, petg, Some(as_committed)).await;
    bare.world
        .printer
        .at_byte(i64::try_from(unlabelled.len()).expect("a size") - 2);
    let print_id = adopted(&bare.world).await;
    assert_eq!(
        decided(&bare.world, print_id).await,
        PrintAlignment::Unavailable,
        "a slice labelling no extrusion type lays no object the check could find"
    );
}

/// The service starts with a `[camera] calibration` naming a valid file, and
/// refuses to start naming `camera.calibration` when the file is missing or
/// is not a calibration.
#[tokio::test(flavor = "multi_thread")]
async fn a_calibration_is_validated_at_start() {
    let held = TempDir::new().expect("a directory for the files");
    let valid = held.path().join("valid.json");
    std::fs::write(&valid, serde_json::to_vec(&committed()).expect("JSON")).expect("written");
    let named = |path: &Path| {
        let path = path.display().to_string();
        move |document: &mut toml::Value| {
            set(
                document,
                "camera",
                toml::Value::Table(toml::Table::from_iter([(
                    "calibration".to_owned(),
                    toml::Value::String(path),
                )])),
            );
        }
    };
    let world = World::configured(
        RecordingPrinter::printing(),
        StandInAgent::new(),
        named(&valid),
    )
    .await;
    assert!(
        world.server.config().camera_calibration.is_some(),
        "the valid calibration is in force"
    );
    drop(world);

    let invalid = held.path().join("invalid.json");
    let mut broken = committed();
    broken["intrinsics"]["fx"] = serde_json::json!(-1.0);
    std::fs::write(&invalid, serde_json::to_vec(&broken).expect("JSON")).expect("written");
    for path in [held.path().join("missing.json"), invalid] {
        let root = TempDir::new().expect("a journey's own root");
        let mut document = crate::world::document(root.path(), "http://127.0.0.1:1");
        named(&path)(&mut document);
        let config = crate::world::write(root.path(), &document);
        let refusal = printobserver_server::Server::start(&config)
            .await
            .err()
            .unwrap_or_else(|| panic!("{} was accepted", path.display()));
        assert_eq!(
            refusal.field(),
            Some(printobserver_server::ConfigField::CameraCalibration),
            "{refusal}"
        );
        assert!(
            refusal.to_string().contains("camera.calibration"),
            "{refusal}"
        );
    }
}

/// The committed calibration fitted on another stream than the journey's
/// camera.
fn other_stream(mut calibration: Value, _: &[u8]) -> Value {
    calibration["camera_snapshot_url"] = Value::String("http://127.0.0.1:9/elsewhere".to_owned());
    calibration
}

/// A camera that is not the calibration's stream fails naming the stream; a
/// first layer projecting nowhere in the frame is not clearly found and stays
/// pending; and a fetch that gives no good frame judges nothing, which a later
/// recheck then does.
#[tokio::test(flavor = "multi_thread")]
async fn another_stream_fails_an_off_frame_layer_is_inconclusive_and_a_bad_fetch_judges_nothing() {
    let bytes = slice("jar-top-petg.gcode");
    let index = index_of(&bytes);
    let past = position_at(&index, bytes.len(), 0.6);
    let good = frame("jar-top-petg-healthy", "agent-007-obs-121230-p01.3.jpg");

    let elsewhere = start(
        &bytes,
        vec![Scripted::frame(good.clone())],
        Some(other_stream),
    )
    .await;
    elsewhere.world.printer.at_byte(past);
    let print_id = adopted(&elsewhere.world).await;
    assert_eq!(
        decided(&elsewhere.world, print_id).await,
        PrintAlignment::Failed {
            reason: AlignmentFailure::Stream
        }
    );
    drop(elsewhere);

    // The camera sits at the bed's front looking back: a layer laid far in
    // front of the printer is behind it.
    let behind = b";LAYER_CHANGE\n;Z:0.2\nG90\nM83\n;TYPE:External perimeter\nG1 X100 Y-5000\nG1 X150 Y-5000 E2\nG1 X150 Y-4950 E2\n;LAYER_CHANGE\n;Z:0.4\nG1 X100 Y-4950 E2\n";
    let off = start(
        behind,
        vec![Scripted::frame(good.clone())],
        Some(as_committed),
    )
    .await;
    off.world
        .printer
        .at_byte(i64::try_from(behind.len()).expect("a size") - 2);
    let print_id = adopted(&off.world).await;
    assert_eq!(decided(&off.world, print_id).await, PrintAlignment::Pending);
    let judged = judgements(&off.world, print_id).await;
    assert_eq!(judged[0]["reason"], "not_clearly_found", "{judged:#?}");
    assert_eq!(judged[0]["offset_px"], Value::Null, "nothing was searched");
    drop(off);

    let flaky = start(
        &bytes,
        vec![
            Scripted::refusing("404 Not Found", "no frame yet"),
            Scripted::frame(good),
        ],
        Some(as_committed),
    )
    .await;
    flaky.world.printer.at_byte(past);
    let print_id = adopted(&flaky.world).await;
    let (status, answer) = recheck(&flaky.world, print_id).await;
    assert_eq!(status, reqwest::StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(answer["error"], "the camera gave no good frame to judge");
    assert!(judgements(&flaky.world, print_id).await.is_empty());
    let verdict = decided(&flaky.world, print_id).await;
    assert!(
        matches!(verdict, PrintAlignment::Passed { .. }),
        "the recheck passed once a good frame came: {verdict:?}"
    );
    assert_eq!(flaky.camera.exchanges().len(), 2);
}
