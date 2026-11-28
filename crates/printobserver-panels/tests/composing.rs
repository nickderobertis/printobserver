//! The check image's refusals and edges, on one recorded frame and planned
//! geometry written here: a frame that is no JPEG, geometry landing nowhere in
//! the frame, an active object off the frame, a previous frame of another
//! resolution, and every refusal's words.

use printobserver_core::{ExtrusionKind, LayerToolpath, ObjectOutline, ToolpathSegment};
use printobserver_panels::compose::{IMAGE_HEIGHT, PANEL_WIDTH};
use printobserver_panels::{
    AlignmentInputs, CameraCalibration, CheckImageInputs, ComposeError, FrameField,
    compose_check_image, verify_first_layer_alignment,
};
use printobserver_types::Timestamp;

/// The committed calibration.
fn calibration() -> CameraCalibration {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/real-prints/cameras/core-one-2026-09-27.json"
    );
    CameraCalibration::from_json(&std::fs::read(path).expect("the calibration")).expect("valid")
}

/// A recorded frame from the calibrated camera.
fn frame() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/frames/jar-top-petg-healthy/agent-031-obs-125703-p43.2.jpg"
    ))
    .expect("the recorded frame")
}

/// A square of one kind around a bed point, at a height.
fn square(at: [f32; 2], z_mm: f32, kind: ExtrusionKind) -> LayerToolpath {
    let corners = [
        [at[0] - 10.0, at[1] - 10.0],
        [at[0] + 10.0, at[1] - 10.0],
        [at[0] + 10.0, at[1] + 10.0],
        [at[0] - 10.0, at[1] + 10.0],
    ];
    LayerToolpath {
        z_mm,
        segments: (0..4)
            .map(|k| ToolpathSegment {
                from_mm: corners[k],
                to_mm: corners[(k + 1) % 4],
                kind,
            })
            .collect(),
    }
}

/// The outline of a square around a bed point.
fn outline(at: [f32; 2]) -> ObjectOutline {
    ObjectOutline {
        hull_mm: square(at, 0.0, ExtrusionKind::Wall)
            .segments
            .iter()
            .map(|segment| segment.from_mm)
            .collect(),
    }
}

/// Compose a check of some geometry over a frame.
fn compose(
    current: &[u8],
    previous: Option<&[u8]>,
    toolpath: &LayerToolpath,
    outlines: &[ObjectOutline],
) -> Result<(u32, u32, [u32; 4]), ComposeError> {
    let calibration = calibration();
    let url = calibration.camera_snapshot_url.clone();
    compose_check_image(&CheckImageInputs {
        current_jpeg: current,
        current_at: Timestamp::now(),
        camera_snapshot_url: &url,
        previous: previous.map(|jpeg| (jpeg, Timestamp::now())),
        outlines,
        active_object: 0,
        toolpath,
        object_layer_below: None,
        print_z_mm: 10.6,
        calibration: &calibration,
        correction: None,
    })
    .map(|image| (image.width, image.height, image.close_up_source_rect))
}

#[test]
fn a_frame_that_is_no_jpeg_is_refused() {
    let path = square([125.0, 110.0], 10.6, ExtrusionKind::Other);
    let outlines = [outline([125.0, 110.0])];
    assert!(matches!(
        compose(b"not a jpeg", None, &path, &outlines),
        Err(ComposeError::Undecodable { .. })
    ));
    assert!(matches!(
        compose(&frame(), Some(b"not a jpeg"), &path, &outlines),
        Err(ComposeError::Undecodable { .. })
    ));
}

#[test]
fn a_previous_frame_of_another_resolution_is_refused() {
    let low = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/frames/fan-cut-bridge/01-healthy-before-fault-p41.jpg"
    ))
    .expect("the 1280x720 recorded frame");
    let path = square([125.0, 110.0], 10.6, ExtrusionKind::Other);
    let outlines = [outline([125.0, 110.0])];
    assert_eq!(
        compose(&frame(), Some(&low), &path, &outlines),
        Err(ComposeError::FrameMismatch {
            field: FrameField::Resolution,
            expected: "1920x1080".to_owned(),
            got: "1280x720".to_owned(),
        })
    );
}

#[test]
fn geometry_landing_nowhere_in_the_frame_is_refused() {
    // The camera sits at the bed's front looking back, so a point far in
    // front of the printer is behind it.
    let far = square([125.0, -5_000.0], 10.6, ExtrusionKind::Wall);
    assert_eq!(
        compose(&frame(), None, &far, &[outline([125.0, -5_000.0])]),
        Err(ComposeError::OutlineOffFrame)
    );
    let calibration = calibration();
    assert_eq!(
        verify_first_layer_alignment(&AlignmentInputs {
            frame_jpeg: &frame(),
            camera_snapshot_url: &calibration.camera_snapshot_url,
            first_layer: &far,
            print_z_mm: 0.4,
            calibration: &calibration,
        }),
        Err(ComposeError::OutlineOffFrame)
    );
}

#[test]
fn an_active_object_off_the_frame_closes_up_on_the_whole_frame() {
    let path = square([125.0, 110.0], 10.6, ExtrusionKind::Other);
    let (width, height, rect) = compose(&frame(), None, &path, &[outline([125.0, -5_000.0])])
        .expect("the toolpath lands in the frame");
    assert_eq!((width, height), (PANEL_WIDTH, IMAGE_HEIGHT));
    assert_eq!(rect, [0, 0, 1920, 1080]);
    let none = compose(&frame(), None, &path, &[]).expect("no outline at all");
    assert_eq!(none.2, [0, 0, 1920, 1080]);
}

#[test]
fn every_refusal_says_what_was_refused() {
    let cases = [
        (
            ComposeError::FrameMismatch {
                field: FrameField::Resolution,
                expected: "1920x1080".to_owned(),
                got: "1280x720".to_owned(),
            },
            "resolution is 1280x720",
        ),
        (
            ComposeError::FrameMismatch {
                field: FrameField::Stream,
                expected: "http://a".to_owned(),
                got: "http://b".to_owned(),
            },
            "stream is http://b",
        ),
        (
            ComposeError::CameraMoved {
                landmark: "rail-end-right".to_owned(),
                offset_px: 12.25,
            },
            "rail-end-right is 12.2 px",
        ),
        (ComposeError::DriftUnverified, "fewer than two"),
        (ComposeError::OutlineOffFrame, "lands in the frame"),
        (
            ComposeError::Undecodable {
                detail: "truncated".to_owned(),
            },
            "not a JPEG: truncated",
        ),
        (
            ComposeError::Encode {
                detail: "full".to_owned(),
            },
            "check image: full",
        ),
    ];
    for (error, said) in cases {
        let shown = error.to_string();
        assert!(shown.contains(said), "{shown:?} says {said:?}");
    }
    assert_eq!(FrameField::Resolution.as_str(), "resolution");
    assert_eq!(FrameField::Stream.as_str(), "stream");
}
