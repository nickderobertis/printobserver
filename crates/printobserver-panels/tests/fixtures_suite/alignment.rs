//! The first-layer alignment check over the four verified 1920x1080 cases:
//! the first recorded frame past each case's first object layer, chosen by
//! its recorded height, passes; each case's first frame, an empty bed, fails;
//! and each passing frame given a bed pose moved so the first layer's
//! projection moves 49 px — just past the smallest failed projection
//! measured, 48.7 px — fails. Every assertion states the offset it observed
//! beside the threshold.

use printobserver_core::LayerToolpath;
use printobserver_panels::alignment::{CORRECTION_TAKEN_WITHIN_MM, correction_shift};
use printobserver_panels::{
    AlignmentInputs, AlignmentVerdict, BedPose, CameraCalibration, FirstLayerInputs,
    FirstLayerVerdict, InconclusiveReason, judge_first_layer, verify_first_layer_alignment,
};

use crate::support::{calibration, frame, index, recorded_z, snapshot_url, toolpath};

/// Each case, its first frame past the first object layer, and its empty bed.
const CASES: [(&str, &str, &str); 4] = [
    (
        "jar-top-wrong-material-first-layer-detach",
        "agent-007-obs-182248-p00.3.jpg",
        "agent-001-obs-181445-p00.0.jpg",
    ),
    (
        "jar-top-wrong-material-detach-spaghetti",
        "agent-006-obs-191658-p04.3.jpg",
        "agent-001-obs-190853-p00.0.jpg",
    ),
    (
        "jar-top-petg-healthy",
        "agent-007-obs-121230-p01.3.jpg",
        "agent-001-obs-120024-p00.0.jpg",
    ),
    (
        "bubbler-guard-and-pad-pla-healthy",
        "agent-005-obs-165325-p01.7.jpg",
        "agent-001-obs-164721-p00.0.jpg",
    ),
];

/// The check of one frame.
fn verdict(
    calibration: &CameraCalibration,
    jpeg: &[u8],
    first: &LayerToolpath,
    print_z: f64,
) -> AlignmentVerdict {
    verify_first_layer_alignment(&AlignmentInputs {
        frame_jpeg: jpeg,
        camera_snapshot_url: &snapshot_url(),
        first_layer: first,
        print_z_mm: print_z,
        calibration,
    })
    .expect("the check rules on a calibrated frame")
}

/// The first-layer judgement of one frame, the nozzle placed nowhere: the
/// controls test the search's confidence rule, not the coverage test.
fn judgement(
    calibration: &CameraCalibration,
    jpeg: &[u8],
    first: &LayerToolpath,
    print_z: f64,
) -> FirstLayerVerdict {
    judge_first_layer(&FirstLayerInputs {
        alignment: AlignmentInputs {
            frame_jpeg: jpeg,
            camera_snapshot_url: &snapshot_url(),
            first_layer: first,
            print_z_mm: print_z,
            calibration,
        },
        nozzle_xy_mm: None,
    })
    .expect("the check rules on a calibrated frame")
    .verdict
}

/// How far, on average, a first layer's projection moves between two
/// calibrations, in pixels.
fn moved_px(
    a: &CameraCalibration,
    b: &CameraCalibration,
    first: &LayerToolpath,
    print_z: f64,
) -> f64 {
    let points: Vec<[f64; 3]> = first
        .segments
        .iter()
        .step_by(7)
        .map(|segment| {
            [
                f64::from(segment.from_mm[0]),
                f64::from(segment.from_mm[1]),
                f64::from(first.z_mm),
            ]
        })
        .collect();
    let size = [100_000, 100_000];
    let (pa, pb) = (
        a.project(&points, print_z, size),
        b.project(&points, print_z, size),
    );
    let moves: Vec<f64> = pa
        .iter()
        .zip(&pb)
        .filter_map(|(p, q)| Some((p.as_ref()?, q.as_ref()?)))
        .map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]))
        .collect();
    let count = f64::from(u32::try_from(moves.len()).expect("a count"));
    moves.iter().sum::<f64>() / count
}

