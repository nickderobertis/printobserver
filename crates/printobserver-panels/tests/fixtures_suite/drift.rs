//! The drift check and the frame refusals, over recorded frames: a frame
//! from the calibrated camera composes; the same frame shifted past the
//! tolerance is a moved camera; one landmark hidden still composes, three
//! hidden is unverified; and a frame of another size or stream is refused,
//! by the composite and by the alignment check alike.

use image::{Rgb, RgbImage};
use printobserver_panels::{
    AlignmentInputs, CheckImageInputs, ComposeError, FrameField, compose_check_image,
    verify_first_layer_alignment,
};
use printobserver_types::Timestamp;

use crate::support::{
    calibration, decoded, frame, index, jpeg_of, layer_at_z, recorded_z, snapshot_url, toolpath,
};

/// The PETG jar top.
const PETG: &str = "jar-top-petg-healthy";

/// One of its frames.
const FRAME: &str = "agent-031-obs-125703-p43.2.jpg";

/// Compose a check of a frame's bytes at the PETG frame's layer.
fn compose(jpeg: &[u8], url: &str) -> Result<(), ComposeError> {
    compose_under(&calibration(), jpeg, url)
}

/// The same, under a calibration of the test's own.
fn compose_under(
    calibration: &printobserver_panels::CameraCalibration,
    jpeg: &[u8],
    url: &str,
) -> Result<(), ComposeError> {
    let index = index(PETG);
    let print_z = recorded_z(PETG, FRAME);
    let path = toolpath(PETG, layer_at_z(&index, print_z));
    compose_check_image(&CheckImageInputs {
        current_jpeg: jpeg,
        current_at: Timestamp::now(),
        camera_snapshot_url: url,
        previous: None,
        outlines: index.object_outlines(),
        active_object: 0,
        toolpath: &path,
        object_layer_below: None,
        print_z_mm: print_z,
        calibration,
    })
    .map(|_| ())
}

/// The committed calibration with its JSON edited.
fn edited(
    change: impl FnOnce(&mut printobserver_types::serde_json::Value),
) -> printobserver_panels::CameraCalibration {
    let mut value: printobserver_types::serde_json::Value =
        printobserver_types::serde_json::from_slice(&crate::support::calibration_bytes())
            .expect("JSON");
    change(&mut value);
    printobserver_panels::CameraCalibration::from_json(
        &printobserver_types::serde_json::to_vec(&value).expect("JSON"),
    )
    .expect("the edited calibration is valid")
}

/// A stored patch with no texture matches nothing, so its landmark counts as
/// hidden; and one landmark found displaced, with the rest where they were,
/// is no moved camera.
#[test]
fn a_flat_stored_patch_is_hidden_and_one_displaced_landmark_is_no_moved_camera() {
    let url = snapshot_url();
    let mut flat = Vec::new();
    image::GrayImage::from_pixel(48, 48, image::Luma([128]))
        .write_to(
            &mut std::io::Cursor::new(&mut flat),
            image::ImageFormat::Png,
        )
        .expect("a flat PNG");
    let flat = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &flat);
    let flattened = edited(|value| {
        for landmark in value["landmarks"]
            .as_array_mut()
            .expect("landmarks")
            .iter_mut()
            .take(3)
        {
            landmark["patch_png_base64"] = flat.clone().into();
        }
    });
    assert_eq!(
        compose_under(&flattened, &frame(PETG, FRAME), &url),
        Err(ComposeError::DriftUnverified)
    );
    let displaced = edited(|value| {
        let x = value["landmarks"][0]["center_px"][0].as_f64().expect("x");
        value["landmarks"][0]["center_px"][0] = (x - 20.0).into();
    });
    compose_under(&displaced, &frame(PETG, FRAME), &url)
        .expect("one landmark 20 px off, the rest in place, still composes");
}

