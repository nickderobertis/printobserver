//! Whether the camera's view still matches what a check draws through.
//!
//! A calibration fixes where the camera looked when it was fitted, and a
//! camera that shifts a little afterwards is ordinary on a real printer. A
//! print's own first layer, once it is clearly found, says where the
//! projection lands now ([`crate::alignment`]); that offset becomes the print's
//! [`ViewCorrection`], a translation every later drawing of the print goes
//! through. The landmarks found agreeing on the frame it was taken on are kept
//! as its reference, and each later frame is judged against them
//! ([`judge_view`]): landmarks that moved from the calibration but sit where
//! the reference had them are the same shifted camera, and the frame is drawn
//! through the correction; two or more that moved again mean the camera moved
//! again.
//!
//! Landmark-fitted models were measured and rejected: on the first shifted
//! print, every landmark found agreeing sat on one side of the frame, about
//! 60 to 80 px off, while the part had moved about 15 px, so any model fitted
//! to them landed the outline beside the part. The other three models stay in
//! [`ViewModel`] so the record's shape need not change when one is earned.

use image::GrayImage;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

use crate::calibration::CameraCalibration;
use crate::drift::{HIDDEN_BELOW, Located, VIEW_SEARCH_RADIUS_PX, locate_landmarks_within};

/// How close two landmarks' offsets must be to count as agreeing, in pixels:
/// the two that agreed on the first shifted print sat 20 px apart.
pub const AGREEMENT_PX: f64 = 25.0;

/// The model a [`ViewCorrection`] maps pixels by. Only [`Self::Translation`]
/// is ever written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum ViewModel {
    /// A shift alone.
    Translation,
    /// A shift, a rotation and one scale.
    Similarity,
    /// A general linear map and a shift.
    Affine,
    /// A plane-to-plane projective map.
    Homography,
}

/// One landmark of a correction's reference: where it was found on the frame
/// the correction was taken on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ReferenceLandmark {
    /// The calibration's name for it.
    pub name: String,
    /// How far from its calibrated place it was found, `[dx, dy]` pixels.
    pub offset_px: [f64; 2],
    /// The correlation it was found at.
    pub score: f64,
}

/// How a print's drawings map the calibrated view onto the current frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ViewCorrection {
    /// The model `matrix` is of.
    pub model: ViewModel,
    /// Maps a pixel of the calibrated view to the current frame, in
    /// homogeneous coordinates, row by row.
    pub matrix: [[f64; 3]; 3],
    /// The landmarks found agreeing on the frame the correction was taken on,
    /// which later frames are compared against.
    pub landmarks: Vec<ReferenceLandmark>,
    /// The RMS distance between those landmarks' offsets and the correction's
    /// shift, in pixels: how far the landmarks disagree with the part.
    pub residual_px: f64,
}

impl ViewCorrection {
    /// The translation by `shift_px`, its reference `landmarks`.
    #[must_use]
    pub fn translation(shift_px: [f64; 2], landmarks: Vec<ReferenceLandmark>) -> Self {
        let squares: Vec<f64> = landmarks
            .iter()
            .map(|landmark| {
                let (dx, dy) = (
                    landmark.offset_px[0] - shift_px[0],
                    landmark.offset_px[1] - shift_px[1],
                );
                dx * dx + dy * dy
            })
            .collect();
        let residual_px = if squares.is_empty() {
            0.0
        } else {
            (squares.iter().sum::<f64>() / f64::from(u32::try_from(squares.len()).unwrap_or(1)))
                .sqrt()
        };
        Self {
            model: ViewModel::Translation,
            matrix: [
                [1.0, 0.0, shift_px[0]],
                [0.0, 1.0, shift_px[1]],
                [0.0, 0.0, 1.0],
            ],
            landmarks,
            residual_px,
        }
    }

    /// The shift the correction applies at the frame's origin, `[dx, dy]`.
    #[must_use]
    pub const fn shift_px(&self) -> [f64; 2] {
        [self.matrix[0][2], self.matrix[1][2]]
    }

