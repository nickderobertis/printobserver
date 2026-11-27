//! The per-print check that the projection lands on the printed first layer.
//!
//! A projection can be wrong with the camera unmoved, so each print verifies
//! it once its first object layer is down. The first object layer's whole
//! toolpath — walls, brim and infill — is projected at the frame's print
//! height and sampled every [`SAMPLE_PX`]; a band is sampled [`BAND_PX`]
//! outside the projected path's convex hull. For every shift of the two
//! within [`SEARCH_PX`], the score is how far the frame's mean grey on the
//! path lies from its mean grey on the band — printed material against the
//! sheet around it — smoothed over neighbouring shifts. `offset_px` is the
//! length of the best shift: how far the printed first layer lies from where
//! the calibration puts it. A nearest-edge score was measured and rejected:
//! a first layer's infill and the parts beside it put an edge near almost
//! any point, so a projection 49 px off scored as well as a right one.
//!
//! A first layer is *clearly found* ([`judge_first_layer`]) when its offset is
//! at most the calibration's `alignment_max_offset_px` and the peak smoothed
//! contrast — `match_strength`, in grey levels — is at least
//! [`CLEARLY_FOUND_CONTRAST`], on a frame where the part is not covered. Every
//! empty bed and every misplaced projection measured was found at the search's
//! edge, and every visible layer within 21 px at a contrast of at least 19. A
//! frame anything else is *inconclusive*, never a failure: the next check's
//! frame is judged again.
//!
//! The toolhead itself is high-contrast and scores as a found layer, so
//! coverage is not an image score. A frame is covered when the first layer is
//! small enough for the toolhead to hide ([`TOOLHEAD_HIDES_MM`]) and the
//! nozzle's planned position, projected at the nozzle's plane, lies within
//! [`TOOLHEAD_MARGIN_PX`] of the first layer's hull projected where the bed has
//! carried it at the frame's print height. As a small part grows, the bed
//! carries its first layer down and away from the head in the camera's view; a
//! large part's hull keeps the nozzle inside it at any height, while the head
//! hides only a little of it.

use image::GrayImage;
use printobserver_core::LayerToolpath;

use crate::calibration::CameraCalibration;
use crate::compose::decode;
use crate::drift::check_frame;
use crate::error::ComposeError;
use crate::geometry::{band_outside, convex_hull, project_toolpath, sample_along, signed_distance};
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// How far apart the path's samples are, in pixels.
pub const SAMPLE_PX: f64 = 4.0;

/// How far outside the path's hull the band lies, in pixels.
pub const BAND_PX: f64 = 10.0;

/// How far the printed layer is looked for from where it was projected, in
/// pixels: the largest `offset_px` there is.
pub const SEARCH_PX: i32 = 60;

/// The spacing of the shifts tried, in pixels.
const SHIFT_STEP: i32 = 2;

/// How many shifts either side each score is averaged over.
const SMOOTHING: i32 = 2;

/// The grey blur the frame is read through, in pixels.
const BLUR_SIGMA: f32 = 1.5;

/// The least peak smoothed contrast, in grey levels, at which a first layer
/// counts as clearly found.
pub const CLEARLY_FOUND_CONTRAST: f64 = 15.0;

/// How near the projected nozzle may lie to the projected first layer's hull,
/// in pixels, for the part to count as covered by the toolhead.
///
/// On the first shifted print the covered checks lay within 5.4 px of the hull
/// and the visible ones at least 15.3 px from it; 10 px sits midway. Calling a
/// covered part visible is the dangerous direction — a toolhead scored as the
/// layer — and calling a visible part covered only waits a check longer.
pub const TOOLHEAD_MARGIN_PX: f64 = 10.0;

/// The largest first layer the toolhead can hide, in bed millimetres: the
/// longer side of its extent on the bed. A first layer larger than this is
/// never covered, and is judged on the image score alone.
///
/// The 20 mm box the margin was measured on spans 18.9 mm; the smallest other
/// verified first layer, a jar top's, spans 82.5 mm, which the toolhead's hot
/// end and duct cannot hide as they hide the box.
pub const TOOLHEAD_HIDES_MM: f64 = 50.0;

/// What the alignment check is given.
#[derive(Debug, Clone, Copy)]
pub struct AlignmentInputs<'a> {
    /// One frame, past the first object layer.
    pub frame_jpeg: &'a [u8],
    /// The snapshot URL it came from.
    pub camera_snapshot_url: &'a str,
    /// The first object layer's toolpath.
    pub first_layer: &'a LayerToolpath,
    /// The print's height when the frame was taken, in millimetres.
    pub print_z_mm: f64,
    /// The printer's calibration.
    pub calibration: &'a CameraCalibration,
}