/// The calibration with its bed moved along the camera's X until the first
/// layer's projection moves `target` pixels.
fn moved_by(
    calibration: &CameraCalibration,
    first: &LayerToolpath,
    print_z: f64,
    target: f64,
) -> CameraCalibration {
    let (mut low, mut high) = (0.0, 40.0);
    let at = |shift: f64| {
        let mut translation = calibration.bed_pose.translation_mm;
        translation[0] += shift;
        calibration.with_bed_pose(BedPose {
            rotation_rodrigues: calibration.bed_pose.rotation_rodrigues,
            translation_mm: translation,
        })
    };
    for _ in 0..60 {
        let middle = f64::midpoint(low, high);
        if moved_px(calibration, &at(middle), first, print_z) < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    at(low)
}

#[test]
fn a_healthy_first_layer_passes_an_empty_bed_and_a_moved_projection_fail() {
    let calibration = calibration();
    let threshold = calibration.alignment_max_offset_px;
    let mut observed = Vec::new();
    let mut corrected_by_their_shift = 0;
    for (case, past, empty) in CASES {
        let index = index(case);
        let first = toolpath(
            case,
            index.first_object_layer().expect("a first object layer").0,
        );
        let print_z = recorded_z(case, past);
        assert!(
            print_z > f64::from(first.z_mm),
            "{case}/{past} is past the first layer"
        );

        let healthy = verdict(&calibration, &frame(case, past), &first, print_z);
        observed.push(format!(
            "{case}: healthy {:.1} px at contrast {:.1}",
            healthy.offset_px, healthy.match_strength
        ));
        assert!(
            healthy.passed && healthy.offset_px <= threshold,
            "{case}/{past}: offset {:.1} px against a threshold of {threshold} px; it should pass",
            healthy.offset_px
        );
        assert_eq!(
            judgement(&calibration, &frame(case, past), &first, print_z),
            FirstLayerVerdict::Passed
        );
        let shift = correction_shift(&healthy, &first, print_z);
        let within = print_z - f64::from(first.z_mm) <= CORRECTION_TAKEN_WITHIN_MM + 1e-6;
        let expected = if within { healthy.shift_px } else { [0.0, 0.0] };
        assert!(
            (shift[0] - expected[0]).hypot(shift[1] - expected[1]) < 1e-9,
            "{case}: a pass {:.1} mm above its first layer corrects by {expected:?}, not {shift:?}",
            print_z - f64::from(first.z_mm)
        );
        if within {
            corrected_by_their_shift += 1;
        }

        let bare = verdict(&calibration, &frame(case, empty), &first, print_z);
        observed.push(format!("{case}: empty bed {:.1} px", bare.offset_px));
        assert!(
            !bare.passed,
            "{case}/{empty} (an empty bed): offset {:.1} px against a threshold of {threshold} px; it should fail",
            bare.offset_px
        );
        assert_eq!(
            judgement(&calibration, &frame(case, empty), &first, print_z),
            FirstLayerVerdict::Inconclusive(InconclusiveReason::NotClearlyFound),
            "{case}/{empty}: an empty bed is not clearly found"
        );

        let wrong = moved_by(&calibration, &first, print_z, 49.0);
        let moved = moved_px(&calibration, &wrong, &first, print_z);
        assert!(
            (moved - 49.0).abs() <= 2.0,
            "the projection moved {moved:.1} px"
        );
        let off = verdict(&wrong, &frame(case, past), &first, print_z);
        observed.push(format!(
            "{case}: moved {moved:.1} px, offset {:.1} px",
            off.offset_px
        ));
        assert!(
            !off.passed,
            "{case}/{past} with the projection moved {moved:.1} px: offset {:.1} px against a threshold of {threshold} px; it should fail",
            off.offset_px
        );
        assert_eq!(
            judgement(&wrong, &frame(case, past), &first, print_z),
            FirstLayerVerdict::Inconclusive(InconclusiveReason::NotClearlyFound),
            "{case}/{past}: a projection moved {moved:.1} px is not clearly found"
        );
    }
    assert!(
        corrected_by_their_shift > 0,
        "no healthy first layer was judged within one layer of itself: {observed:#?}"
    );
    eprintln!("{observed:#?}");
}

/// Each verified healthy first layer is larger than the toolhead can hide, so
/// it is judged on the image score alone and passes as it always did, with the
/// plan's nozzle where it was — though that nozzle lies within the coverage
/// margin of the layer's projected hull, which on the small box would call it
/// covered.
#[test]
fn a_first_layer_larger_than_the_toolhead_hides_is_judged_on_the_image_score() {
    use printobserver_panels::alignment::{
        TOOLHEAD_HIDES_MM, TOOLHEAD_MARGIN_PX, first_layer_span_mm, nozzle_from_first_layer_px,
    };
    let calibration = calibration();
    let box_index = index("two-colour-planned-change");
    let box_first = toolpath(
        "two-colour-planned-change",
        box_index.first_object_layer().expect("a first layer").0,
    );
    let box_span = first_layer_span_mm(&box_first);
    assert!(
        box_span <= TOOLHEAD_HIDES_MM,
        "the box's first layer spans {box_span:.1} mm, which the toolhead can hide"
    );
    let mut observed = Vec::new();
    let mut within_the_margin = 0;
    for (case, past, _) in CASES {
        let index = index(case);
        let (layer, ends) = index.first_object_layer().expect("a first object layer");
        let first = toolpath(case, layer);
        let span = first_layer_span_mm(&first);
        assert!(
            span > TOOLHEAD_HIDES_MM,
            "{case}: its first layer spans {span:.1} mm"
        );
        let print_z = recorded_z(case, past);
        // Where the plan had the nozzle as the frame was taken, or, for a
        // frame recording no position, as the first layer ended.
        let position = crate::support::recorded_position_if_any(case, past).unwrap_or(ends + 1);
        let nozzle = index
            .planned_head_at(position)
            .expect("the spool reads back");
        let jpeg = frame(case, past);
        let url = snapshot_url();
        let inputs = FirstLayerInputs {
            alignment: AlignmentInputs {
                frame_jpeg: &jpeg,
                camera_snapshot_url: &url,
                first_layer: &first,
                print_z_mm: print_z,
                calibration: &calibration,
            },
            nozzle_xy_mm: nozzle,
        };
        let distance = nozzle_from_first_layer_px(&inputs).expect("both project");
        if distance <= TOOLHEAD_MARGIN_PX {
            within_the_margin += 1;
        }
        let judged = judge_first_layer(&inputs).expect("the check rules on a calibrated frame");
        observed.push(format!(
            "{case}: {span:.1} mm, nozzle {distance:.1} px from the layer, {:?}",
            judged.verdict
        ));
        assert_eq!(
            judged.verdict,
            FirstLayerVerdict::Passed,
            "{case}/{past}: {observed:#?}"
        );
    }
    assert!(
        within_the_margin > 0,
        "no healthy first layer had its nozzle within the margin, so none shows the gate: {observed:#?}"
    );
    eprintln!("{observed:#?}");
}
