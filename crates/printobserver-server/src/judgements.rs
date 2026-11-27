//! The records of what the server judged of a print's camera frames.
//!
//! Every judgement of a print's first-layer alignment and of its camera's view
//! is written into the print's history, each naming by its SHA-256 the stored
//! frame it was made on ([`printobserver_core::ImageRef::sha256`]), so a
//! reader can open the very frame behind any verdict. Two further kinds are
//! change notices rather than judgements: the correction a pass put in force
//! while the landmarks had moved ([`CameraViewRefitPayload`]), and each change
//! in which image a print's checks are shown, with its reason
//! ([`CheckImageChangedPayload`]).
//!
//! This crate alone writes these kinds, so it declares them, as it declares the
//! startup reconciliation.

use printobserver_panels::{ViewCorrection, ViewOutcome};
use printobserver_supervisor_api::{ImageFallback, ImageKind};
use printobserver_types::EventPayload;
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// What one judgement of a print's first layer came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum AlignmentJudgedVerdict {
    /// The first layer is clearly found, and its offset is the print's
    /// correction from now on.
    Passed,
    /// The frame could not tell; a later check's frame is judged again.
    Inconclusive,
    /// The camera's frames cannot be judged against the calibration, which
    /// holds for the rest of the print.
    Failed,
}

/// One judgement of a print's first-layer alignment, on one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FirstLayerAlignmentJudgedPayload {
    /// What it came to.
    pub verdict: AlignmentJudgedVerdict,
    /// How far the printed first layer was found from its uncorrected
    /// projection, in calibration pixels; null when it was not searched for.
    pub offset_px: Option<f64>,
    /// The correction the pass put in force, `[dx, dy]` pixels; null unless it
    /// passed.
    pub correction_px: Option<[f64; 2]>,
    /// The peak smoothed contrast the layer was found at, in grey levels; null
    /// when it was not searched for.
    pub match_strength: Option<f64>,
    /// `clearly_found` for a pass, `covered` or `not_clearly_found` for an
    /// inconclusive judgement, and the failure's own sentence for a failure.
    pub reason: String,
    /// The stored frame it was judged on.
    pub frame_sha256: String,
    /// The correction in force when the frame was judged; null before the
    /// print's first pass.
    pub view_correction: Option<ViewCorrection>,
}

impl EventPayload for FirstLayerAlignmentJudgedPayload {
    const KIND: &'static str = "first_layer_alignment_judged";
}

/// One judgement of a print's camera view, on one frame: one for every
/// judgement, whatever it found and whether or not it repeats the last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct CameraViewJudgedPayload {
    /// The stored frame it was judged on.
    pub frame_sha256: String,
    /// What it found.
    pub outcome: ViewOutcome,
    /// The correction in force when the frame was judged; null when none was.
    pub view_correction: Option<ViewCorrection>,
    /// Why the frame may not be drawn on, for the three refusals; null
    /// otherwise.
    pub reason: Option<String>,
}

impl EventPayload for CameraViewJudgedPayload {
    const KIND: &'static str = "camera_view_judged";
}

/// A first-layer pass put a correction in force while the landmarks had moved
/// from the calibration: the print's first, or one taken again after the
/// camera moved again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct CameraViewRefitPayload {
    /// The stored frame the correction was taken on.
    pub frame_sha256: String,
    /// The correction now in force.
    pub view_correction: ViewCorrection,
}

impl EventPayload for CameraViewRefitPayload {
    const KIND: &'static str = "camera_view_refit";
}

/// The image a print's checks are shown changed: its first check's image, and
/// every check whose image kind or fallback code differs from the last one
/// recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct CheckImageChangedPayload {
    /// Which image the check was shown.
    pub kind: ImageKind,
    /// Why it was the whole frame; null for the three-panel image.
    pub fallback: Option<ImageFallback>,
    /// The stored frame the check's image was made of.
    pub frame_sha256: String,
}

impl EventPayload for CheckImageChangedPayload {
    const KIND: &'static str = "check_image_changed";
}

/// A frame's SHA-256, as samples name one.
const SAMPLE_SHA256: &str = "9f2c4d1e8b7a6f5e4d3c2b1a0f9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e";

/// The correction samples carry.
fn sample_correction() -> ViewCorrection {
    ViewCorrection::translation(
        [14.0, 4.0],
        vec![
            printobserver_panels::ReferenceLandmark {
                name: "frame-screws-right-front".to_owned(),
                offset_px: [59.1, 0.7],
                score: 0.83,
            },
            printobserver_panels::ReferenceLandmark {
                name: "frame-label-right".to_owned(),
                offset_px: [79.6, 0.2],
                score: 0.84,
            },
        ],
    )
}

impl Sample for FirstLayerAlignmentJudgedPayload {
    fn sample_full() -> Self {
        Self {
            verdict: AlignmentJudgedVerdict::Passed,
            offset_px: Some(14.6),
            correction_px: Some([14.0, 4.0]),
            match_strength: Some(57.6),
            reason: "clearly_found".to_owned(),
            frame_sha256: SAMPLE_SHA256.to_owned(),
            view_correction: Some(sample_correction()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            verdict: AlignmentJudgedVerdict::Inconclusive,
            offset_px: None,
            correction_px: None,
            match_strength: None,
            reason: "not_clearly_found".to_owned(),
            frame_sha256: SAMPLE_SHA256.to_owned(),
            view_correction: None,
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self {
            verdict: AlignmentJudgedVerdict::Failed,
            reason: "the camera is not the stream the calibration was fitted on".to_owned(),
            ..Self::sample_minimal()
        }]
    }
}

impl Sample for CameraViewJudgedPayload {
    fn sample_full() -> Self {
        Self {
            frame_sha256: SAMPLE_SHA256.to_owned(),
            outcome: ViewOutcome::ViewRefitRejected,
            view_correction: Some(sample_correction()),
            reason: Some(
                "the camera has moved again since this print's first-layer correction was taken: frame-label-right is 10.2 px from where that correction's frame had it"
                    .to_owned(),
            ),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            frame_sha256: SAMPLE_SHA256.to_owned(),
            outcome: ViewOutcome::Unshifted,
            view_correction: None,
            reason: None,
        }
    }

    fn sample_alternates() -> Vec<Self> {
        [
            ViewOutcome::Refit,
            ViewOutcome::LandmarksNotFound,
            ViewOutcome::FrameMismatch,
        ]
        .into_iter()
        .map(|outcome| Self {
            outcome,
            ..Self::sample_minimal()
        })
        .collect()
    }
}

impl Sample for CameraViewRefitPayload {
    fn sample_full() -> Self {
        Self {
            frame_sha256: SAMPLE_SHA256.to_owned(),
            view_correction: sample_correction(),
        }
    }
}

impl Sample for CheckImageChangedPayload {
    fn sample_full() -> Self {
        Self {
            kind: ImageKind::WholeFrame,
            fallback: Some(ImageFallback::sample_full()),
            frame_sha256: SAMPLE_SHA256.to_owned(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            kind: ImageKind::ThreePanel,
            fallback: None,
            frame_sha256: SAMPLE_SHA256.to_owned(),
        }
    }
}
