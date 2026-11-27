//! Recovery from a slightly shifted camera, over the nineteen check frames of
//! the 2026-11-26 two-colour print, taken under the stale
//! `core-one-2026-09-27.json` the print ran with: its camera had shifted, so
//! its two agreeing landmarks sit about +59 and +79 px from the calibration
//! while the part moved about 15 px.
//!
//! Every assertion states what it observed beside what it requires, and the
//! figures it holds come from the measurement that chose the recovery: the
//! found shifts of the three frames showing the part, the covered and empty
//! frames, and the landmark search radius.

use image::{Rgb, RgbImage};
use printobserver_core::LayerToolpath;
use printobserver_panels::alignment::{
    TOOLHEAD_MARGIN_PX, correction_shift, nozzle_from_first_layer_px,
};
use printobserver_panels::drift::{HIDDEN_BELOW, VIEW_SEARCH_RADIUS_PX, locate_landmarks_within};
use printobserver_panels::{
    AlignmentInputs, CameraCalibration, CheckImageInputs, FirstLayerInputs, FirstLayerJudgement,
    FirstLayerVerdict, InconclusiveReason, ViewCorrection, ViewModel, ViewOutcome,
    compose_check_image, judge_first_layer, judge_view,
};
use printobserver_types::Timestamp;

use crate::support::{
    calibration, decoded, frame, index, layer_at_z, recorded_position, snapshot_url, toolpath,
};

/// The case the frames are of.
const CASE: &str = "two-colour-planned-change";

/// Every frame, in the order they were taken.
const FRAMES: [&str; 19] = [
    "130805-periodic_observation.jpg",
    "131006-periodic_observation.jpg",
    "131207-periodic_observation.jpg",
    "131408-periodic_observation.jpg",
    "131609-periodic_observation.jpg",
    "131810-periodic_observation.jpg",
    "132011-periodic_observation.jpg",
    "132212-periodic_observation.jpg",
    "132413-periodic_observation.jpg",
    "132614-periodic_observation.jpg",
    "132815-periodic_observation.jpg",
    "133015-periodic_observation.jpg",
    "133216-periodic_observation.jpg",
    "133417-periodic_observation.jpg",
    "133618-periodic_observation.jpg",
    "134200-filament_change_planned_resumed.jpg",
    "134401-periodic_observation.jpg",
    "152603-filament_change_planned_resumed.jpg",
    "152804-periodic_observation.jpg",
];

/// The three frames showing the part, and the shift the measurement found
/// each at.
const VISIBLE: [(&str, [f64; 2]); 3] = [
    ("134401-periodic_observation.jpg", [4.0, -20.0]),
    ("152603-filament_change_planned_resumed.jpg", [14.0, 4.0]),
    ("152804-periodic_observation.jpg", [16.0, 4.0]),
];

/// The three frames with the toolhead over the part.
const COVERED: [&str; 3] = [
    "131408-periodic_observation.jpg",
    "133618-periodic_observation.jpg",
    "134200-filament_change_planned_resumed.jpg",
];

/// The two landmarks found agreeing on every frame, and where the
/// measurement found each, `[dx, dy]` px from the calibration.
const AGREEING: [(&str, [f64; 2]); 2] = [
    ("frame-screws-right-front", [59.0, 1.0]),
    ("frame-label-right", [79.0, 1.0]),
];

/// The print's first object layer's toolpath.
fn first_layer() -> LayerToolpath {
    toolpath(
        CASE,
        index(CASE)
            .first_object_layer()
            .expect("the box lays a first layer")
            .0,
    )
}

/// The judgement of one frame at its recorded file position: the layer the
/// plan has reached there gives its print height, and the plan's nozzle there
/// whether the toolhead covers the part.
fn judged(calibration: &CameraCalibration, file: &str, jpeg: &[u8]) -> (FirstLayerJudgement, f64) {
    let index = index(CASE);
    let position = recorded_position(CASE, file);
    let print_z = index
        .layer_at(position)
        .and_then(|layer| index.layer_z_mm(layer))
        .expect("a height at the position");
    let nozzle = index
        .planned_head_at(position)
        .expect("the spool reads back")
        .expect("the plan places the nozzle");
    let first = first_layer();
    let url = snapshot_url();
    let inputs = FirstLayerInputs {
        alignment: AlignmentInputs {
            frame_jpeg: jpeg,
            camera_snapshot_url: &url,
            first_layer: &first,
            print_z_mm: print_z,
            calibration,
        },
        nozzle_xy_mm: Some(nozzle),
    };
    let distance = nozzle_from_first_layer_px(&inputs).expect("the nozzle and the layer project");
    (
        judge_first_layer(&inputs).expect("a calibrated frame is judged"),
        distance,
    )
}

