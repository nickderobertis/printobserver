//! Whether the parts the current layer lays have grown since an earlier frame,
//! measured rather than judged by eye.
//!
//! On a bed-z printer the layer being laid always sits at the nozzle's plane,
//! so a part growing as planned keeps its top where the camera saw the top of
//! an earlier frame, while the bed and everything left on it travel down
//! under it. A part that stopped growing — extrusion stopped while the
//! printer kept climbing, printing air — travels down with the bed instead.
//! Its top in this frame sits where the earlier frame's top would be once
//! that frame was lowered by the plan's rise since. For a rise of a few
//! millimetres that is about ten pixels, at the edge of what an eye reads off
//! one drawn outline, and well inside what a correlation tells apart.
//!
//! So every object segment of the current layer is sampled every
//! [`SAMPLE_MM`] in bed millimetres, and at each sample two grey patches are
//! compared by normalised cross-correlation, once under each hypothesis:
//! **held**, the earlier and the current frame each at the sample's place on
//! the nozzle's plane; and **carried**, the earlier frame there and the
//! current frame where that place has travelled with the bed since. A patch
//! of near-uniform grey in both frames says nothing and is skipped. The
//! verdict is [`Growth::not_grown`] only when the carried correlation's median
//! is high and most samples prefer carried to held. Anything else, such as a
//! gantry across the parts, a rise too small to separate the hypotheses or
//! too few samples, is no verdict, which leaves the agent to judge as before.

use image::GrayImage;
use printobserver_core::LayerToolpath;

use crate::calibration::CameraCalibration;
use crate::compose::decode;
use crate::drift::check_frame;
use crate::error::ComposeError;

/// The least rise of the plan a measurement is made over, in millimetres:
/// the earlier frame is one at least this far below the current one. Over
/// the recorded cases a rise of 3 mm moves a carried part about 8 px, past
/// [`MIN_SEPARATION_PX`].
pub const MIN_RISE_MM: f64 = 3.0;

/// How far apart samples lie along a segment, in bed millimetres.
pub const SAMPLE_MM: f64 = 1.5;

/// The most samples a layer is measured at, taken evenly along its segments.
const MAX_SAMPLES: usize = 1500;

/// Half a patch's side, in pixels: patches are 13 px square.
const HALF_PATCH: i64 = 6;

/// The grey standard deviation below which a patch carries no texture.
const FEATURELESS_GREY: f64 = 5.0;

/// The grey blur the frames are read through, in pixels.
const BLUR_SIGMA: f32 = 1.0;

/// The fewest compared samples a verdict is drawn from.
pub const MIN_SAMPLES: usize = 50;

/// The least median distance, in pixels, between where the two hypotheses
/// put a sample in the current frame: closer than this, the patches overlap
/// too much to tell carried from held.
pub const MIN_SEPARATION_PX: f64 = 6.0;

/// The least median carried correlation a "not grown" verdict needs. Over
/// every recorded case's frames of healthy growth, compared with an earlier
/// frame at least this far below, the median never passed 0.26; over the air
/// print's frames once the parts stopped growing it lay between 0.55 and
/// 0.96 wherever no gantry crossed the parts.
pub const NOT_GROWN_CORRELATION: f64 = 0.45;

/// The least share of samples that must correlate better carried than held.
pub const NOT_GROWN_SHARE: f64 = 0.7;

/// What the measurement is given.
#[derive(Debug, Clone, Copy)]
pub struct GrowthInputs<'a> {
    /// An earlier frame of the same print.
    pub earlier_jpeg: &'a [u8],
    /// The print's height when the earlier frame was captured, in millimetres.
    pub earlier_print_z_mm: f64,
    /// The frame being checked.
    pub current_jpeg: &'a [u8],
    /// The print's height now, in millimetres.
    pub current_print_z_mm: f64,
    /// The snapshot URL both frames came from.
    pub camera_snapshot_url: &'a str,
    /// The current layer's toolpath.
    pub toolpath: &'a LayerToolpath,
    /// The printer's calibration.
    pub calibration: &'a CameraCalibration,
}

/// What the measurement found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Growth {
    /// How many samples were compared.
    pub samples: usize,
    /// The median distance between the two hypotheses' places in the current
    /// frame, in pixels.
    pub separation_px: f64,
    /// The median correlation with the parts held at the nozzle's plane.
    pub held: f64,
    /// The median correlation with the parts carried down with the bed.
    pub carried: f64,
    /// The share of samples correlating better carried than held.
    pub carried_share: f64,
}

impl Growth {
    /// Whether the parts this layer lays sit in the current frame where the
    /// earlier frame had them, carried down with the bed: nothing has been
    /// laid on them since.
    #[must_use]
    pub fn not_grown(&self) -> bool {
        self.samples >= MIN_SAMPLES
            && self.separation_px >= MIN_SEPARATION_PX
            && self.carried >= NOT_GROWN_CORRELATION
            && self.carried_share >= NOT_GROWN_SHARE
    }
}

