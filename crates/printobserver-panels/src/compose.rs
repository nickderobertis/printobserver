//! The check image: three 1024x576 panels stacked, each under a 40 px label
//! band, as one JPEG.
//!
//! 1. The current frame scaled whole, the current layer's planned toolpath
//!    drawn over it 1 px wide in each segment's kind's colour — and, when that
//!    layer lays supports alone, the nearest object layer below it in grey
//!    beneath, so the part still shows. Lines are 1 px because 2 px merged
//!    wall and infill at this size. Infill is not drawn ([`is_drawn`]): its
//!    dense hatch over the part's interior read as material being laid, and
//!    hid whether that interior holds the part or bare sheet, which is what
//!    tells a part off its place.
//! 2. The current frame cropped to the active object's outline projected at
//!    the bed and at the current height, with a [`MARGIN_PX`] margin, widened
//!    to 16:9, never narrower than [`MIN_CROP_WIDTH_PX`] (so upscaled at most
//!    2x), clamped inside the frame; nothing drawn.
//! 3. The previous check's frame scaled whole, nothing drawn, or a
//!    placeholder on the print's first check.

use image::{Rgb, RgbImage, imageops::FilterType};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut};
use imageproc::rect::Rect;
use printobserver_core::{ExtrusionKind, LayerToolpath, ObjectOutline};
use printobserver_types::Timestamp;

use crate::calibration::CameraCalibration;
use crate::drift::{check_drift, check_frame};
use crate::error::ComposeError;
use crate::geometry::project_toolpath;
use crate::text::draw_text;

/// A panel's width.
pub const PANEL_WIDTH: u32 = 1024;
/// A panel's height.
pub const PANEL_HEIGHT: u32 = 576;
/// The label band above each panel.
pub const BAND_HEIGHT: u32 = 40;
/// The whole image's height: three bands and three panels.
pub const IMAGE_HEIGHT: u32 = 3 * (BAND_HEIGHT + PANEL_HEIGHT);
/// The close-up's margin around the active object, in source pixels.
pub const MARGIN_PX: f64 = 48.0;
/// The close-up's least width, in source pixels.
pub const MIN_CROP_WIDTH_PX: f64 = 512.0;
/// The JPEG quality the image is written at.
const QUALITY: u8 = 90;
/// What panel 3 reads on a print's first check.
pub const NO_EARLIER_CHECK: &str = "no earlier check this print";

/// The grey the nearest object layer below a support-only layer is drawn in.
pub const BELOW_GREY: Rgb<u8> = Rgb([150, 150, 150]);
/// The label bands' background.
const BAND: Rgb<u8> = Rgb([24, 24, 24]);
/// The label text.
const TEXT: Rgb<u8> = Rgb([235, 235, 235]);

/// The colour a segment of each kind is drawn in.
#[must_use]
pub const fn colour_of(kind: ExtrusionKind) -> Rgb<u8> {
    match kind {
        ExtrusionKind::Wall => Rgb([255, 40, 40]),
        ExtrusionKind::Infill => Rgb([255, 220, 0]),
        ExtrusionKind::Support => Rgb([0, 200, 255]),
        ExtrusionKind::Bridge => Rgb([255, 0, 255]),
        ExtrusionKind::Overhang => Rgb([0, 230, 0]),
        ExtrusionKind::Other => Rgb([255, 255, 255]),
    }
}

/// Whether panel 1 draws a segment of this kind: every kind but infill.
#[must_use]
pub const fn is_drawn(kind: ExtrusionKind) -> bool {
    !matches!(kind, ExtrusionKind::Infill)
}

/// The colour key panel 1's band carries, in its order: each kind it draws.
const KEY: [(&str, ExtrusionKind); 4] = [
    ("wall", ExtrusionKind::Wall),
    ("support", ExtrusionKind::Support),
    ("bridge", ExtrusionKind::Bridge),
    ("overhang", ExtrusionKind::Overhang),
];

/// What the check image is composed from.
#[derive(Debug, Clone, Copy)]
pub struct CheckImageInputs<'a> {
    /// The frame this check was given.
    pub current_jpeg: &'a [u8],
    /// When it was captured. Panel 1 is always the frame being checked, so
    /// its label names no time; the field is the one the previous frame's
    /// label is read against by the check that keeps both.
    pub current_at: Timestamp,
    /// The snapshot URL it came from.
    pub camera_snapshot_url: &'a str,
    /// The previous check's frame and when it was captured; absent on the
    /// print's first check.
    pub previous: Option<(&'a [u8], Timestamp)>,
    /// Every object's outline.
    pub outlines: &'a [ObjectOutline],
    /// The object being laid, an index into `outlines`.
    pub active_object: usize,
    /// The current layer's toolpath.
    pub toolpath: &'a LayerToolpath,
    /// The nearest object layer below, given when `toolpath` lays supports
    /// alone.
    pub object_layer_below: Option<&'a LayerToolpath>,
    /// The print's height, in millimetres.
    pub print_z_mm: f64,
    /// The printer's calibration.
    pub calibration: &'a CameraCalibration,
}

