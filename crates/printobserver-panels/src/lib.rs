//! What lets a print check show the agent the current layer's planned
//! toolpath where it sits in the camera's own view: one printer's camera
//! calibration and its projection, the check that the camera has not moved
//! since, the per-print check that the projection lands on the printed first
//! layer, and the check image's three panels.
//!
//! The geometry comes from the core, which alone reads G-code: object
//! outlines and a layer's toolpath as bed millimetres
//! ([`printobserver_core::toolpath`]). This crate never sees a G-code line,
//! and needs no `OpenCV`.

pub mod alignment;
pub mod calibration;
pub mod compose;
pub mod drift;
pub mod error;
mod geometry;

pub use alignment::{AlignmentInputs, AlignmentVerdict, verify_first_layer_alignment};
pub use calibration::{
    BedPose, CalibrationError, CameraCalibration, Fit, Intrinsics, Landmark, Motion, SCHEMA_VERSION,
};
pub use compose::{CheckImage, CheckImageInputs, compose_check_image};
pub use error::{ComposeError, FrameField};