    /// Where one pixel of the calibrated view lands on the current frame.
    #[must_use]
    pub fn apply(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let m = &self.matrix;
        let w = m[2][0] * x + m[2][1] * y + m[2][2];
        let w = if w.abs() < 1e-12 { 1.0 } else { w };
        [
            (m[0][0] * x + m[0][1] * y + m[0][2]) / w,
            (m[1][0] * x + m[1][1] * y + m[1][2]) / w,
        ]
    }
}

/// What one judgement of the view found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum ViewOutcome {
    /// No more than one found landmark sits beyond the calibration's
    /// tolerance of its calibrated place: one alone is as likely a false match
    /// as a moved camera.
    Unshifted,
    /// Landmarks moved from the calibration but agree with the reference of
    /// the correction in force, so the frame is drawn through it.
    Refit,
    /// Fewer than two landmarks are found, or fewer than two of the
    /// reference's are found again.
    LandmarksNotFound,
    /// Landmarks moved and no correction is in force, or two or more moved
    /// beyond the tolerance from the correction's reference: the camera moved
    /// again.
    ViewRefitRejected,
    /// The frame is not the calibration's size or stream.
    FrameMismatch,
}

impl ViewOutcome {
    /// Whether a frame so judged may be drawn on.
    #[must_use]
    pub const fn drawable(self) -> bool {
        matches!(self, Self::Unshifted | Self::Refit)
    }
}

/// One landmark as one frame showed it.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundLandmark {
    /// The calibration's name for it.
    pub name: String,
    /// Where and how well it was found.
    pub located: Located,
}

impl FoundLandmark {
    /// How far from its calibrated place it was found.
    #[must_use]
    pub fn distance_px(&self) -> f64 {
        self.located.offset_px[0].hypot(self.located.offset_px[1])
    }

    /// Whether it was found at all, rather than hidden.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.located.score >= HIDDEN_BELOW
    }
}

/// One judgement of the view.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewJudgement {
    /// What it found.
    pub outcome: ViewOutcome,
    /// Why the frame may not be drawn on, for the refusals.
    pub reason: Option<String>,
    /// Every landmark of the calibration as this frame showed it, in its
    /// order.
    pub landmarks: Vec<FoundLandmark>,
}

impl ViewJudgement {
    /// The largest set of visible landmarks whose offsets all lie within
    /// [`AGREEMENT_PX`] of one another: the reference a correction taken on
    /// this frame keeps. Ties go to the set found the more surely.
    #[must_use]
    pub fn agreeing(&self) -> Vec<ReferenceLandmark> {
        let visible: Vec<&FoundLandmark> = self.landmarks.iter().filter(|l| l.visible()).collect();
        let mut best: Vec<&FoundLandmark> = Vec::new();
        let mut best_score = f64::NEG_INFINITY;
        let count = visible.len().min(16);
        for mask in 1_u32..(1 << count) {
            let members: Vec<&FoundLandmark> = (0..count)
                .filter(|bit| mask & (1 << bit) != 0)
                .map(|bit| visible[bit])
                .collect();
            let agree = members.iter().enumerate().all(|(i, a)| {
                members.iter().skip(i + 1).all(|b| {
                    (a.located.offset_px[0] - b.located.offset_px[0])
                        .hypot(a.located.offset_px[1] - b.located.offset_px[1])
                        <= AGREEMENT_PX
                })
            });
            if !agree {
                continue;
            }
            let score: f64 = members.iter().map(|l| f64::from(l.located.score)).sum();
            if members.len() > best.len() || (members.len() == best.len() && score > best_score) {
                best = members;
                best_score = score;
            }
        }
        best.into_iter()
            .map(|landmark| ReferenceLandmark {
                name: landmark.name.clone(),
                offset_px: landmark.located.offset_px,
                score: f64::from(landmark.located.score),
            })
            .collect()
    }

    /// Whether two or more visible landmarks sit beyond the calibration's
    /// tolerance of their calibrated places.
    #[must_use]
    pub fn moved(&self, calibration: &CameraCalibration) -> bool {
        self.landmarks
            .iter()
            .filter(|l| l.visible() && l.distance_px() > calibration.drift_tolerance_px)
            .count()
            >= 2
    }
}

