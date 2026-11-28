//! Whether the camera has moved since it was calibrated, and whether a frame
//! is the calibration's at all.
//!
//! Each landmark's stored patch is located by normalized cross-correlation
//! within its search radius; a best score below [`HIDDEN_BELOW`] counts as
//! hidden (by the head, the gantry or a part). The camera has moved when at
//! least two visible landmarks are displaced by more than the calibration's
//! tolerance, and drift is unverified when fewer than two are visible: one
//! moved landmark alone is as likely a false match as a moved camera.

use image::GrayImage;

use crate::calibration::CameraCalibration;
use crate::error::{ComposeError, FrameField};

/// The correlation below which a landmark counts as hidden.
pub const HIDDEN_BELOW: f32 = 0.6;

/// Where one landmark was found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// How far from its calibrated place, in pixels, `[dx, dy]`.
    pub offset_px: [f64; 2],
    /// The best correlation found.
    pub score: f32,
}

/// Refuse a frame of another size, or from another stream, than the
/// calibration's.
///
/// # Errors
///
/// [`ComposeError::FrameMismatch`] naming the resolution or the stream.
pub fn check_frame(
    calibration: &CameraCalibration,
    size: [u32; 2],
    camera_snapshot_url: &str,
) -> Result<(), ComposeError> {
    if size != calibration.image_size_px {
        let [ew, eh] = calibration.image_size_px;
        return Err(ComposeError::FrameMismatch {
            field: FrameField::Resolution,
            expected: format!("{ew}x{eh}"),
            got: format!("{}x{}", size[0], size[1]),
        });
    }
    if camera_snapshot_url != calibration.camera_snapshot_url {
        return Err(ComposeError::FrameMismatch {
            field: FrameField::Stream,
            expected: calibration.camera_snapshot_url.clone(),
            got: camera_snapshot_url.to_owned(),
        });
    }
    Ok(())
}

/// How far the server searches for every landmark, in pixels, whatever each
/// calibration file's own `search_radius_px`: the smallest radius at which both
/// landmarks that agree are found on every frame of the first print a slightly
/// shifted camera recorded. Wider radii found no further landmark there, only
/// false matches.
pub const VIEW_SEARCH_RADIUS_PX: u32 = 96;

/// Locate every landmark in a grey frame, in the calibration's order, each
/// within its own `search_radius_px`.
#[must_use]
pub fn locate_landmarks(calibration: &CameraCalibration, frame: &GrayImage) -> Vec<Located> {
    calibration
        .landmarks
        .iter()
        .zip(calibration.patches())
        .map(|(landmark, patch)| {
            locate(
                frame,
                patch,
                landmark.center_px,
                i64::from(landmark.search_radius_px),
            )
        })
        .collect()
}

/// Locate every landmark in a grey frame, in the calibration's order, each
/// within `radius_px` of its calibrated place rather than within its own
/// radius.
#[must_use]
pub fn locate_landmarks_within(
    calibration: &CameraCalibration,
    frame: &GrayImage,
    radius_px: u32,
) -> Vec<Located> {
    calibration
        .landmarks
        .iter()
        .zip(calibration.patches())
        .map(|(landmark, patch)| locate(frame, patch, landmark.center_px, i64::from(radius_px)))
        .collect()
}

/// Rule on drift from where the landmarks were found.
///
/// # Errors
///
/// [`ComposeError::CameraMoved`] naming the landmark that moved furthest, and
/// [`ComposeError::DriftUnverified`] when fewer than two are visible.
pub fn check_drift(calibration: &CameraCalibration, frame: &GrayImage) -> Result<(), ComposeError> {
    let found = locate_landmarks(calibration, frame);
    let visible: Vec<(&str, f64)> = calibration
        .landmarks
        .iter()
        .zip(&found)
        .filter(|(_, located)| located.score >= HIDDEN_BELOW)
        .map(|(landmark, located)| {
            (
                landmark.name.as_str(),
                located.offset_px[0].hypot(located.offset_px[1]),
            )
        })
        .collect();
    if visible.len() < 2 {
        return Err(ComposeError::DriftUnverified);
    }
    let moved: Vec<&(&str, f64)> = visible
        .iter()
        .filter(|(_, offset)| *offset > calibration.drift_tolerance_px)
        .collect();
    if moved.len() >= 2 {
        let (landmark, offset_px) =
            moved.iter().copied().fold(
                moved[0],
                |far, next| if next.1 > far.1 { next } else { far },
            );
        return Err(ComposeError::CameraMoved {
            landmark: (*landmark).to_owned(),
            offset_px: *offset_px,
        });
    }
    Ok(())
}

