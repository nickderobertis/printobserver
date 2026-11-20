//! The planned geometry of a print, read from its G-code by the one read the
//! [`GcodeIndex`](crate::GcodeIndex) makes: each object's footprint outline,
//! and one layer's toolpath read back from the print's spool.
//!
//! The index's single streamed read of a print's file also copies its bytes
//! to a per-print spool file and records each layer's byte range, so a
//! toolpath is read from that range of the spool and never from a second read
//! of the printer's file. The core alone reads G-code; what leaves it is
//! segments in bed millimetres, each with the [`ExtrusionKind`] its
//! `;TYPE:` maps to.

use std::path::{Path, PathBuf};

use printobserver_types::PrintId;

/// How many directions an outline is bounded in: its most vertices.
pub const OUTLINE_DIRECTIONS: usize = 64;

/// What a toolpath segment lays, from `PrusaSlicer`'s `;TYPE:` label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtrusionKind {
    /// `External perimeter` and `Perimeter`.
    Wall,
    /// `Internal infill`, `Solid infill`, `Top solid infill` and `Gap fill`.
    Infill,
    /// `Support material` and `Support material interface`.
    Support,
    /// `Bridge infill`.
    Bridge,
    /// `Overhang perimeter`.
    Overhang,
    /// Anything else: the skirt or brim, the slicer's own start and end
    /// G-code, or an unlabelled move.
    Other,
}

impl ExtrusionKind {
    /// The kind a `;TYPE:` label names.
    #[must_use]
    pub fn of_type(name: &str) -> Self {
        match name {
            "External perimeter" | "Perimeter" => Self::Wall,
            "Internal infill" | "Solid infill" | "Top solid infill" | "Gap fill" => Self::Infill,
            "Support material" | "Support material interface" => Self::Support,
            "Bridge infill" => Self::Bridge,
            "Overhang perimeter" => Self::Overhang,
            _ => Self::Other,
        }
    }

    /// Whether this kind holds the part up rather than making it.
    #[must_use]
    pub const fn is_support(self) -> bool {
        matches!(self, Self::Support)
    }
}

/// One object's footprint: an outward bound of the convex hull of every
/// extrusion move laying it, in bed millimetres, counter-clockwise, with at
/// most [`OUTLINE_DIRECTIONS`] vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectOutline {
    /// The vertices, counter-clockwise.
    pub hull_mm: Vec<[f32; 2]>,
}

/// One layer of an index, as the index that answered it numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerRef(pub(crate) usize);

impl LayerRef {
    /// The layer at one place among a file's layers, the first being 0; an
    /// index answers [`SpoolError::UnknownLayer`] for one it does not have.
    #[must_use]
    pub const fn of_index(index: usize) -> Self {
        Self(index)
    }

    /// Its place among the file's layers, the first being 0.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// One extrusion move, or one chord of an arc, of a layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolpathSegment {
    /// Where it starts, in bed millimetres.
    pub from_mm: [f32; 2],
    /// Where it ends.
    pub to_mm: [f32; 2],
    /// What it lays.
    pub kind: ExtrusionKind,
}

/// Every extrusion move of one layer, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerToolpath {
    /// The layer's height, from its `;Z:`; 0 when the file states none.
    pub z_mm: f32,
    /// Its segments.
    pub segments: Vec<ToolpathSegment>,
}

impl LayerToolpath {
    /// Whether any segment is object extrusion: anything but supports, as
    /// `GcodeIndex::object_layer_at_or_below` reads it, so an `Other` segment
    /// (skirt, brim or custom G-code) counts.
    #[must_use]
    pub fn carries_object_extrusion(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| !segment.kind.is_support())
    }
}

/// Why a layer's toolpath could not be read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpoolError {
    /// The index was built with no spool beside it.
    NoSpool,
    /// The layer is not one of this index's.
    UnknownLayer,
    /// The spool could not be read, or is shorter than the index recorded.
    Io {
        /// What the filesystem said.
        detail: String,
    },
}

impl core::fmt::Display for SpoolError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSpool => formatter.write_str("the print's G-code was indexed with no spool"),
            Self::UnknownLayer => formatter.write_str("no such layer in the print's index"),
            Self::Io { detail } => write!(formatter, "the print's G-code spool: {detail}"),
        }
    }
}

impl std::error::Error for SpoolError {}

/// The spool file of one print under a spool directory.
#[must_use]
pub fn spool_path(directory: &Path, print_id: PrintId) -> PathBuf {
    directory.join(format!("{print_id}{SPOOL_SUFFIX}"))
}

/// What every spool file's name ends in.
pub(crate) const SPOOL_SUFFIX: &str = ".gcode";

