//! Why a check image or an alignment verdict could not be made.

/// Which property of a frame differs from the calibration's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameField {
    /// Its size in pixels.
    Resolution,
    /// The snapshot URL it came from.
    Stream,
}

impl FrameField {
    /// The field's name, as a refusal names it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resolution => "resolution",
            Self::Stream => "stream",
        }
    }
}

/// Why nothing was drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum ComposeError {
    /// The frame is not the calibration's: another size, or another stream.
    /// It is refused, never scaled.
    FrameMismatch {
        /// What differs.
        field: FrameField,
        /// The calibration's.
        expected: String,
        /// The frame's.
        got: String,
    },
    /// At least two visible landmarks have moved past the calibration's
    /// tolerance: the camera has moved since it was calibrated.
    CameraMoved {
        /// The landmark that moved furthest.
        landmark: String,
        /// How far it moved, in pixels.
        offset_px: f64,
    },
    /// Fewer than two landmarks are visible, so whether the camera has moved
    /// cannot be told.
    DriftUnverified,
    /// No point of the geometry to draw lands in the frame.
    OutlineOffFrame,
    /// A frame is not a JPEG this crate can decode.
    Undecodable {
        /// What the decoder said.
        detail: String,
    },
    /// The check image could not be encoded.
    Encode {
        /// What the encoder said.
        detail: String,
    },
}

impl core::fmt::Display for ComposeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FrameMismatch {
                field,
                expected,
                got,
            } => write!(
                formatter,
                "the frame's {} is {got}, and the calibration's is {expected}",
                field.as_str()
            ),
            Self::CameraMoved {
                landmark,
                offset_px,
            } => write!(
                formatter,
                "the camera has moved since it was calibrated: {landmark} is {offset_px:.1} px from where it was"
            ),
            Self::DriftUnverified => formatter.write_str(
                "fewer than two of the calibration's landmarks are visible, so whether the camera has moved is unknown",
            ),
            Self::OutlineOffFrame => {
                formatter.write_str("nothing of the planned geometry lands in the frame")
            }
            Self::Undecodable { detail } => write!(formatter, "the frame is not a JPEG: {detail}"),
            Self::Encode { detail } => write!(formatter, "the check image: {detail}"),
        }
    }
}

impl std::error::Error for ComposeError {}
