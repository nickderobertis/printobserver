//! The label bands' text, drawn from the bundled font's own outlines.
//!
//! `skrifa` reads each glyph's outline and `ab_glyph_rasterizer` turns it into
//! coverage, which is blended over the image in the text's colour. The usual
//! pairing, `imageproc`'s text drawing over `ab_glyph`, parses fonts with
//! `ttf-parser`, which is unmaintained (RUSTSEC-2026-0192).

use ab_glyph_rasterizer::{Point, Rasterizer, point};
use image::{Rgb, RgbImage};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider as _};

/// The bundled label font, Liberation Mono (SIL OFL 1.1; its licence is
/// `assets/LiberationMono-LICENSE.txt`).
const FONT: &[u8] = include_bytes!("../assets/LiberationMono-Regular.ttf");

/// A glyph's outline as straight, quadratic and cubic pieces, in pixels with
/// y down from the text's top.
#[derive(Default)]
struct Path {
    /// The pieces.
    pieces: Vec<Piece>,
    /// Where the pen is.
    at: Point,
    /// Where the current contour began.
    start: Point,
    /// The text's ascent, which y is measured down from.
    ascent: f32,
}

/// One piece of an outline.
enum Piece {
    /// A line.
    Line(Point, Point),
    /// A quadratic curve.
    Quad(Point, Point, Point),
    /// A cubic curve.
    Cubic(Point, Point, Point, Point),
}

impl Path {
    /// A font point, in pixels with y down.
    fn flip(&self, x: f32, y: f32) -> Point {
        point(x, self.ascent - y)
    }
}

impl OutlinePen for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = self.flip(x, y);
        self.start = self.at;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.flip(x, y);
        self.pieces.push(Piece::Line(self.at, to));
        self.at = to;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (control, to) = (self.flip(cx0, cy0), self.flip(x, y));
        self.pieces.push(Piece::Quad(self.at, control, to));
        self.at = to;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (first, second, to) = (self.flip(cx0, cy0), self.flip(cx1, cy1), self.flip(x, y));
        self.pieces.push(Piece::Cubic(self.at, first, second, to));
        self.at = to;
    }

    fn close(&mut self) {
        if self.at != self.start {
            self.pieces.push(Piece::Line(self.at, self.start));
        }
        self.at = self.start;
    }
}

/// Draw one line of text with its top-left at `(x, y)`, `size_px` high.
/// A character the font does not carry is skipped; nothing outside the image
/// is drawn.
pub(crate) fn draw_text(
    image: &mut RgbImage,
    colour: Rgb<u8>,
    x: i32,
    y: i32,
    size_px: f32,
    text: &str,
) {
    let Ok(font) = FontRef::new(FONT) else {
        return;
    };
    let size = Size::new(size_px);
    let location = LocationRef::default();
    let ascent = font.metrics(size, location).ascent;
    let charmap = font.charmap();
    let advances = font.glyph_metrics(size, location);
    let outlines = font.outline_glyphs();
    let mut pen_x = 0.0_f32;
    for character in text.chars() {
        let Some(glyph) = charmap.map(character) else {
            continue;
        };
        if let Some(outline) = outlines.get(glyph) {
            let mut path = Path {
                ascent,
                ..Path::default()
            };
            if outline
                .draw(DrawSettings::unhinted(size, location), &mut path)
                .is_ok()
            {
                blend(image, colour, &path, x, y, pen_x);
            }
        }
        pen_x += advances.advance_width(glyph).unwrap_or(size_px * 0.6);
    }
}

/// Rasterize one glyph's path and blend its coverage into the image.
fn blend(image: &mut RgbImage, colour: Rgb<u8>, path: &Path, x: i32, y: i32, pen_x: f32) {
    let corners = path.pieces.iter().flat_map(|piece| match piece {
        Piece::Line(from, to) => vec![*from, *to],
        Piece::Quad(from, control, to) => vec![*from, *control, *to],
        Piece::Cubic(from, first, second, to) => vec![*from, *first, *second, *to],
    });
    let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
    for corner in corners {
        left = left.min(corner.x);
        right = right.max(corner.x);
        top = top.min(corner.y);
        bottom = bottom.max(corner.y);
    }
    if !(left.is_finite() && top.is_finite()) {
        return;
    }
    let (left, top) = (left.floor(), top.floor());
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a glyph of a label is a few tens of pixels across"
    )]
    let (width, height) = (
        (right - left).ceil() as usize + 1,
        (bottom - top).ceil() as usize + 1,
    );
    let shift = |p: Point| point(p.x - left, p.y - top);
    let mut rasterizer = Rasterizer::new(width, height);
    for piece in &path.pieces {
        match piece {
            Piece::Line(from, to) => rasterizer.draw_line(shift(*from), shift(*to)),
            Piece::Quad(from, control, to) => {
                rasterizer.draw_quad(shift(*from), shift(*control), shift(*to));
            }
            Piece::Cubic(from, first, second, to) => {
                rasterizer.draw_cubic(shift(*from), shift(*first), shift(*second), shift(*to));
            }
        }
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a label's pixel offsets are a few hundred at most"
    )]
    let (origin_x, origin_y) = (x + (pen_x + left) as i32, y + top as i32);
    rasterizer.for_each_pixel_2d(|gx, gy, coverage| {
        let (Ok(px), Ok(py)) = (
            u32::try_from(origin_x + i32::try_from(gx).unwrap_or(i32::MAX)),
            u32::try_from(origin_y + i32::try_from(gy).unwrap_or(i32::MAX)),
        ) else {
            return;
        };
        if px >= image.width() || py >= image.height() {
            return;
        }
        let alpha = coverage.clamp(0.0, 1.0);
        let under = image.get_pixel_mut(px, py);
        for (channel, over) in under.0.iter_mut().zip(colour.0) {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a blend of two bytes stays a byte"
            )]
            let blended =
                (f32::from(*channel) * (1.0 - alpha) + f32::from(over) * alpha).round() as u8;
            *channel = blended;
        }
    });
}
