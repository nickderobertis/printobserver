//! Judging a camera frame as a picture of the print.
//!
//! Two faults hand an agent nothing while looking like a picture: a frame that
//! is not whole, and a frame of nothing. The first is caught by decoding the
//! JPEG **strictly** — a frame cut part-way is refused rather than padded out,
//! because a padded frame's missing rows are grey and its spread overlaps a
//! healthy one's. The second is caught by the spread of the decoded frame's
//! grey levels: a camera showing nothing is near-uniform, and a frame of a
//! print is not. The bound between them is
//! [`DEFAULT_BLANK_FRAME_MAX_STDDEV`] unless one is configured.

use printobserver_vision_api::{FetchedImage, FrameFault};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

/// The grey-level standard deviation at or below which a frame is blank.
///
/// Measured against 69 recorded frames of real prints, whose spread ran from
/// 66.11 to 81.37, and 25 uniform-grey frames with noise, which ran from 0.00
/// to 6.85: the midpoint, with a margin of 29.6 to each side.
pub const DEFAULT_BLANK_FRAME_MAX_STDDEV: f64 = 36.5;

/// The least blank-frame bound a configuration may name.
pub const MIN_BLANK_FRAME_MAX_STDDEV: f64 = 1.0;

/// The greatest blank-frame bound a configuration may name.
pub const MAX_BLANK_FRAME_MAX_STDDEV: f64 = 128.0;

/// A grey-level spread at or below which a frame is blank: a finite number
/// from [`MIN_BLANK_FRAME_MAX_STDDEV`] to [`MAX_BLANK_FRAME_MAX_STDDEV`], so a
/// bound that would judge every frame blank, or none, cannot be held.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct BlankFrameBound(f64);

impl BlankFrameBound {
    /// The bound used unless one is configured,
    /// [`DEFAULT_BLANK_FRAME_MAX_STDDEV`].
    pub const DEFAULT: Self = Self(DEFAULT_BLANK_FRAME_MAX_STDDEV);

    /// The bound `spread` names, when it is one a configuration may name.
    ///
    /// # Errors
    ///
    /// Answers the refused number for one that is not finite or lies outside
    /// [`MIN_BLANK_FRAME_MAX_STDDEV`] to [`MAX_BLANK_FRAME_MAX_STDDEV`].
    pub fn new(spread: f64) -> Result<Self, f64> {
        if (MIN_BLANK_FRAME_MAX_STDDEV..=MAX_BLANK_FRAME_MAX_STDDEV).contains(&spread) {
            Ok(Self(spread))
        } else {
            Err(spread)
        }
    }

    /// The spread, in grey levels.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

impl Default for BlankFrameBound {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Judge one frame: answered back when it decodes whole and its grey levels
/// spread wider than `bound`.
///
/// # Errors
///
/// Answers [`FrameFault::Undecodable`] for a frame a strict decode refuses, and
/// [`FrameFault::Blank`] for one whose spread is at or below the bound.
pub fn judge(image: FetchedImage, bound: BlankFrameBound) -> Result<FetchedImage, FrameFault> {
    let max_stddev = bound.value();
    let options = DecoderOptions::new_cmd()
        .set_strict_mode(true)
        .jpeg_set_out_colorspace(ColorSpace::Luma);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(image.bytes.as_slice()), options);
    let pixels = decoder.decode().map_err(|error| FrameFault::Undecodable {
        detail: format!("a strict JPEG decode refused it: {error:?}"),
    })?;
    let spread = stddev(&pixels).ok_or_else(|| FrameFault::Undecodable {
        detail: "it decoded to no pixels".to_owned(),
    })?;
    if spread <= max_stddev {
        return Err(FrameFault::Blank {
            detail: format!(
                "its grey levels spread {spread:.2}, at or below the blank-frame bound of \
                 {max_stddev}"
            ),
        });
    }
    Ok(image)
}

/// The standard deviation of grey levels, over every pixel; nothing for none.
fn stddev(pixels: &[u8]) -> Option<f64> {
    if pixels.is_empty() {
        return None;
    }
    // A frame's pixel count is far inside `u32`, and so exact as a double.
    let count = f64::from(u32::try_from(pixels.len()).ok()?);
    let mean = pixels.iter().map(|&pixel| f64::from(pixel)).sum::<f64>() / count;
    let variance = pixels
        .iter()
        .map(|&pixel| {
            let offset = f64::from(pixel) - mean;
            offset * offset
        })
        .sum::<f64>()
        / count;
    Some(variance.sqrt())
}

#[cfg(test)]
mod tests {
    use super::{
        BlankFrameBound, DEFAULT_BLANK_FRAME_MAX_STDDEV, MAX_BLANK_FRAME_MAX_STDDEV,
        MIN_BLANK_FRAME_MAX_STDDEV,
    };

    /// Both ends of the range are bounds, and nothing past them or not a
    /// number at all is.
    #[test]
    fn a_bound_is_a_finite_spread_inside_its_range() {
        for spread in [MIN_BLANK_FRAME_MAX_STDDEV, 36.5, MAX_BLANK_FRAME_MAX_STDDEV] {
            assert_eq!(
                BlankFrameBound::new(spread).map(BlankFrameBound::value),
                Ok(spread)
            );
        }
        for spread in [0.99, 128.01, -1.0, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(BlankFrameBound::new(spread), Err(spread));
        }
        assert!(BlankFrameBound::new(f64::NAN).is_err_and(f64::is_nan));
        assert_eq!(
            BlankFrameBound::default(),
            BlankFrameBound::new(DEFAULT_BLANK_FRAME_MAX_STDDEV).expect("the default is a bound")
        );
    }
}