#[test]
fn the_visible_first_layer_passes_near_the_measured_shift_and_the_covered_part_does_not() {
    let calibration = calibration();
    let mut observed = Vec::new();
    for (file, expected) in VISIBLE {
        let (judgement, distance) = judged(&calibration, file, &frame(CASE, file));
        let found = judgement.found.expect("the layer is searched");
        observed.push(format!(
            "{file}: {:?} shift {:?} contrast {:.1}, nozzle {distance:.1} px from the layer",
            judgement.verdict, found.shift_px, found.match_strength
        ));
        assert_eq!(
            judgement.verdict,
            FirstLayerVerdict::Passed,
            "{file} shows the part: {observed:#?}"
        );
        let off = (found.shift_px[0] - expected[0]).hypot(found.shift_px[1] - expected[1]);
        assert!(
            off <= 3.0,
            "{file}: found {:?}, {off:.1} px from the measured {expected:?}",
            found.shift_px
        );
        assert!(
            distance > TOOLHEAD_MARGIN_PX,
            "{file}: the nozzle lies {distance:.1} px from the layer, inside the {TOOLHEAD_MARGIN_PX} px margin"
        );
    }
    for file in COVERED {
        let (judgement, distance) = judged(&calibration, file, &frame(CASE, file));
        observed.push(format!(
            "{file}: {:?}, nozzle {distance:.1} px from the layer",
            judgement.verdict
        ));
        assert_eq!(
            judgement.verdict,
            FirstLayerVerdict::Inconclusive(InconclusiveReason::Covered),
            "{file} has the toolhead over the part: {observed:#?}"
        );
        assert!(distance <= TOOLHEAD_MARGIN_PX, "{file}: {distance:.1} px");
    }
    eprintln!("{observed:#?}");
}

#[test]
fn an_empty_bed_is_not_clearly_found() {
    let calibration = calibration();
    let first = first_layer();
    let url = snapshot_url();
    for file in [
        "130805-periodic_observation.jpg",
        "131006-periodic_observation.jpg",
    ] {
        // Taken before the first layer was down, so judged at that layer's
        // own height, as the measurement judged them.
        let jpeg = frame(CASE, file);
        let judgement = judge_first_layer(&FirstLayerInputs {
            alignment: AlignmentInputs {
                frame_jpeg: &jpeg,
                camera_snapshot_url: &url,
                first_layer: &first,
                print_z_mm: f64::from(first.z_mm),
                calibration: &calibration,
            },
            nozzle_xy_mm: None,
        })
        .expect("a calibrated frame is judged");
        assert_eq!(
            judgement.verdict,
            FirstLayerVerdict::Inconclusive(InconclusiveReason::NotClearlyFound),
            "{file} shows the bed before the first layer: {:?}",
            judgement.found
        );
    }
}

/// The correction a frame's pass puts in force — its found shift within one
/// layer of the first layer, no shift from a taller frame — against the
/// landmarks found agreeing on it, and the shift the search found.
fn correction_on(calibration: &CameraCalibration, file: &str) -> (ViewCorrection, [f64; 2]) {
    let (judgement, _) = judged(calibration, file, &frame(CASE, file));
    assert_eq!(
        judgement.verdict,
        FirstLayerVerdict::Passed,
        "{file} passes"
    );
    let found = judgement.found.expect("searched");
    let index = index(CASE);
    let print_z = index
        .layer_at(recorded_position(CASE, file))
        .and_then(|layer| index.layer_z_mm(layer))
        .expect("a height");
    let view = judge_view(
        calibration,
        &image::imageops::grayscale(&decoded(CASE, file)),
        None,
    );
    assert_eq!(
        view.outcome,
        ViewOutcome::ViewRefitRejected,
        "with no correction in force, the shifted landmarks refuse the frame"
    );
    (
        ViewCorrection::translation(
            correction_shift(&found, &first_layer(), print_z),
            view.agreeing(),
        ),
        found.shift_px,
    )
}

/// Compose one frame through a correction, at its own layer.
fn compose_through(
    calibration: &CameraCalibration,
    jpeg: &[u8],
    file: &str,
    correction: &ViewCorrection,
) -> Result<printobserver_panels::CheckImage, printobserver_panels::ComposeError> {
    let index = index(CASE);
    let position = recorded_position(CASE, file);
    let layer = index.layer_at(position).expect("a layer");
    let print_z = index.layer_z_mm(layer).expect("a height");
    let path = toolpath(CASE, layer_at_z(&index, print_z));
    compose_check_image(&CheckImageInputs {
        current_jpeg: jpeg,
        current_at: Timestamp::now(),
        camera_snapshot_url: &snapshot_url(),
        previous: None,
        outlines: index.object_outlines(),
        active_object: 0,
        toolpath: &path,
        object_layer_below: None,
        print_z_mm: print_z,
        calibration,
        correction: Some(correction),
    })
}

