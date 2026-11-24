//! The growth measurement over recorded real-print frames.
//!
//! The air print's frame at Z 5.8, measured against its check frame at Z 0.8,
//! finds the parts carried down with the bed: extrusion had stopped near Z 2
//! and the printer printed air. Every frame of the two healthy recorded prints,
//! measured against the newest earlier frame at least the least rise below it,
//! finds no such thing. The same air-print pair told that the plan rose less
//! than separates the two hypotheses draws no verdict, and a frame that is not
//! the calibration's is refused. Every assertion states what was measured.

use printobserver_panels::growth::{MIN_RISE_MM, MIN_SEPARATION_PX};
use printobserver_panels::{ComposeError, FrameField, Growth, GrowthInputs, measure_growth};

use crate::support::{calibration_of, frame, index, layer_at_z, recorded_z, snapshot_url};

/// The air print, its check frame at Z 0.8 and its frame at Z 5.8.
const AIR_PRINT: (&str, &str, &str) = (
    "petg-spool-jam-air-print",
    "frame-144430-p01.4.jpg",
    "frame-162654-p09.8.jpg",
);

/// The healthy prints' frames that record a height, in capture order.
const HEALTHY: [(&str, &[&str]); 2] = [
    (
        "jar-top-petg-healthy",
        &[
            "agent-007-obs-121230-p01.3.jpg",
            "agent-018-obs-123246-p24.5.jpg",
            "agent-027-obs-124858-p38.9.jpg",
            "agent-028-obs-125100-p39.7.jpg",
            "agent-031-obs-125703-p43.2.jpg",
            "agent-049-obs-133131-p59.0.jpg",
            "agent-059-obs-135146-p74.2.jpg",
            "agent-069-obs-141200-p88.4.jpg",
            "agent-074-obs-142205-p97.0.jpg",
        ],
    ),
    (
        "bubbler-guard-and-pad-pla-healthy",
        &[
            "agent-003-obs-165123-p00.1.jpg",
            "agent-005-obs-165325-p01.7.jpg",
            "agent-016-obs-170731-p13.8.jpg",
            "agent-048-obs-180412-p65.7.jpg",
            "agent-067-obs-184042-p89.2.jpg",
            "agent-086-obs-191910-p98.6.jpg",
        ],
    ),
];

/// The measurement of `current` against `earlier`, told the print stood at
/// `earlier_z` then, read at the layer `current` was recorded at.
fn measured(
    case: &str,
    earlier: &str,
    earlier_z: f64,
    current: &[u8],
    current_z: f64,
) -> Result<Option<Growth>, ComposeError> {
    let index = index(case);
    let toolpath = index
        .layer_toolpath(layer_at_z(&index, current_z))
        .expect("the layer reads back from the spool");
    measure_growth(&GrowthInputs {
        earlier_jpeg: &frame(case, earlier),
        earlier_print_z_mm: earlier_z,
        current_jpeg: current,
        current_print_z_mm: current_z,
        camera_snapshot_url: &snapshot_url(),
        toolpath: &toolpath,
        calibration: &calibration_of(case),
    })
}

/// Parts that stopped growing while the printer climbed are measured as not
/// grown.
#[test]
fn the_air_prints_parts_are_measured_carried_down_with_the_bed() {
    let (case, earlier, current) = AIR_PRINT;
    let growth = measured(
        case,
        earlier,
        recorded_z(case, earlier),
        &frame(case, current),
        recorded_z(case, current),
    )
    .expect("both frames are the calibration's")
    .expect("enough samples compare");
    assert!(
        growth.not_grown(),
        "the air print's parts are not grown: {growth:?}"
    );
    assert!(
        growth.carried > growth.held,
        "carried down correlates better than held: {growth:?}"
    );
}

/// No frame of a print growing as planned is measured as not grown.
#[test]
fn no_healthy_frame_is_measured_as_not_grown() {
    let mut compared = 0;
    for (case, files) in HEALTHY {
        for (at, current) in files.iter().enumerate() {
            let current_z = recorded_z(case, current);
            let Some(earlier) = files[..at]
                .iter()
                .rev()
                .find(|earlier| recorded_z(case, earlier) <= current_z - MIN_RISE_MM)
            else {
                continue;
            };
            let growth = measured(
                case,
                earlier,
                recorded_z(case, earlier),
                &frame(case, current),
                current_z,
            )
            .expect("both frames are the calibration's");
            assert!(
                !growth.is_some_and(|growth| growth.not_grown()),
                "{case}: {current} against {earlier} is not measured as not grown: {growth:?}"
            );
            compared += 1;
        }
    }
    assert!(
        compared >= 10,
        "the healthy prints give at least ten pairs, not {compared}"
    );
}

/// A rise too small to move a carried part clear of where a held one sits
/// draws no verdict, however alike the frames are.
#[test]
fn a_rise_too_small_to_separate_draws_no_verdict() {
    let (case, earlier, current) = AIR_PRINT;
    let current_z = recorded_z(case, current);
    let growth = measured(
        case,
        earlier,
        current_z - 1.0,
        &frame(case, current),
        current_z,
    )
    .expect("both frames are the calibration's")
    .expect("enough samples compare");
    assert!(
        growth.separation_px < MIN_SEPARATION_PX && !growth.not_grown(),
        "a 1 mm rise separates the hypotheses by under {MIN_SEPARATION_PX} px and draws no verdict: {growth:?}"
    );
}

/// A frame that is not the calibration's size is refused, never measured.
#[test]
fn a_frame_of_another_size_is_refused() {
    let (case, earlier, _) = AIR_PRINT;
    let other = frame("jar-top-petg-healthy", "agent-018-obs-123246-p24.5.jpg");
    let refused = measured(case, earlier, 0.8, &other, 5.8);
    assert!(
        matches!(
            refused,
            Err(ComposeError::FrameMismatch {
                field: FrameField::Resolution,
                ..
            })
        ),
        "a 1920x1080 frame against the 1280x720 calibration is refused: {refused:?}"
    );
}
