//! A check's image of a whole camera frame: the frame scaled whole to fit
//! [`WHOLE_FRAME_WIDTH`] by [`WHOLE_FRAME_HEIGHT`], never cropped and never
//! enlarged.
//!
//! Every call of a supervision turn re-sends the images it holds, so a check is
//! shown a frame no larger than the agent needs to judge it, while the stored
//! frame keeps the size the camera took it at.

use image::imageops::FilterType;

use crate::compose::decode;
use crate::error::ComposeError;

/// The widest a whole-frame check image is.
pub const WHOLE_FRAME_WIDTH: u32 = 1280;
/// The tallest a whole-frame check image is.
pub const WHOLE_FRAME_HEIGHT: u32 = 720;

/// How finely the scaled frame is encoded.
const QUALITY: u8 = 90;

/// One whole frame, scaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WholeFrame {
    /// The JPEG.
    pub jpeg: Vec<u8>,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// Whether it was scaled: a frame that fits already is its own bytes.
    pub scaled: bool,
}

/// The size a `width` by `height` frame is scaled to: the largest that fits
/// the bound with its aspect kept, and the frame's own when it fits already.
#[must_use]
pub fn scaled_size(width: u32, height: u32) -> (u32, u32) {
    if width <= WHOLE_FRAME_WIDTH && height <= WHOLE_FRAME_HEIGHT {
        return (width, height);
    }
    let (wide, tall) = (u64::from(width), u64::from(height));
    let (bound_wide, bound_tall) = (u64::from(WHOLE_FRAME_WIDTH), u64::from(WHOLE_FRAME_HEIGHT));
    // Whichever side the bound is tighter on sets the scale; the other side is
    // rounded to the nearest pixel and is never zero.
    let fitted = if wide * bound_tall >= tall * bound_wide {
        (bound_wide, (tall * bound_wide + wide / 2) / wide)
    } else {
        ((wide * bound_tall + tall / 2) / tall, bound_tall)
    };
    let narrow = |side: u64| u32::try_from(side.max(1)).unwrap_or(u32::MAX);
    (narrow(fitted.0), narrow(fitted.1))
}

/// Scale one JPEG frame whole to the check's bound.
///
/// # Errors
///
/// Returns [`ComposeError::Undecodable`] for bytes that are not a JPEG, and
/// [`ComposeError::Encode`] when the scaled frame cannot be encoded.
pub fn whole_frame(jpeg: &[u8]) -> Result<WholeFrame, ComposeError> {
    let frame = decode(jpeg)?;
    let (width, height) = scaled_size(frame.width(), frame.height());
    if (width, height) == frame.dimensions() {
        return Ok(WholeFrame {
            jpeg: jpeg.to_vec(),
            width,
            height,
            scaled: false,
        });
    }
    let scaled = image::imageops::resize(&frame, width, height, FilterType::Triangle);
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, QUALITY)
        .encode_image(&scaled)
        .map_err(|error| ComposeError::Encode {
            detail: error.to_string(),
        })?;
    Ok(WholeFrame {
        jpeg: encoded,
        width,
        height,
        scaled: true,
    })
}

#[cfg(test)]
mod tests {
    use super::scaled_size;

    #[test]
    fn a_frame_is_fitted_whole_and_never_enlarged() {
        assert_eq!(scaled_size(1920, 1080), (1280, 720));
        assert_eq!(scaled_size(2560, 1440), (1280, 720));
        assert_eq!(scaled_size(1600, 1200), (960, 720));
        assert_eq!(scaled_size(2000, 500), (1280, 320));
        assert_eq!(scaled_size(640, 480), (640, 480));
        assert_eq!(scaled_size(1280, 720), (1280, 720));
    }
}