#[test]
fn a_later_frame_composes_through_the_first_layer_correction() {
    let calibration = calibration();
    let (correction, found) = correction_on(&calibration, "134401-periodic_observation.jpg");
    assert_eq!(correction.model, ViewModel::Translation);
    assert!(
        (found[0] - 4.0).hypot(found[1] + 20.0) <= 3.0,
        "13:44's search finds its measured shift, not {found:?}"
    );
    // 6.6 mm above the first layer, the search scores the box's body, so the
    // pass corrects by nothing: through that shift the outline floated over
    // the toolhead, off a healthy part.
    let [cx, cy] = correction.shift_px();
    assert!(
        cx.hypot(cy) < 1e-9,
        "a pass from a frame this far above its first layer corrects by nothing, not [{cx}, {cy}]"
    );
    for (name, expected) in AGREEING {
        let reference = correction
            .landmarks
            .iter()
            .find(|landmark| landmark.name == name)
            .unwrap_or_else(|| panic!("{name} is in the reference: {:?}", correction.landmarks));
        let off =
            (reference.offset_px[0] - expected[0]).hypot(reference.offset_px[1] - expected[1]);
        assert!(off <= 3.0, "{name} at {:?}", reference.offset_px);
    }
    assert!(
        (40.0..80.0).contains(&correction.residual_px),
        "the landmarks disagree with the part by {:.1} px",
        correction.residual_px
    );

    let later = "152804-periodic_observation.jpg";
    let view = judge_view(
        &calibration,
        &image::imageops::grayscale(&decoded(CASE, later)),
        Some(&correction),
    );
    assert_eq!(view.outcome, ViewOutcome::Refit, "{view:?}");
    let image = compose_through(&calibration, &frame(CASE, later), later, &correction)
        .expect("the later frame composes through the correction");
    assert_eq!(image.height, printobserver_panels::compose::IMAGE_HEIGHT);
}

/// A frame moved by `dx` pixels, its uncovered edge filled grey.
fn shifted(frame: &RgbImage, dx: u32) -> RgbImage {
    let mut moved = RgbImage::from_pixel(frame.width(), frame.height(), Rgb([90, 90, 90]));
    image::imageops::replace(&mut moved, frame, i64::from(dx), 0);
    moved
}

/// A frame with the named landmarks' regions painted flat — each one's patch
/// and the whole search around it — as something standing in front of them
/// would hide them.
fn painted_over(frame: &RgbImage, calibration: &CameraCalibration, names: &[&str]) -> RgbImage {
    let mut painted = frame.clone();
    for landmark in calibration
        .landmarks
        .iter()
        .filter(|landmark| names.contains(&landmark.name.as_str()))
    {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a landmark's centre is a pixel of the frame"
        )]
        let (u, v) = (
            landmark.center_px[0].round() as u32,
            landmark.center_px[1].round() as u32,
        );
        let reach = 40 + VIEW_SEARCH_RADIUS_PX + 100;
        for y in v.saturating_sub(reach)..(v + reach).min(frame.height()) {
            for x in u.saturating_sub(reach)..(u + reach).min(frame.width()) {
                painted.put_pixel(x, y, Rgb([128, 128, 128]));
            }
        }
    }
    painted
}

#[test]
fn after_a_pass_a_camera_moved_again_is_rejected_and_hidden_landmarks_are_not_found() {
    let calibration = calibration();
    let (correction, _) = correction_on(&calibration, "152603-filament_change_planned_resumed.jpg");
    let later = decoded(CASE, "152804-periodic_observation.jpg");
    let moved = judge_view(
        &calibration,
        &image::imageops::grayscale(&shifted(&later, 10)),
        Some(&correction),
    );
    assert_eq!(
        moved.outcome,
        ViewOutcome::ViewRefitRejected,
        "the reference landmarks moved a further 10 px: {moved:?}"
    );
    assert!(moved.reason.is_some());
    let names: Vec<&str> = AGREEING.iter().map(|(name, _)| *name).collect();
    let hidden = judge_view(
        &calibration,
        &image::imageops::grayscale(&painted_over(&later, &calibration, &names)),
        Some(&correction),
    );
    assert_eq!(
        hidden.outcome,
        ViewOutcome::LandmarksNotFound,
        "the reference landmarks are painted over: {hidden:?}"
    );
}

#[test]
fn the_wide_search_scores_no_window_above_one_and_finds_the_agreeing_pair_on_every_frame() {
    let calibration = calibration();
    let names: Vec<String> = calibration
        .landmarks
        .iter()
        .map(|landmark| landmark.name.clone())
        .collect();
    // One frame after another rather than a thread each: every thread of an
    // instrumented build counts its coverage in the same counters, and a
    // thread a frame turned a minute's search into a quarter of an hour of a
    // whole runner's cores.
    let mut failures: Vec<String> = Vec::new();
    for file in FRAMES {
        let grey = image::imageops::grayscale(&decoded(CASE, file));
        for (name, located) in names
            .iter()
            .zip(locate_landmarks_within(&calibration, &grey, 256))
        {
            if located.score > 1.0 + 1e-6 {
                failures.push(format!(
                    "{file}: {name} scored {} at radius 256",
                    located.score
                ));
            }
        }
        let found = locate_landmarks_within(&calibration, &grey, VIEW_SEARCH_RADIUS_PX);
        for (name, expected) in AGREEING {
            let at = names.iter().position(|held| held == name).expect("named");
            let located = found[at];
            let off =
                (located.offset_px[0] - expected[0]).hypot(located.offset_px[1] - expected[1]);
            if located.score < HIDDEN_BELOW || off > 3.0 {
                failures.push(format!(
                    "{file}: {name} found at {:?} scoring {} within {VIEW_SEARCH_RADIUS_PX} px",
                    located.offset_px, located.score
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