/// Measure whether the parts the current layer lays grew since the earlier
/// frame. `None` when fewer than [`MIN_SAMPLES`] samples could be compared.
///
/// # Errors
///
/// [`ComposeError::Undecodable`] for a frame that is no JPEG, and
/// [`ComposeError::FrameMismatch`] for one of another size or stream than the
/// calibration's.
pub fn measure_growth(inputs: &GrowthInputs<'_>) -> Result<Option<Growth>, ComposeError> {
    let earlier = grey(inputs.earlier_jpeg, inputs)?;
    let current = grey(inputs.current_jpeg, inputs)?;
    let projector = inputs.calibration.projector();
    let at_nozzle = f64::from(inputs.toolpath.z_mm) - inputs.current_print_z_mm;
    let travelled = inputs.earlier_print_z_mm - inputs.current_print_z_mm;
    let mut held = Vec::new();
    let mut carried = Vec::new();
    let mut separations = Vec::new();
    for [x, y] in samples(inputs.toolpath) {
        let (Some(here), Some(lowered)) = (
            projector.pixel([x, y, at_nozzle]),
            projector.pixel([x, y, at_nozzle + travelled]),
        ) else {
            continue;
        };
        let (Some(then), Some(now_held), Some(now_carried)) = (
            patch(&earlier, here),
            patch(&current, here),
            patch(&current, lowered),
        ) else {
            continue;
        };
        if featureless(&then) && featureless(&now_held) && featureless(&now_carried) {
            continue;
        }
        let (Some(held_at), Some(carried_at)) = (
            correlation(&then, &now_held),
            correlation(&then, &now_carried),
        ) else {
            continue;
        };
        held.push(held_at);
        carried.push(carried_at);
        separations.push((lowered[0] - here[0]).hypot(lowered[1] - here[1]));
    }
    if held.len() < MIN_SAMPLES {
        return Ok(None);
    }
    let preferring = held
        .iter()
        .zip(&carried)
        .filter(|(held, carried)| carried > held)
        .count();
    let carried_share = ratio(preferring, held.len());
    Ok(Some(Growth {
        samples: held.len(),
        separation_px: median(&mut separations),
        held: median(&mut held),
        carried: median(&mut carried),
        carried_share,
    }))
}

/// A frame, refused unless it is the calibration's, as blurred grey.
fn grey(jpeg: &[u8], inputs: &GrowthInputs<'_>) -> Result<GrayImage, ComposeError> {
    let frame = decode(jpeg)?;
    check_frame(
        inputs.calibration,
        [frame.width(), frame.height()],
        inputs.camera_snapshot_url,
    )?;
    Ok(imageproc::filter::gaussian_blur_f32(
        &image::imageops::grayscale(&frame),
        BLUR_SIGMA,
    ))
}

/// Points every [`SAMPLE_MM`] along every segment that lays the part, at most
/// [`MAX_SAMPLES`] of them, taken evenly.
fn samples(toolpath: &LayerToolpath) -> Vec<[f64; 2]> {
    let mut points = Vec::new();
    for segment in toolpath
        .segments
        .iter()
        .filter(|segment| !segment.kind.is_support())
    {
        let from = segment.from_mm.map(f64::from);
        let to = segment.to_mm.map(f64::from);
        let steps = ((to[0] - from[0]).hypot(to[1] - from[1]) / SAMPLE_MM)
            .floor()
            .clamp(1.0, 4096.0);
        let mut k = 0.0;
        while k < steps {
            let t = k / steps;
            points.push([
                from[0] + t * (to[0] - from[0]),
                from[1] + t * (to[1] - from[1]),
            ]);
            k += 1.0;
        }
    }
    if points.len() <= MAX_SAMPLES {
        return points;
    }
    let last = points.len() - 1;
    (0..MAX_SAMPLES)
        .map(|k| points[k * last / (MAX_SAMPLES - 1)])
        .collect()
}

/// The grey patch centred on a pixel, or none when it would leave the frame.
fn patch(frame: &GrayImage, at: [f64; 2]) -> Option<Vec<f64>> {
    let (width, height) = (i64::from(frame.width()), i64::from(frame.height()));
    if !(at[0].is_finite() && at[1].is_finite()) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a finite pixel coordinate, bounded below before it is used"
    )]
    let (x, y) = (at[0].round() as i64, at[1].round() as i64);
    if x < HALF_PATCH || y < HALF_PATCH || x >= width - HALF_PATCH || y >= height - HALF_PATCH {
        return None;
    }
    let mut values = Vec::with_capacity(169);
    for row in (y - HALF_PATCH)..=(y + HALF_PATCH) {
        for column in (x - HALF_PATCH)..=(x + HALF_PATCH) {
            let (Ok(column), Ok(row)) = (u32::try_from(column), u32::try_from(row)) else {
                return None;
            };
            values.push(f64::from(frame.get_pixel(column, row).0[0]));
        }
    }
    Some(values)
}

/// Whether a patch's grey barely varies.
fn featureless(values: &[f64]) -> bool {
    moments(values).1 < FEATURELESS_GREY
}

/// A patch's mean and standard deviation.
fn moments(values: &[f64]) -> (f64, f64) {
    let count = ratio(values.len(), 1);
    let mean = values.iter().sum::<f64>() / count;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count;
    (mean, variance.sqrt())
}

/// The normalised cross-correlation of two patches, or none when either is
/// perfectly flat.
fn correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    let (mean_a, _) = moments(a);
    let (mean_b, _) = moments(b);
    let mut product = 0.0;
    let mut square_a = 0.0;
    let mut square_b = 0.0;
    for (a, b) in a.iter().zip(b) {
        let (a, b) = (a - mean_a, b - mean_b);
        product += a * b;
        square_a += a * a;
        square_b += b * b;
    }
    let scale = (square_a * square_b).sqrt();
    (scale > 1e-9).then(|| product / scale)
}

/// One count over another, each at most [`MAX_SAMPLES`] or a patch's 169
/// values, so each is exact as a `u32` and as an `f64`.
fn ratio(count: usize, of: usize) -> f64 {
    let exact = |count: usize| f64::from(u32::try_from(count).unwrap_or(u32::MAX));
    exact(count) / exact(of)
}

/// The median of values, which it sorts.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