/// How far the printed first layer lies from its projection.
#[derive(Debug, Clone, Copy, PartialEq)]
// llmlint: ignore[invalid_states_unrepresentable] See suppressions.toml.
pub struct AlignmentVerdict {
    /// The offset, in calibration pixels: the length of `shift_px`.
    pub offset_px: f64,
    /// The best shift, `[dx, dy]` calibration pixels: where the printed layer
    /// lies from its projection.
    pub shift_px: [f64; 2],
    /// The peak smoothed contrast between the path and the band around it, in
    /// grey levels.
    pub match_strength: f64,
    /// Whether the layer is clearly found: the offset at most the
    /// calibration's `alignment_max_offset_px` and the contrast at least
    /// [`CLEARLY_FOUND_CONTRAST`].
    pub passed: bool,
}

/// Why a first layer was not clearly found on a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum InconclusiveReason {
    /// The toolhead stands over the part.
    Covered,
    /// The layer is not found within the threshold at the contrast it needs:
    /// an empty bed, a layer found at the search's edge, or one projecting
    /// nowhere in the frame.
    NotClearlyFound,
}

impl InconclusiveReason {
    /// The reason as the record spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Covered => "covered",
            Self::NotClearlyFound => "not_clearly_found",
        }
    }
}

/// What one frame says of the first layer's place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstLayerVerdict {
    /// It is clearly found.
    Passed,
    /// It cannot be told on this frame; a later one is judged again.
    Inconclusive(InconclusiveReason),
}

/// One judgement of the first layer on one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FirstLayerJudgement {
    /// What it came to.
    pub verdict: FirstLayerVerdict,
    /// The search's finding, absent when the layer projects nowhere in the
    /// frame.
    pub found: Option<AlignmentVerdict>,
}

/// What the first-layer judgement is given beyond the alignment check's
/// inputs.
#[derive(Debug, Clone, Copy)]
pub struct FirstLayerInputs<'a> {
    /// The alignment check's inputs.
    pub alignment: AlignmentInputs<'a>,
    /// Where the plan has the nozzle at the frame's file position, in bed
    /// millimetres; absent when the plan places it nowhere.
    pub nozzle_xy_mm: Option<[f64; 2]>,
}

/// Judge whether one frame clearly finds the first layer, on the uncorrected
/// projection, and whether the toolhead covers the part.
///
/// # Errors
///
/// [`ComposeError::FrameMismatch`] for a frame of another size or stream than
/// the calibration's, and [`ComposeError::Undecodable`] for one that is no
/// JPEG.
pub fn judge_first_layer(
    inputs: &FirstLayerInputs<'_>,
) -> Result<FirstLayerJudgement, ComposeError> {
    let alignment = &inputs.alignment;
    let found = match verify_first_layer_alignment(alignment) {
        Ok(found) => found,
        Err(ComposeError::OutlineOffFrame) => {
            return Ok(FirstLayerJudgement {
                verdict: FirstLayerVerdict::Inconclusive(InconclusiveReason::NotClearlyFound),
                found: None,
            });
        }
        Err(other) => return Err(other),
    };
    let verdict = if covered(inputs) {
        FirstLayerVerdict::Inconclusive(InconclusiveReason::Covered)
    } else if found.passed {
        FirstLayerVerdict::Passed
    } else {
        FirstLayerVerdict::Inconclusive(InconclusiveReason::NotClearlyFound)
    };
    Ok(FirstLayerJudgement {
        verdict,
        found: Some(found),
    })
}

/// How far the projected nozzle lies outside the projected first layer's
/// hull, in pixels, negative inside: what [`TOOLHEAD_MARGIN_PX`] is held
/// against. Nothing when the plan places the nozzle nowhere, or either does
/// not project.
#[must_use]
pub fn nozzle_from_first_layer_px(inputs: &FirstLayerInputs<'_>) -> Option<f64> {
    let [x, y] = inputs.nozzle_xy_mm?;
    let alignment = &inputs.alignment;
    let projector = alignment.calibration.projector();
    let nozzle = projector.pixel([x, y, 0.0])?;
    let pieces = project_toolpath(&projector, alignment.first_layer, alignment.print_z_mm);
    let hull = convex_hull(&sample_along(&pieces, SAMPLE_PX));
    (hull.len() >= 3).then(|| signed_distance(nozzle, &hull))
}

/// The longer side of a first layer's extent on the bed, in millimetres.
#[must_use]
pub fn first_layer_span_mm(first_layer: &LayerToolpath) -> f64 {
    let mut bounds: Option<[f64; 4]> = None;
    for segment in &first_layer.segments {
        for [x, y] in [segment.from_mm, segment.to_mm] {
            let (x, y) = (f64::from(x), f64::from(y));
            bounds = Some(bounds.map_or([x, y, x, y], |[x0, y0, x1, y1]| {
                [x0.min(x), y0.min(y), x1.max(x), y1.max(y)]
            }));
        }
    }
    bounds.map_or(0.0, |[x0, y0, x1, y1]| (x1 - x0).max(y1 - y0))
}