/// The composed check image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckImage {
    /// The JPEG.
    pub jpeg: Vec<u8>,
    /// Its width: [`PANEL_WIDTH`].
    pub width: u32,
    /// Its height: [`IMAGE_HEIGHT`].
    pub height: u32,
    /// Panel 2's source, `[x, y, w, h]` in current-frame pixels.
    pub close_up_source_rect: [u32; 4],
}

/// Decode a JPEG frame.
pub(crate) fn decode(jpeg: &[u8]) -> Result<RgbImage, ComposeError> {
    image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .map(|frame| frame.to_rgb8())
        .map_err(|error| ComposeError::Undecodable {
            detail: error.to_string(),
        })
}

/// Compose the check image.
///
/// # Errors
///
/// In the order they are checked: [`ComposeError::Undecodable`] for a frame
/// that is no JPEG; [`ComposeError::FrameMismatch`] for a current or previous
/// frame of another size, or a stream, other than the calibration's;
/// [`ComposeError::CameraMoved`] or [`ComposeError::DriftUnverified`] from
/// the drift check; and
/// [`ComposeError::OutlineOffFrame`] when neither the toolpath nor the active
/// object lands in the frame.
pub fn compose_check_image(inputs: &CheckImageInputs<'_>) -> Result<CheckImage, ComposeError> {
    let current = decode(inputs.current_jpeg)?;
    let size = [current.width(), current.height()];
    check_frame(inputs.calibration, size, inputs.camera_snapshot_url)?;
    check_drift(inputs.calibration, &image::imageops::grayscale(&current))?;
    let previous = inputs
        .previous
        .map(|(jpeg, at)| decode(jpeg).map(|frame| (frame, at)))
        .transpose()?;
    if let Some((frame, _)) = &previous {
        // Panel 3 is scaled by the current frame's factors, so a previous
        // frame of another size would be drawn distorted rather than refused.
        check_frame(
            inputs.calibration,
            [frame.width(), frame.height()],
            inputs.camera_snapshot_url,
        )?;
    }

    let projector = inputs.calibration.projector();
    let drawn = project_toolpath(&projector, inputs.toolpath, inputs.print_z_mm);
    let below = inputs
        .object_layer_below
        .map(|layer| project_toolpath(&projector, layer, inputs.print_z_mm))
        .unwrap_or_default();
    let rect = close_up_rect(inputs, size);
    let in_frame = |[x, y]: [f64; 2]| {
        (0.0..f64::from(size[0])).contains(&x) && (0.0..f64::from(size[1])).contains(&y)
    };
    if rect.is_none() && !drawn.iter().chain(&below).any(|piece| in_frame(piece.from)) {
        return Err(ComposeError::OutlineOffFrame);
    }
    let rect = rect.unwrap_or_else(|| whole_width_rect(size));

    let mut image = RgbImage::from_pixel(PANEL_WIDTH, IMAGE_HEIGHT, BAND);
    let scale = f64::from(PANEL_WIDTH) / f64::from(size[0]);
    let scale_y = f64::from(PANEL_HEIGHT) / f64::from(size[1]);

    let mut whole = scaled(&current, None);
    for piece in below.iter().filter(|piece| is_drawn(piece.kind)) {
        line(&mut whole, piece.from, piece.to, scale, scale_y, BELOW_GREY);
    }
    for piece in drawn.iter().filter(|piece| is_drawn(piece.kind)) {
        line(
            &mut whole,
            piece.from,
            piece.to,
            scale,
            scale_y,
            colour_of(piece.kind),
        );
    }
    paste(&mut image, &whole, 0);
    band(&mut image, 0, "1 NOW: planned toolpath of this layer", true);

    paste(&mut image, &scaled(&current, Some(rect)), 1);
    band(&mut image, 1, "2 NOW: close-up, nothing drawn", false);

    let third = band_top(2) + BAND_HEIGHT;
    if let Some((frame, at)) = &previous {
        paste(&mut image, &scaled(frame, None), 2);
        let local = at.as_utc().with_timezone(&chrono::Local).format("%H:%M:%S");
        band(
            &mut image,
            2,
            &format!("3 PREVIOUS CHECK {local}: whole view, nothing drawn"),
            false,
        );
    } else {
        draw_filled_rect_mut(
            &mut image,
            Rect::at(0, signed(third)).of_size(PANEL_WIDTH, PANEL_HEIGHT),
            Rgb([48, 48, 48]),
        );
        draw_text(
            &mut image,
            TEXT,
            40,
            signed(third + PANEL_HEIGHT / 2 - 16),
            32.0,
            NO_EARLIER_CHECK,
        );
        band(&mut image, 2, "3 PREVIOUS CHECK: none", false);
    }

    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, QUALITY)
        .encode_image(&image)
        .map_err(|error| ComposeError::Encode {
            detail: error.to_string(),
        })?;
    Ok(CheckImage {
        jpeg,
        width: PANEL_WIDTH,
        height: IMAGE_HEIGHT,
        close_up_source_rect: rect,
    })
}

