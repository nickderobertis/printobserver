//! Which image a check or a look was shown, and why it was the plain frame.
//!
//! The server shows a print's check its three-panel image when it can trust
//! the drawing and the whole camera frame otherwise. These shapes say which it
//! was in structured fields, beside the one line of prose the image's note
//! carries, so a client and the agent learn each downgrade and its reason
//! rather than reading it out of a sentence.

use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// Which image a check or a look was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum ImageKind {
    /// The three panels: the frame with this layer's plan drawn on it, a
    /// close-up of the part, and the previous check's frame.
    ThreePanel,
    /// The camera's frame, scaled whole and uncropped, nothing drawn.
    WholeFrame,
}

/// Why a check or a look was shown the whole frame: one code per reason the
/// server gives one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum WholeFrameReasonCode {
    /// No print is named: a look at the printer.
    NoPrint,
    /// No camera calibration is configured.
    NoCalibration,
    /// No camera snapshot address is configured.
    NoStream,
    /// The print's G-code index gives no file position for the check.
    NoPosition,
    /// The G-code names no extruded object laid at that point.
    NoObject,
    /// It names no layer there.
    NoLayer,
    /// The layer's planned toolpath could not be read back.
    ToolpathUnread,
    /// The print's first layer is not down, or no frame of it has been judged
    /// yet.
    AlignmentPending,
    /// The last frame judged could not tell where the first layer lies: the
    /// toolhead covered the part, or the layer was not clearly found. A later
    /// check's frame is judged again. No evidence of a defect.
    AlignmentInconclusive,
    /// The print's G-code lays no object to align.
    AlignmentUnavailable,
    /// The camera's frames cannot be judged against the calibration.
    AlignmentFailed,
    /// Fewer than two landmarks are found, so whether the camera shifted is
    /// unknown.
    LandmarksNotFound,
    /// The camera moved since the drawing's correction was taken.
    ViewRefitRejected,
    /// The frame is not the calibration's size or stream.
    FrameMismatch,
    /// The printer reports the print paused, its bed moved off the plan's
    /// height.
    Paused,
    /// The composer refused the frame for another reason.
    Refused,
}

/// Why a check or a look was shown the whole frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ImageFallback {
    /// The reason, as a code.
    pub code: WholeFrameReasonCode,
    /// The reason, in the sentence the image's note gives it in.
    pub message: String,
}

impl Sample for ImageFallback {
    fn sample_full() -> Self {
        Self {
            code: WholeFrameReasonCode::AlignmentInconclusive,
            message: "the print's first-layer alignment is inconclusive on the frame judged last: the toolhead covers the part, so a later check's frame is judged again".to_owned(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            code: WholeFrameReasonCode::Paused,
            message: "the printer reports the print paused".to_owned(),
        }
    }
}

impl Sample for ImageKind {
    fn sample_full() -> Self {
        Self::ThreePanel
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self::WholeFrame]
    }
}

impl Sample for WholeFrameReasonCode {
    fn sample_full() -> Self {
        Self::AlignmentInconclusive
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::NoPrint,
            Self::NoCalibration,
            Self::NoStream,
            Self::NoPosition,
            Self::NoObject,
            Self::NoLayer,
            Self::ToolpathUnread,
            Self::AlignmentPending,
            Self::AlignmentUnavailable,
            Self::AlignmentFailed,
            Self::LandmarksNotFound,
            Self::ViewRefitRejected,
            Self::FrameMismatch,
            Self::Paused,
            Self::Refused,
        ]
    }
}