/// The unit vectors an outline is bounded along, counter-clockwise from +X.
fn direction(k: usize) -> [f64; 2] {
    let angle = core::f64::consts::TAU * f64::from(u32::try_from(k).unwrap_or(0))
        / f64::from(u32::try_from(OUTLINE_DIRECTIONS).unwrap_or(1));
    [angle.cos(), angle.sin()]
}

/// How far an outline reaches along each of its directions: the support
/// function of every point it was given. Its polygon is the intersection of
/// one half-plane per direction, so it holds every point and only grows
/// outward from the hull.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reach {
    /// The furthest point's projection on each direction.
    extent: [f64; OUTLINE_DIRECTIONS],
    /// Whether any point was given.
    any: bool,
    /// A circle inside the polygon so far: its centre and radius. A point
    /// within it reaches no further in any direction, so it is passed over
    /// without the per-direction work, which is most of a print's moves.
    inside: ([f64; 2], f64),
}

impl Default for Reach {
    fn default() -> Self {
        Self {
            extent: [f64::NEG_INFINITY; OUTLINE_DIRECTIONS],
            any: false,
            inside: ([0.0, 0.0], -1.0),
        }
    }
}

/// The directions, computed once.
fn directions() -> &'static [[f64; 2]; OUTLINE_DIRECTIONS] {
    static HELD: std::sync::OnceLock<[[f64; 2]; OUTLINE_DIRECTIONS]> = std::sync::OnceLock::new();
    HELD.get_or_init(|| {
        let mut held = [[0.0; 2]; OUTLINE_DIRECTIONS];
        let mut k = 0;
        while k < OUTLINE_DIRECTIONS {
            held[k] = direction(k);
            k += 1;
        }
        held
    })
}

impl Reach {
    /// Push every direction's extent out to `(x, y)`; a point inside the
    /// cached inner circle cannot move any extent, so it is skipped.
    pub(crate) fn point(&mut self, x: f64, y: f64) {
        let ([cx, cy], radius) = self.inside;
        let (dx, dy) = (x - cx, y - cy);
        if radius > 0.0 && dx * dx + dy * dy < radius * radius {
            return;
        }
        let held = directions();
        self.any = true;
        // A plain loop: this runs for every extrusion move of a print that
        // the circle inside does not hold.
        let mut grew = false;
        let mut k = 0;
        while k < OUTLINE_DIRECTIONS {
            let along = held[k][0] * x + held[k][1] * y;
            if along > self.extent[k] {
                self.extent[k] = along;
                grew = true;
            }
            k += 1;
        }
        if grew {
            self.refit_inside();
        }
    }

    /// The circle inside the polygon about the middle of its extents: the
    /// nearest of its half-planes from that centre. The four axis directions
    /// are among the directions, a quarter turn apart.
    fn refit_inside(&mut self) {
        let quarter = OUTLINE_DIRECTIONS / 4;
        let (right, up) = (self.extent[0], self.extent[quarter]);
        let (left, down) = (-self.extent[2 * quarter], -self.extent[3 * quarter]);
        if !(right.is_finite() && up.is_finite() && left.is_finite() && down.is_finite()) {
            return;
        }
        let centre = [f64::midpoint(right, left), f64::midpoint(up, down)];
        let held = directions();
        let mut radius = f64::INFINITY;
        let mut k = 0;
        while k < OUTLINE_DIRECTIONS {
            let gap = self.extent[k] - (held[k][0] * centre[0] + held[k][1] * centre[1]);
            radius = radius.min(gap);
            k += 1;
        }
        self.inside = (centre, radius);
    }

    /// Push out the extents of the directions whose circle point the arc
    /// sweeps over: `sweep` radians counter-clockwise from the angle `from`
    /// about `centre`. The arc's ends are not reached here; the caller gives
    /// them to `point`.
    pub(crate) fn arc(&mut self, centre: [f64; 2], radius: f64, from: f64, sweep: f64) {
        let tau = core::f64::consts::TAU;
        let held = directions();
        for (k, unit) in held.iter().enumerate() {
            let angle = tau * f64::from(u32::try_from(k).unwrap_or(0))
                / f64::from(u32::try_from(OUTLINE_DIRECTIONS).unwrap_or(1));
            if (angle - from).rem_euclid(tau) <= sweep + 1e-9 {
                let along = unit[0] * centre[0] + unit[1] * centre[1] + radius;
                if along > self.extent[k] {
                    self.extent[k] = along;
                    self.any = true;
                }
            }
        }
        self.refit_inside();
    }

