//! The calibration file's schema is generated from its one Rust type, and so
//! are the shapes of the view correction the server records, each in a file of
//! its own named for its type.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes `schemas/printobserver-panels/camera-calibration.json`, and without
//! it refuses a tree whose checked-in schema no longer matches the type.
//! `tools/camera-calibration`'s tests validate what that tool writes against
//! the same file.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_panels::{
    CameraCalibration, InconclusiveReason, ReferenceLandmark, ViewCorrection, ViewModel,
    ViewOutcome,
};
use printobserver_types::contract::schema_of;
use printobserver_types::schemars::JsonSchema;

/// One type's schema, under the file named for it.
fn named<T: JsonSchema>() -> (String, printobserver_types::serde_json::Value) {
    (format!("{}.json", T::schema_name()), schema_of::<T>())
}

#[test]
fn the_calibration_schema_is_what_the_type_generates() {
    let findings = schema_files::reconcile(
        "printobserver-panels",
        &[
            (
                "camera-calibration.json".to_owned(),
                schema_of::<CameraCalibration>(),
            ),
            named::<ViewCorrection>(),
            named::<ViewModel>(),
            named::<ReferenceLandmark>(),
            named::<ViewOutcome>(),
            named::<InconclusiveReason>(),
        ],
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