/// A frame moved by `dx` pixels, its uncovered edge filled grey.
fn shifted(frame: &RgbImage, dx: u32) -> RgbImage {
    let mut moved = RgbImage::from_pixel(frame.width(), frame.height(), Rgb([90, 90, 90]));
    image::imageops::replace(&mut moved, frame, i64::from(dx), 0);
    moved
}

/// A frame with some landmarks' regions painted flat: each landmark's patch
/// and the search around it, as something standing in front of it would hide
/// it. Painting the patch alone leaves the texture around it, in which a
/// landmark can find a false match scoring just over the hidden cut.
fn hidden(frame: &RgbImage, count: usize) -> RgbImage {
    let mut painted = frame.clone();
    for landmark in calibration().landmarks.iter().take(count) {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a landmark's centre is a pixel of the frame"
        )]
        let (u, v) = (
            landmark.center_px[0].round() as u32,
            landmark.center_px[1].round() as u32,
        );
        let reach = 24 + landmark.search_radius_px;
        for y in v.saturating_sub(reach)..(v + reach).min(frame.height()) {
            for x in u.saturating_sub(reach)..(u + reach).min(frame.width()) {
                painted.put_pixel(x, y, Rgb([128, 128, 128]));
            }
        }
    }
    painted
}

#[test]
fn a_recorded_frame_composes_and_a_moved_camera_is_refused() {
    let url = snapshot_url();
    compose(&frame(PETG, FRAME), &url).expect("the recorded frame composes");
    let tolerance = calibration().drift_tolerance_px;
    let original = decoded(PETG, FRAME);
    match compose(&jpeg_of(&shifted(&original, 12)), &url) {
        Err(ComposeError::CameraMoved {
            landmark,
            offset_px,
        }) => {
            assert!(!landmark.is_empty());
            assert!(offset_px > tolerance, "{landmark} moved {offset_px:.1} px");
        }
        other => panic!("a frame moved 12 px is a moved camera, not {other:?}"),
    }
}

#[test]
fn one_hidden_landmark_still_composes_and_three_are_unverified() {
    let url = snapshot_url();
    let original = decoded(PETG, FRAME);
    compose(&jpeg_of(&hidden(&original, 1)), &url).expect("one landmark hidden still composes");
    assert_eq!(
        compose(&jpeg_of(&hidden(&original, 3)), &url),
        Err(ComposeError::DriftUnverified)
    );
}

#[test]
fn a_frame_of_another_resolution_or_stream_is_refused_alike() {
    let calibration = calibration();
    let url = snapshot_url();
    let small = frame("fan-cut-bridge", "01-healthy-before-fault-p41.jpg");
    let full = frame(PETG, FRAME);
    let other = "http://127.0.0.1:1984/api/frame.jpeg?src=another";
    let first = toolpath(
        PETG,
        index(PETG).first_object_layer().expect("a first layer").0,
    );
    let align = |jpeg: &[u8], url: &str| {
        verify_first_layer_alignment(&AlignmentInputs {
            frame_jpeg: jpeg,
            camera_snapshot_url: url,
            first_layer: &first,
            print_z_mm: 0.6,
            calibration: &calibration,
        })
        .map(|_| ())
    };
    for (what, refused) in [
        ("the composite", compose(&small, &url)),
        ("the alignment check", align(&small, &url)),
    ] {
        assert_eq!(
            refused,
            Err(ComposeError::FrameMismatch {
                field: FrameField::Resolution,
                expected: "1920x1080".to_owned(),
                got: "1280x720".to_owned(),
            }),
            "{what} refuses a 1280x720 frame"
        );
    }
    for (what, refused) in [
        ("the composite", compose(&full, other)),
        ("the alignment check", align(&full, other)),
    ] {
        assert_eq!(
            refused,
            Err(ComposeError::FrameMismatch {
                field: FrameField::Stream,
                expected: url.clone(),
                got: other.to_owned(),
            }),
            "{what} refuses another stream's frame"
        );
    }
}