/// A pixel row as the drawing calls take it.
fn signed(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Where panel `n`'s band starts.
const fn band_top(panel: u32) -> u32 {
    panel * (BAND_HEIGHT + PANEL_HEIGHT)
}

/// Copy a panel under its band.
fn paste(image: &mut RgbImage, panel: &RgbImage, n: u32) {
    image::imageops::replace(image, panel, 0, i64::from(band_top(n) + BAND_HEIGHT));
}

/// A frame, or a rectangle of it, scaled to one panel.
fn scaled(frame: &RgbImage, rect: Option<[u32; 4]>) -> RgbImage {
    let source = match rect {
        Some([x, y, w, h]) => image::imageops::crop_imm(frame, x, y, w, h).to_image(),
        None => frame.clone(),
    };
    image::imageops::resize(&source, PANEL_WIDTH, PANEL_HEIGHT, FilterType::Triangle)
}

/// Draw one projected piece, 1 px wide, scaled into a panel.
fn line(panel: &mut RgbImage, from: [f64; 2], to: [f64; 2], sx: f64, sy: f64, colour: Rgb<u8>) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a projected pixel is a few thousand at most; one far off the panel is clipped by the drawing call"
    )]
    let at = |[x, y]: [f64; 2]| ((x * sx) as f32, (y * sy) as f32);
    draw_line_segment_mut(panel, at(from), at(to), colour);
}

/// One label band: its text, and for panel 1 the colour key.
fn band(image: &mut RgbImage, n: u32, text: &str, key: bool) {
    let top = signed(band_top(n));
    draw_text(image, TEXT, 8, top + 9, 20.0, text);
    if !key {
        return;
    }
    let mut x = 520;
    for (name, kind) in KEY {
        draw_filled_rect_mut(
            image,
            Rect::at(x, top + 14).of_size(12, 12),
            colour_of(kind),
        );
        draw_text(image, TEXT, x + 16, top + 11, 16.0, name);
        x += 16 + 10 * i32::try_from(name.len()).unwrap_or(0) + 18;
    }
}

/// The whole frame's width at 16:9, centred: what panel 2 shows when the
/// active object lands nowhere in the frame.
fn whole_width_rect(size: [u32; 2]) -> [u32; 4] {
    fit_rect([0.0, 0.0, f64::from(size[0]), f64::from(size[1])], size)
}

/// The close-up's source rectangle: the box holding the active object's
/// outline projected at the bed and at the current height, with its margin,
/// widened to 16:9 and to the least width, clamped inside the frame. Nothing
/// when no vertex of the outline projects at all.
fn close_up_rect(inputs: &CheckImageInputs<'_>, size: [u32; 2]) -> Option<[u32; 4]> {
    let outline = inputs.outlines.get(inputs.active_object)?;
    let projector = inputs.calibration.projector();
    let points: Vec<[f64; 2]> = outline
        .hull_mm
        .iter()
        .flat_map(|vertex| {
            let (x, y) = (f64::from(vertex[0]), f64::from(vertex[1]));
            [
                projector.pixel([x, y, -inputs.print_z_mm]),
                projector.pixel([x, y, 0.0]),
            ]
        })
        .flatten()
        .collect();
    let first = points.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first[0], first[1], first[0], first[1]);
    for [x, y] in &points {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    let (w, h) = (f64::from(size[0]), f64::from(size[1]));
    if x1 < 0.0 || y1 < 0.0 || x0 >= w || y0 >= h {
        return None;
    }
    Some(fit_rect(
        [
            x0 - MARGIN_PX,
            y0 - MARGIN_PX,
            x1 + MARGIN_PX,
            y1 + MARGIN_PX,
        ],
        size,
    ))
}

/// A box `[x0, y0, x1, y1]` widened about its centre to 16:9 and to the least
/// width, then moved — and only if it must, shrunk at 16:9 — inside the frame.
fn fit_rect(bounds: [f64; 4], size: [u32; 2]) -> [u32; 4] {
    let (frame_w, frame_h) = (f64::from(size[0]), f64::from(size[1]));
    let centre = [
        f64::midpoint(bounds[0], bounds[2]),
        f64::midpoint(bounds[1], bounds[3]),
    ];
    let mut width = (bounds[2] - bounds[0])
        .max((bounds[3] - bounds[1]) * 16.0 / 9.0)
        .max(MIN_CROP_WIDTH_PX);
    width = width.min(frame_w).min(frame_h * 16.0 / 9.0);
    let height = width * 9.0 / 16.0;
    let x = (centre[0] - width / 2.0).clamp(0.0, frame_w - width);
    let y = (centre[1] - height / 2.0).clamp(0.0, frame_h - height);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "each is clamped inside the frame, whose size fits a u32"
    )]
    let whole = |value: f64| value.round() as u32;
    let (rx, ry) = (whole(x), whole(y));
    let rw = whole(width).min(size[0] - rx);
    let rh = whole(height).min(size[1] - ry);
    [rx, ry, rw, rh]
}
