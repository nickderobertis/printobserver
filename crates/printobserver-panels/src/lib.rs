//! `printobserver-panels`.
//!
//! Owns: what lets a print check show the agent the current layer's planned
//! toolpath where it sits in the camera's own view — one printer's camera
//! calibration file and its fisheye projection ([`calibration`]), the check
//! that the camera has not moved since ([`drift`]), the per-print check that
//! the projection lands on the printed first layer ([`alignment`]), and the
//! check image's three panels, their colours and their drawing
//! ([`compose`]).
//!
//! May depend on: `printobserver-types` and `printobserver-core`, whose
//! planned geometry — object outlines and a layer's toolpath in bed
//! millimetres ([`printobserver_core::toolpath`]) — it draws; the core alone
//! reads G-code, and this crate never sees a G-code line. It needs no
//! `OpenCV`. Never an implementation crate.

pub mod alignment;
pub mod calibration;
pub mod compose;
pub mod drift;
pub mod error;
mod geometry;
mod text;

pub use alignment::{AlignmentInputs, AlignmentVerdict, verify_first_layer_alignment};
pub use calibration::{
    BedPose, CalibrationError, CameraCalibration, Fit, Intrinsics, Landmark, Motion, SCHEMA_VERSION,
};
pub use compose::{CheckImage, CheckImageInputs, compose_check_image};
pub use error::{ComposeError, FrameField};