/// Whether the toolhead covers the part on the frame: a first layer small
/// enough to hide, with the projected nozzle near it.
fn covered(inputs: &FirstLayerInputs<'_>) -> bool {
    first_layer_span_mm(inputs.alignment.first_layer) <= TOOLHEAD_HIDES_MM
        && nozzle_from_first_layer_px(inputs).is_some_and(|distance| distance <= TOOLHEAD_MARGIN_PX)
}

/// Find how far the printed first layer lies from where the calibration puts
/// it, and whether that is within the calibration's threshold.
///
/// # Errors
///
/// [`ComposeError::FrameMismatch`] for a frame of another size or stream than
/// the calibration's, [`ComposeError::Undecodable`] for one that is no JPEG,
/// and [`ComposeError::OutlineOffFrame`] when no point of the layer lands in
/// the frame.
pub fn verify_first_layer_alignment(
    inputs: &AlignmentInputs<'_>,
) -> Result<AlignmentVerdict, ComposeError> {
    let frame = decode(inputs.frame_jpeg)?;
    check_frame(
        inputs.calibration,
        [frame.width(), frame.height()],
        inputs.camera_snapshot_url,
    )?;
    let grey =
        imageproc::filter::gaussian_blur_f32(&image::imageops::grayscale(&frame), BLUR_SIGMA);
    let pieces = project_toolpath(
        &inputs.calibration.projector(),
        inputs.first_layer,
        inputs.print_z_mm,
    );
    let (w, h) = (f64::from(frame.width()), f64::from(frame.height()));
    let path: Vec<[f64; 2]> = sample_along(&pieces, SAMPLE_PX)
        .into_iter()
        .filter(|[x, y]| (0.0..w).contains(x) && (0.0..h).contains(y))
        .collect();
    if path.len() < 3 {
        return Err(ComposeError::OutlineOffFrame);
    }
    let band = band_outside(&convex_hull(&path), BAND_PX, SAMPLE_PX);
    let (match_strength, dx, dy) = best_shift(&grey, &path, &band);
    let shift_px = [f64::from(dx), f64::from(dy)];
    let offset_px = shift_px[0].hypot(shift_px[1]);
    Ok(AlignmentVerdict {
        offset_px,
        shift_px,
        match_strength,
        passed: offset_px <= inputs.calibration.alignment_max_offset_px
            && match_strength >= CLEARLY_FOUND_CONTRAST,
    })
}

/// The shift at which the path and the band contrast most, and that
/// contrast, smoothed.
fn best_shift(grey: &GrayImage, path: &[[f64; 2]], band: &[[f64; 2]]) -> (f64, i32, i32) {
    let side = usize::try_from(2 * SEARCH_PX / SHIFT_STEP + 1).unwrap_or(1);
    let index = |shift: i32| usize::try_from((shift + SEARCH_PX) / SHIFT_STEP).unwrap_or(0);
    let mut contrast = vec![0.0_f64; side * side];
    let shifts = || (-SEARCH_PX..=SEARCH_PX).step_by(usize::try_from(SHIFT_STEP).unwrap_or(1));
    for dx in shifts() {
        for dy in shifts() {
            let inside = mean_at(grey, path, dx, dy);
            let outside = mean_at(grey, band, dx, dy);
            contrast[index(dy) * side + index(dx)] = (inside - outside).abs();
        }
    }
    let mut best = (f64::NEG_INFINITY, 0, 0);
    for dx in shifts() {
        for dy in shifts() {
            if dx * dx + dy * dy > SEARCH_PX * SEARCH_PX {
                continue;
            }
            let mut sum = 0.0;
            let mut count = 0.0;
            for ox in -SMOOTHING..=SMOOTHING {
                for oy in -SMOOTHING..=SMOOTHING {
                    let x = (index(dx).cast_signed() + isize::try_from(ox).unwrap_or(0))
                        .clamp(0, side.cast_signed() - 1)
                        .cast_unsigned();
                    let y = (index(dy).cast_signed() + isize::try_from(oy).unwrap_or(0))
                        .clamp(0, side.cast_signed() - 1)
                        .cast_unsigned();
                    sum += contrast[y * side + x];
                    count += 1.0;
                }
            }
            let score = sum / count;
            if score > best.0 {
                best = (score, dx, dy);
            }
        }
    }
    best
}

/// The mean grey at points shifted by `(dx, dy)`, each clamped into the frame.
fn mean_at(grey: &GrayImage, points: &[[f64; 2]], dx: i32, dy: i32) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let (w, h) = (f64::from(grey.width() - 1), f64::from(grey.height() - 1));
    let mut sum = 0.0;
    for [x, y] in points {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped into the frame, whose size fits a u32"
        )]
        let (px, py) = (
            (x + f64::from(dx)).round().clamp(0.0, w) as u32,
            (y + f64::from(dy)).round().clamp(0.0, h) as u32,
        );
        sum += f64::from(grey.get_pixel(px, py).0[0]);
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a path holds at most some tens of thousands of samples"
    )]
    let count = points.len() as f64;
    sum / count
}