    /// The outline's polygon, counter-clockwise; nothing when no point was
    /// given.
    pub(crate) fn outline(&self) -> Option<ObjectOutline> {
        if !self.any {
            return None;
        }
        // A box far past any bed, clipped by each half-plane in turn.
        let far = 1e6;
        let mut polygon = vec![[-far, -far], [far, -far], [far, far], [-far, far]];
        for (unit, extent) in directions().iter().zip(self.extent) {
            polygon = clip(&polygon, *unit, extent);
            if polygon.is_empty() {
                return None;
            }
        }
        let mut hull: Vec<[f32; 2]> = Vec::with_capacity(polygon.len());
        for [x, y] in polygon {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "bed millimetres are far inside f32's range; the outline is drawn, not measured"
            )]
            let vertex = [x as f32, y as f32];
            let repeats = hull.last().is_some_and(|last: &[f32; 2]| {
                (last[0] - vertex[0]).abs() < 1e-4 && (last[1] - vertex[1]).abs() < 1e-4
            });
            if !repeats {
                hull.push(vertex);
            }
        }
        while hull.len() > 1
            && hull.first().zip(hull.last()).is_some_and(|(first, last)| {
                (first[0] - last[0]).abs() < 1e-4 && (first[1] - last[1]).abs() < 1e-4
            })
        {
            hull.pop();
        }
        Some(ObjectOutline { hull_mm: hull })
    }
}

/// One Sutherland–Hodgman step: what of a convex polygon lies on the side of
/// `unit · p <= extent`.
fn clip(polygon: &[[f64; 2]], unit: [f64; 2], extent: f64) -> Vec<[f64; 2]> {
    let inside = |p: [f64; 2]| unit[0] * p[0] + unit[1] * p[1] - extent;
    let mut kept = Vec::with_capacity(polygon.len() + 1);
    for (at, point) in polygon.iter().enumerate() {
        let next = polygon[(at + 1) % polygon.len()];
        let (here, there) = (inside(*point), inside(next));
        if here <= 0.0 {
            kept.push(*point);
        }
        if (here <= 0.0) != (there <= 0.0) {
            let t = here / (here - there);
            kept.push([
                point[0] + t * (next[0] - point[0]),
                point[1] + t * (next[1] - point[1]),
            ]);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::{ExtrusionKind, OUTLINE_DIRECTIONS, Reach};

    /// Signed area of a polygon: positive when counter-clockwise.
    fn area(hull: &[[f32; 2]]) -> f64 {
        let mut twice = 0.0;
        for (at, a) in hull.iter().enumerate() {
            let b = hull[(at + 1) % hull.len()];
            twice += f64::from(a[0]) * f64::from(b[1]) - f64::from(b[0]) * f64::from(a[1]);
        }
        twice / 2.0
    }

    #[test]
    fn an_outline_holds_every_point_counter_clockwise_and_is_bounded() {
        let mut reach = Reach::default();
        let points = [
            [10.0, 10.0],
            [30.0, 12.0],
            [25.0, 40.0],
            [12.0, 33.0],
            [20.0, 20.0],
        ];
        for [x, y] in points {
            reach.point(x, y);
        }
        let outline = reach.outline().expect("points were given");
        assert!(outline.hull_mm.len() <= OUTLINE_DIRECTIONS);
        assert!(area(&outline.hull_mm) > 0.0, "counter-clockwise");
        for [x, y] in points {
            for (at, a) in outline.hull_mm.iter().enumerate() {
                let b = outline.hull_mm[(at + 1) % outline.hull_mm.len()];
                let cross = (f64::from(b[0]) - f64::from(a[0])) * (y - f64::from(a[1]))
                    - (f64::from(b[1]) - f64::from(a[1])) * (x - f64::from(a[0]));
                assert!(cross >= -1e-3, "({x}, {y}) lies inside every edge");
            }
        }
    }

    #[test]
    fn an_arc_reaches_the_part_of_its_circle_it_sweeps() {
        let mut reach = Reach::default();
        reach.point(10.0, 0.0);
        reach.point(0.0, 10.0);
        reach.arc([0.0, 0.0], 10.0, 0.0, core::f64::consts::FRAC_PI_2);
        let outline = reach.outline().expect("points were given");
        let furthest = outline
            .hull_mm
            .iter()
            .map(|[x, y]| f64::from(*x).hypot(f64::from(*y)))
            .fold(0.0, f64::max);
        assert!((10.0..10.1).contains(&furthest), "{furthest}");
    }

    #[test]
    fn no_point_is_no_outline_and_types_map_to_kinds() {
        assert!(Reach::default().outline().is_none());
        assert_eq!(ExtrusionKind::of_type("Perimeter"), ExtrusionKind::Wall);
        assert_eq!(ExtrusionKind::of_type("Gap fill"), ExtrusionKind::Infill);
        assert_eq!(
            ExtrusionKind::of_type("Support material interface"),
            ExtrusionKind::Support
        );
        assert_eq!(ExtrusionKind::of_type("Skirt/Brim"), ExtrusionKind::Other);
    }
}
