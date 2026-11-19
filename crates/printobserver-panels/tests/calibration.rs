//! A calibration file is read as the service reads it at start: the
//! committed one is accepted and projects as the calibration tool's own model
//! does, and each kind of invalid file is refused naming its key.

use printobserver_panels::{CalibrationError, CameraCalibration};
use printobserver_types::serde_json::{self, Value};

/// The committed calibration's text.
fn committed() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/real-prints/cameras/core-one-2026-09-27.json"
    );
    serde_json::from_slice(&std::fs::read(path).expect("the committed calibration")).expect("JSON")
}

/// Read a calibration from a JSON value.
fn read(value: &Value) -> Result<CameraCalibration, CalibrationError> {
    CameraCalibration::from_json(&serde_json::to_vec(value).expect("serialises"))
}

/// The key a refusal names.
fn refused_field(value: &Value) -> &'static str {
    match read(value) {
        Err(CalibrationError::Invalid { field, .. }) => field,
        other => panic!("refused naming a key, not {other:?}"),
    }
}

/// The committed file projects bed points where the calibration tool's model
/// — itself held to `OpenCV`'s fisheye projection — puts them: those pixels
/// were computed by `tools/camera-calibration`'s `Camera.project` over this
/// file.
#[test]
fn the_committed_calibration_projects_as_the_tools_model_does() {
    let calibration = read(&committed()).expect("the committed calibration is valid");
    let size = [100_000, 100_000];
    for (point, print_z, expected) in [
        (
            [125.0, 110.0, 0.0],
            0.0,
            [1_035.040_224_803_941, 155.061_451_433_186_33],
        ),
        (
            [20.0, 20.0, 0.0],
            0.0,
            [1_142.275_201_480_112_6, 926.853_983_820_626_7],
        ),
        (
            [125.0, 110.0, 0.0],
            40.0,
            [1_022.812_763_804_729, 292.056_083_782_561_36],
        ),
    ] {
        let got = calibration.project(&[point], print_z, size)[0].expect("in view");
        assert!(
            (got[0] - expected[0]).abs() < 1e-6 && (got[1] - expected[1]).abs() < 1e-6,
            "{point:?} at {print_z} mm projects to {got:?}, the tool's model to {expected:?}"
        );
    }
    let frame = calibration.image_size_px;
    assert_eq!(
        calibration.project(&[[230.0, 200.0, 0.0]], 0.0, frame),
        vec![None],
        "a point projecting above the frame is outside it"
    );
    assert_eq!(
        calibration.project(&[[125.0, 110.0, 0.0]], -1000.0, frame),
        vec![None],
        "a point behind the camera has no pixel"
    );
}

/// Each kind of invalid file is refused, naming the key that holds it.
#[test]
fn an_invalid_calibration_is_refused_naming_its_key() {
    let mut cases: Vec<(&str, Value)> = Vec::new();
    let mut edit = |name: &'static str, change: fn(&mut Value)| {
        let mut value = committed();
        change(&mut value);
        cases.push((name, value));
    };
    edit("schema_version", |v| v["schema_version"] = 2.into());
    edit("image_size_px", |v| {
        v["image_size_px"] = serde_json::json!([0, 1080]);
    });
    edit("intrinsics.fx", |v| v["intrinsics"]["fx"] = (-1.0).into());
    edit("drift_tolerance_px", |v| v["drift_tolerance_px"] = 0.into());
    edit("alignment_max_offset_px", |v| {
        v["alignment_max_offset_px"] = (-3.0).into();
    });
    edit("camera_snapshot_url", |v| {
        v["camera_snapshot_url"] = "frame.jpeg".into();
    });
    edit("camera_snapshot_url", |v| {
        v["camera_snapshot_url"] = "http://".into();
    });
    edit("landmarks.center_px", |v| {
        v["landmarks"][0]["center_px"] = printobserver_types::serde_json::json!([1e300, 5.0]);
    });
    edit("fit.calibrated_at", |v| {
        v["fit"]["calibrated_at"] = "27/09/2026".into();
    });
    edit("landmarks.search_radius_px", |v| {
        v["landmarks"][0]["search_radius_px"] = 100_000.into();
    });
    edit("landmarks.search_radius_px", |v| {
        v["landmarks"][0]["search_radius_px"] = 0.into();
    });
    edit("landmarks.patch_png_base64", |v| {
        v["landmarks"][0]["patch_png_base64"] = "bm90IGEgcG5n".into();
    });
    edit("landmarks.patch_png_base64", |v| {
        v["landmarks"][0]["patch_png_base64"] = "%%".into();
    });
    for (field, value) in &cases {
        assert_eq!(refused_field(value), *field);
    }
    let mut tiny = committed();
    let mut png = Vec::new();
    image::GrayImage::new(1, 1)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("a one-pixel PNG");
    tiny["landmarks"][0]["patch_png_base64"] =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png).into();
    assert_eq!(
        refused_field(&tiny),
        "landmarks.patch_png_base64",
        "a one-pixel patch"
    );
    let mut unknown = committed();
    unknown["extra"] = 1.into();
    assert!(matches!(
        read(&unknown),
        Err(CalibrationError::Shape { .. })
    ));
    assert!(matches!(
        CameraCalibration::from_json(b"not json"),
        Err(CalibrationError::Shape { .. })
    ));
    let shown = read(&cases[0].1).expect_err("refused").to_string();
    assert!(shown.contains("schema_version"), "{shown}");
}