/// Judge one grey frame's view, against the reference of the correction in
/// force when there is one, every landmark searched within
/// [`VIEW_SEARCH_RADIUS_PX`].
#[must_use]
pub fn judge_view(
    calibration: &CameraCalibration,
    frame: &GrayImage,
    correction: Option<&ViewCorrection>,
) -> ViewJudgement {
    let landmarks: Vec<FoundLandmark> = calibration
        .landmarks
        .iter()
        .zip(locate_landmarks_within(
            calibration,
            frame,
            VIEW_SEARCH_RADIUS_PX,
        ))
        .map(|(landmark, located)| FoundLandmark {
            name: landmark.name.clone(),
            located,
        })
        .collect();
    let tolerance = calibration.drift_tolerance_px;
    let judged = |outcome: ViewOutcome, reason: Option<String>, landmarks: Vec<FoundLandmark>| {
        ViewJudgement {
            outcome,
            reason,
            landmarks,
        }
    };
    let visible = landmarks.iter().filter(|l| l.visible()).count();
    let Some(correction) = correction else {
        if visible < 2 {
            return judged(
                ViewOutcome::LandmarksNotFound,
                Some(LANDMARKS_NOT_FOUND.to_owned()),
                landmarks,
            );
        }
        let furthest = landmarks
            .iter()
            .filter(|l| l.visible() && l.distance_px() > tolerance)
            .max_by(|a, b| a.distance_px().total_cmp(&b.distance_px()));
        let moved = landmarks
            .iter()
            .filter(|l| l.visible() && l.distance_px() > tolerance)
            .count();
        if moved >= 2
            && let Some(furthest) = furthest
        {
            let reason = format!(
                "the camera has shifted since it was calibrated, {} being {:.1} px from where it was, and no correction taken from this print's first layer is in force yet",
                furthest.name,
                furthest.distance_px()
            );
            return judged(ViewOutcome::ViewRefitRejected, Some(reason), landmarks);
        }
        return judged(ViewOutcome::Unshifted, None, landmarks);
    };
    let again: Vec<(&ReferenceLandmark, &FoundLandmark)> = correction
        .landmarks
        .iter()
        .filter_map(|reference| {
            landmarks
                .iter()
                .find(|found| found.name == reference.name && found.visible())
                .map(|found| (reference, found))
        })
        .collect();
    if again.len() < 2 {
        return judged(
            ViewOutcome::LandmarksNotFound,
            Some(LANDMARKS_NOT_FOUND_AGAIN.to_owned()),
            landmarks,
        );
    }
    let from_reference = |(reference, found): &(&ReferenceLandmark, &FoundLandmark)| {
        (found.located.offset_px[0] - reference.offset_px[0])
            .hypot(found.located.offset_px[1] - reference.offset_px[1])
    };
    let moved_again: Vec<(&str, f64)> = again
        .iter()
        .map(|pair| (pair.0.name.as_str(), from_reference(pair)))
        .filter(|(_, distance)| *distance > tolerance)
        .collect();
    if moved_again.len() >= 2 {
        let (name, distance) = moved_again
            .iter()
            .copied()
            .fold(
                moved_again[0],
                |far, next| if next.1 > far.1 { next } else { far },
            );
        let reason = format!(
            "the camera has moved again since this print's first-layer correction was taken: {name} is {distance:.1} px from where that correction's frame had it"
        );
        return judged(ViewOutcome::ViewRefitRejected, Some(reason), landmarks);
    }
    let shifted = landmarks
        .iter()
        .filter(|l| l.visible() && l.distance_px() > tolerance)
        .count()
        >= 2;
    let outcome = if shifted {
        ViewOutcome::Refit
    } else {
        ViewOutcome::Unshifted
    };
    judged(outcome, None, landmarks)
}

/// Why a frame with fewer than two landmarks found is not drawn on.
pub const LANDMARKS_NOT_FOUND: &str = "fewer than two of the calibration's landmarks are found in the frame, so whether the camera has shifted is unknown";

/// Why a frame finding fewer than two of a correction's reference landmarks
/// again is not drawn on.
pub const LANDMARKS_NOT_FOUND_AGAIN: &str = "fewer than two of the landmarks this print's first-layer correction was taken against are found in the frame, so whether the camera has moved again is unknown";
