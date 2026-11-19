//! The calibration file's schema is generated from its one Rust type.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes `schemas/printobserver-panels/camera-calibration.json`, and without
//! it refuses a tree whose checked-in schema no longer matches the type.
//! `tools/camera-calibration`'s tests validate what that tool writes against
//! the same file.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_panels::CameraCalibration;
use printobserver_types::contract::schema_of;

#[test]
fn the_calibration_schema_is_what_the_type_generates() {
    let findings = schema_files::reconcile(
        "printobserver-panels",
        &[(
            "camera-calibration.json".to_owned(),
            schema_of::<CameraCalibration>(),
        )],
    );
    assert!(findings.is_empty(), "{findings:#?}");
    let schema = schema_of::<CameraCalibration>();
    let version = &schema["properties"]["schema_version"];
    let pinned = u64::from(printobserver_panels::SCHEMA_VERSION);
    assert_eq!(
        (version["minimum"].as_u64(), version["maximum"].as_u64()),
        (Some(pinned), Some(pinned)),
        "the schema pins the one version the reader takes, so a file of another fails validation"
    );
}