/// Find one patch near where it was, by normalized cross-correlation, with
/// the peak refined to a fraction of a pixel by a parabola through its
/// neighbours.
fn locate(frame: &GrayImage, patch: &GrayImage, center: [f64; 2], radius: i64) -> Located {
    let hidden = Located {
        offset_px: [0.0, 0.0],
        score: -1.0,
    };
    let (pw, ph) = (i64::from(patch.width()), i64::from(patch.height()));
    let (fw, fh) = (i64::from(frame.width()), i64::from(frame.height()));
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a landmark's centre is a pixel of a frame of at most a few thousand pixels"
    )]
    let (left, top) = (
        center[0].round() as i64 - pw / 2,
        center[1].round() as i64 - ph / 2,
    );
    let template: Vec<f64> = patch
        .as_raw()
        .iter()
        .map(|value| f64::from(*value))
        .collect();
    let count = u64::try_from(template.len()).unwrap_or(u64::MAX);
    let mean = template.iter().sum::<f64>() / f64::from(u32::try_from(count).unwrap_or(u32::MAX));
    let centred: Vec<f64> = template.iter().map(|value| value - mean).collect();
    let template_norm = centred
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if template_norm < 1e-3 {
        return hidden;
    }
    let pixels = frame.as_raw();
    // The cross term carries no cancellation — the template is centred — so
    // one row's products sum in single precision, eight lanes at a time.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a centred grey value lies within ±255"
    )]
    let weights: Vec<f32> = centred.iter().map(|value| *value as f32).collect();
    let side = usize::try_from(2 * radius + 1).unwrap_or(0);
    let mut scores = vec![f32::NEG_INFINITY; side * side];
    let width = usize::try_from(fw).unwrap_or(0);
    let pw_us = usize::try_from(pw).unwrap_or(0);
    let patch_rows = usize::try_from(ph).unwrap_or(0);
    // Each window's sum and sum of squares are whole numbers, read exactly
    // off integral images of the searched region: `squares - sum² / count` in
    // floating point cancels on a near-flat window and scored such windows
    // above 1, which won any search wide enough to reach one.
    let region = Region::around(left, top, radius, [pw, ph], [fw, fh]);
    let (sums, squares) = region.integrals(pixels, width);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (x0, y0) = (left + dx, top + dy);
            if x0 < 0 || y0 < 0 || x0 + pw > fw || y0 + ph > fh {
                continue;
            }
            let (x0, y0) = (
                usize::try_from(x0).unwrap_or(0),
                usize::try_from(y0).unwrap_or(0),
            );
            let sum = region.window(&sums, x0, y0, pw_us, patch_rows);
            let square_sum = region.window(&squares, x0, y0, pw_us, patch_rows);
            let spread_squared =
                u128::from(count) * u128::from(square_sum) - u128::from(sum) * u128::from(sum);
            if spread_squared == 0 {
                continue;
            }
            let mut cross = 0.0_f64;
            for (row, weights) in weights.chunks_exact(pw_us).enumerate() {
                let start = (y0 + row) * width + x0;
                cross += f64::from(dot(&pixels[start..start + pw_us], weights));
            }
            #[expect(
                clippy::cast_precision_loss,
                reason = "a window's scaled spread is far below 2^53 for any patch a calibration holds"
            )]
            let spread = (spread_squared as f64).sqrt()
                / f64::from(u32::try_from(count).unwrap_or(u32::MAX)).sqrt();
            let at = usize::try_from((dy + radius) * (2 * radius + 1) + dx + radius).unwrap_or(0);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a correlation lies within -1 to 1"
            )]
            let score = (cross / (spread * template_norm)) as f32;
            scores[at] = score;
        }
    }
    let Some((best, score)) = scores
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, score)| score.is_finite())
        .reduce(|best, next| if next.1 > best.1 { next } else { best })
    else {
        return hidden;
    };
    Located {
        offset_px: refined_peak(&scores, side, best, score, radius),
        score,
    }
}

