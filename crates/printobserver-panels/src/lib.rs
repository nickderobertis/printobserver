//! `printobserver-panels`.
//!
//! Owns: what lets a print check show the agent the current layer's planned
//! toolpath where it sits in the camera's own view — one printer's camera
//! calibration file and its fisheye projection ([`calibration`]), the check
//! that the camera has not moved since ([`drift`]), the per-print check that
//! the projection lands on the printed first layer ([`alignment`]), and the
//! check image's three panels, their colours and their drawing
//! ([`compose`]), whether the view still matches the print's correction
//! ([`view`]), the measurement of whether the parts grew since an earlier
//! frame ([`growth`]), and the whole camera frame scaled to a check's bound
//! ([`whole_frame`]).
//!
//! May depend on: `printobserver-types`; `printobserver-vision-api`, whose
//! validated web address the calibration's snapshot URL is held to; and
//! `printobserver-core`, whose
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
pub mod growth;
mod text;
pub mod view;
pub mod whole_frame;

pub use alignment::{
    AlignmentInputs, AlignmentVerdict, FirstLayerInputs, FirstLayerJudgement, FirstLayerVerdict,
    InconclusiveReason, judge_first_layer, verify_first_layer_alignment,
};
pub use calibration::{
    BedPose, CalibrationError, CameraCalibration, Fit, Intrinsics, Landmark, Motion, SCHEMA_VERSION,
};
pub use compose::{CheckImage, CheckImageInputs, compose_check_image, is_drawn};
pub use error::{ComposeError, FrameField};
pub use growth::{Growth, GrowthInputs, measure_growth};
pub use view::{
    ReferenceLandmark, ViewCorrection, ViewJudgement, ViewModel, ViewOutcome, judge_view,
};
pub use whole_frame::{WHOLE_FRAME_HEIGHT, WHOLE_FRAME_WIDTH, WholeFrame, whole_frame};
