//! The planned geometry in the camera's view: toolpath segments projected at
//! a print height, and points sampled along them.

use printobserver_core::{ExtrusionKind, LayerToolpath};

use crate::calibration::Projector;

/// The longest piece a segment is projected as, in millimetres: a straight
/// move is a curve in a fisheye view, so a long one is cut before projecting.
const PIECE_MM: f64 = 2.0;

/// The longest projected piece kept, in pixels: longer than any frame's
/// diagonal, so a piece this long cannot be drawn or sampled usefully, and
/// keeping it would make both walk it pixel by pixel.
const MAX_PIECE_PX: f64 = 32_768.0;

/// One projected piece of a toolpath, in the calibration's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Projected {
    /// Where it starts.
    pub(crate) from: [f64; 2],
    /// Where it ends.
    pub(crate) to: [f64; 2],
    /// What it lays.
    pub(crate) kind: ExtrusionKind,
}

/// Every segment of a layer, projected with the print at `print_z_mm`, each
/// cut into pieces of at most [`PIECE_MM`]; a piece behind the camera, or one
/// projecting to a non-finite point or longer than [`MAX_PIECE_PX`], is left
/// out.
pub(crate) fn project_toolpath(
    projector: &Projector,
    toolpath: &LayerToolpath,
    print_z_mm: f64,
) -> Vec<Projected> {
    let z = f64::from(toolpath.z_mm) - print_z_mm;
    let mut out = Vec::with_capacity(toolpath.segments.len());
    for segment in &toolpath.segments {
        let from = segment.from_mm.map(f64::from);
        let to = segment.to_mm.map(f64::from);
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let pieces = (length / PIECE_MM).ceil().clamp(1.0, 1024.0);
        let mut previous = projector.pixel([from[0], from[1], z]);
        let mut k = 1.0;
        while k <= pieces {
            let t = k / pieces;
            let next = projector.pixel([
                from[0] + t * (to[0] - from[0]),
                from[1] + t * (to[1] - from[1]),
                z,
            ]);
            if let (Some(a), Some(b)) = (previous, next) {
                let length = (b[0] - a[0]).hypot(b[1] - a[1]);
                if length.is_finite() && length <= MAX_PIECE_PX {
                    out.push(Projected {
                        from: a,
                        to: b,
                        kind: segment.kind,
                    });
                }
            }
            previous = next;
            k += 1.0;
        }
    }
    out
}

/// Points every `step` pixels along projected pieces, each piece's start
/// included and its end left to the next.
pub(crate) fn sample_along(pieces: &[Projected], step: f64) -> Vec<[f64; 2]> {
    let mut points = Vec::new();
    for piece in pieces {
        let length = (piece.to[0] - piece.from[0]).hypot(piece.to[1] - piece.from[1]);
        let count = (length / step).floor().max(1.0);
        let mut k = 0.0;
        while k < count {
            let t = k / count;
            points.push([
                piece.from[0] + t * (piece.to[0] - piece.from[0]),
                piece.from[1] + t * (piece.to[1] - piece.from[1]),
            ]);
            k += 1.0;
        }
    }
    points
}

/// The convex hull of points, counter-clockwise in image coordinates (y
/// down), by Andrew's monotone chain.
pub(crate) fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted: Vec<[f64; 2]> = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(2 * sorted.len());
    for pass in [false, true] {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &[f64; 2]>> = if pass {
            Box::new(sorted.iter().rev())
        } else {
            Box::new(sorted.iter())
        };
        for point in ordered {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], *point) <= 0.0
            {
                hull.pop();
            }
            hull.push(*point);
        }
        hull.pop();
    }
    hull
}

/// Points every `step` pixels along a polygon's edges, each pushed `out`
/// pixels away from its centre along the edge's outward normal.
pub(crate) fn band_outside(hull: &[[f64; 2]], out: f64, step: f64) -> Vec<[f64; 2]> {
    if hull.len() < 2 {
        return Vec::new();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a hull holds a few hundred vertices"
    )]
    let count = hull.len() as f64;
    let centre = hull.iter().fold([0.0, 0.0], |sum, p| {
        [sum[0] + p[0] / count, sum[1] + p[1] / count]
    });
    let mut band = Vec::new();
    for (at, a) in hull.iter().enumerate() {
        let b = hull[(at + 1) % hull.len()];
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        if length < 1e-6 {
            continue;
        }
        let mut normal = [(b[1] - a[1]) / length, -(b[0] - a[0]) / length];
        let middle = [f64::midpoint(a[0], b[0]), f64::midpoint(a[1], b[1])];
        if (middle[0] - centre[0]) * normal[0] + (middle[1] - centre[1]) * normal[1] < 0.0 {
            normal = [-normal[0], -normal[1]];
        }
        let samples = (length / step).floor().max(1.0);
        let mut k = 0.0;
        while k < samples {
            let t = k / samples;
            band.push([
                a[0] + t * (b[0] - a[0]) + normal[0] * out,
                a[1] + t * (b[1] - a[1]) + normal[1] * out,
            ]);
            k += 1.0;
        }
    }
    band
}

#[cfg(test)]
mod tests {
    use printobserver_core::{ExtrusionKind, LayerToolpath, ToolpathSegment};
    use printobserver_types::serde_json::{self, Value};

    use super::{MAX_PIECE_PX, project_toolpath, sample_along};
    use crate::CameraCalibration;

    /// A projection that sends a 2 mm move across far more than any frame is
    /// dropped, so neither the drawing nor the sampling walks it.
    #[test]
    fn a_piece_projecting_wider_than_any_frame_is_left_out() {
        let committed =
            include_bytes!("../../../tests/real-prints/cameras/core-one-2026-09-27.json");
        let mut value: Value = serde_json::from_slice(committed).expect("JSON");
        let toolpath = LayerToolpath {
            z_mm: 0.2,
            segments: vec![ToolpathSegment {
                from_mm: [120.0, 100.0],
                to_mm: [122.0, 100.0],
                kind: ExtrusionKind::Wall,
            }],
        };
        let read = |value: &Value| {
            CameraCalibration::from_json(&serde_json::to_vec(value).expect("serialises"))
                .expect("a finite calibration is accepted")
        };
        let ordinary = project_toolpath(&read(&value).projector(), &toolpath, 0.2);
        assert!(!ordinary.is_empty(), "the committed lens keeps the move");
        value["intrinsics"]["fx"] = 1e15.into();
        value["intrinsics"]["fy"] = 1e15.into();
        let pieces = project_toolpath(&read(&value).projector(), &toolpath, 0.2);
        assert!(
            pieces.is_empty(),
            "pieces longer than {MAX_PIECE_PX} px are left out, not {pieces:?}"
        );
        assert!(sample_along(&pieces, 1.0).is_empty());
    }
}