/// One row's products with its weights, summed over eight lanes.
///
/// Each eight are summed in one straight run of arithmetic rather than a loop
/// over the lanes, which keeps the run one block an optimiser vectorises and
/// coverage instrumentation counts once.
fn dot(pixels: &[u8], weights: &[f32]) -> f32 {
    let mut lanes = [0.0_f32; 8];
    let (pixel_chunks, pixel_rest) = pixels.as_chunks::<8>();
    let (weight_chunks, weight_rest) = weights.as_chunks::<8>();
    for ([p0, p1, p2, p3, p4, p5, p6, p7], [w0, w1, w2, w3, w4, w5, w6, w7]) in
        pixel_chunks.iter().zip(weight_chunks)
    {
        lanes = [
            lanes[0] + f32::from(*p0) * w0,
            lanes[1] + f32::from(*p1) * w1,
            lanes[2] + f32::from(*p2) * w2,
            lanes[3] + f32::from(*p3) * w3,
            lanes[4] + f32::from(*p4) * w4,
            lanes[5] + f32::from(*p5) * w5,
            lanes[6] + f32::from(*p6) * w6,
            lanes[7] + f32::from(*p7) * w7,
        ];
    }
    let rest: f32 = pixel_rest
        .iter()
        .zip(weight_rest)
        .map(|(pixel, weight)| f32::from(*pixel) * weight)
        .sum();
    lanes.iter().sum::<f32>() + rest
}

/// The part of a frame one search reads, clamped inside the frame, and its
/// integral images' indexing.
struct Region {
    /// Its left column and top row in the frame.
    origin: [usize; 2],
    /// Its width and height.
    size: [usize; 2],
}

impl Region {
    /// The region every window of a search reads.
    fn around(left: i64, top: i64, radius: i64, patch: [i64; 2], frame: [i64; 2]) -> Self {
        let clamp = |value: i64, most: i64| usize::try_from(value.clamp(0, most)).unwrap_or(0);
        let (x0, y0) = (
            clamp(left - radius, frame[0]),
            clamp(top - radius, frame[1]),
        );
        let (x1, y1) = (
            clamp(left + radius + patch[0], frame[0]),
            clamp(top + radius + patch[1], frame[1]),
        );
        Self {
            origin: [x0, y0],
            size: [x1.saturating_sub(x0), y1.saturating_sub(y0)],
        }
    }

    /// The integral images of the region's grey values and their squares,
    /// each `(width + 1) × (height + 1)` with a zero first row and column.
    fn integrals(&self, pixels: &[u8], width: usize) -> (Vec<u64>, Vec<u64>) {
        let stride = self.size[0] + 1;
        let mut sums = vec![0_u64; stride * (self.size[1] + 1)];
        let mut squares = sums.clone();
        for row in 0..self.size[1] {
            let (mut along, mut along_squares) = (0_u64, 0_u64);
            let start = (self.origin[1] + row) * width + self.origin[0];
            for (column, pixel) in pixels[start..start + self.size[0]].iter().enumerate() {
                let value = u64::from(*pixel);
                along += value;
                along_squares += value * value;
                let at = (row + 1) * stride + column + 1;
                sums[at] = sums[at - stride] + along;
                squares[at] = squares[at - stride] + along_squares;
            }
        }
        (sums, squares)
    }

    /// The sum over one window, its top-left `(x, y)` in the frame.
    fn window(&self, integral: &[u64], x: usize, y: usize, w: usize, h: usize) -> u64 {
        let stride = self.size[0] + 1;
        let (x, y) = (x - self.origin[0], y - self.origin[1]);
        integral[(y + h) * stride + x + w] + integral[y * stride + x]
            - integral[y * stride + x + w]
            - integral[(y + h) * stride + x]
    }
}

/// A correlation peak's place as an offset from the search's centre, refined
/// to a fraction of a pixel by a parabola through its neighbours on each axis.
fn refined_peak(scores: &[f32], side: usize, best: usize, score: f32, radius: i64) -> [f64; 2] {
    let (row, column) = (best / side, best % side);
    let refine = |before: Option<f32>, after: Option<f32>| match (before, after) {
        (Some(a), Some(c)) if a.is_finite() && c.is_finite() => {
            let curve = a - 2.0 * score + c;
            if curve.abs() > 1e-9 {
                f64::from(0.5 * (a - c) / curve)
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    let at = |r: usize, c: usize| scores.get(r * side + c).copied();
    let sub_x = refine(
        column.checked_sub(1).and_then(|c| at(row, c)),
        (column + 1 < side).then(|| at(row, column + 1)).flatten(),
    );
    let sub_y = refine(
        row.checked_sub(1).and_then(|r| at(r, column)),
        (row + 1 < side).then(|| at(row + 1, column)).flatten(),
    );
    let radius = f64::from(u32::try_from(radius).unwrap_or(0));
    let whole = |index: usize| f64::from(u32::try_from(index).unwrap_or(0)) - radius;
    [whole(column) + sub_x, whole(row) + sub_y]
}
