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

use image::GrayImage;
use printobserver_core::LayerToolpath;

use crate::calibration::CameraCalibration;
use crate::compose::decode;
use crate::drift::check_frame;
use crate::error::ComposeError;
use crate::geometry::{band_outside, convex_hull, project_toolpath, sample_along};

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
    /// The offset, in calibration pixels.
    pub offset_px: f64,
    /// Whether it is at most the calibration's `alignment_max_offset_px`.
    pub passed: bool,
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
    let offset_px = best_shift(&grey, &path, &band);
    Ok(AlignmentVerdict {
        offset_px,
        passed: offset_px <= inputs.calibration.alignment_max_offset_px,
    })
}

/// The length of the shift at which the path and the band contrast most.
fn best_shift(grey: &GrayImage, path: &[[f64; 2]], band: &[[f64; 2]]) -> f64 {
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
    f64::from(best.1).hypot(f64::from(best.2))
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
