//! A deterministic, streamed `PrusaSlicer` slice shaped like the long plate the
//! index budgets name.
//!
//! The shape is a bed-filling multi-part CORE One slice (`PrusaSlicer` 2.9.6,
//! HF0.4 nozzle, 0.20mm BALANCED, automatic supports): about 78 MB in
//! 3,349,278 lines over 850 layers to a final Z of 142.0 mm, with 661
//! normal-mode `M73 P.. R..` and 745 silent-mode `M73 Q.. S..` markers, at
//! most eight extrusion types in one layer, and `G2`/`G3` arcs with `I`/`J`.
//! Its per-layer densities follow the recipe of the spike that measured that
//! slice and the index's cost over it (reports
//! `print-run-improvements-spike-ops-costs-report`, which holds the slice, and
//! `print-run-improvements-spike-check-costs-report`, which measured the
//! index): about 23.5 to 24.0 bytes a line, normal-mode `M73` about 0.78 a
//! layer, and silent-mode `M73` 1.13 times as many.
//!
//! It writes one piece at a time into a buffer it is handed and holds nothing
//! else of the file, so streaming it costs what its read buffer does.

use core::fmt::Write as _;

/// How many layers it plans.
pub const LAYERS: u32 = 850;

/// The final layer's height, in millimetres.
pub const FINAL_Z_MM: f64 = 142.0;

/// The first layer's height, in millimetres.
const FIRST_Z_MM: f64 = 0.2;

/// How many normal-mode time estimates it writes over the print.
const NORMAL_M73: u32 = 661;

/// How many silent-mode time estimates it writes over the print.
const SILENT_M73: u32 = 745;

/// The minutes the slicer estimates the whole print at: 9 h 21 min.
const MINUTES: u32 = 561;

/// How many lines each layer carries, all told: 3,349,278 over 850 layers.
const LINES_PER_LAYER: u32 = 3_940;

/// The objects on the plate: a name, and the corner and size of the bed area
/// it is laid in, in millimetres.
const OBJECTS: [(&str, [f64; 2], [f64; 2]); 6] = [
    ("magnetic bin body", [8.0, 10.0], [72.0, 78.0]),
    ("spool clip", [88.0, 12.0], [40.0, 60.0]),
    ("purge pan", [140.0, 10.0], [100.0, 80.0]),
    ("scraper holder", [10.0, 110.0], [70.0, 90.0]),
    ("magnetic base", [92.0, 104.0], [64.0, 96.0]),
    ("screw", [170.0, 120.0], [60.0, 80.0]),
];

/// Every extrusion type the plate names, in the order a layer lays them.
const TYPES: [&str; 10] = [
    "Skirt/Brim",
    "Support material",
    "Support material interface",
    "Perimeter",
    "External perimeter",
    "Overhang perimeter",
    "Internal infill",
    "Solid infill",
    "Top solid infill",
    "Bridge infill",
];

/// The most types any one layer lays.
const MOST_TYPES: usize = 8;

/// How many bytes a piece is filled to before it is handed over: the read
/// buffer's size.
pub const PIECE: usize = 64 * 1024;

/// What the generated file was, counted as it was written.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// Bytes written.
    pub bytes: u64,
    /// Lines written.
    pub lines: u64,
    /// `;LAYER_CHANGE` markers written.
    pub layers: u64,
    /// Normal-mode `M73 P.. R..` markers written.
    pub normal_m73: u64,
    /// Silent-mode `M73 Q.. S..` markers written.
    pub silent_m73: u64,
    /// The most extrusion types any one layer lays.
    pub most_types: u64,
    /// `G2` and `G3` arcs written.
    pub arcs: u64,
}

/// Where the generator is in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    /// The start G-code and the object labels.
    Head,
    /// The start of a layer.
    Layer(u32),
    /// One type's run of moves for one object of one layer.
    Run {
        /// The layer.
        layer: u32,
        /// The object.
        object: usize,
        /// Which of the layer's types.
        run: usize,
    },
    /// The end G-code.
    Tail,
    /// Nothing more.
    Done,
}

/// The long plate, written a piece at a time.
#[derive(Debug)]
pub struct LongPlate {
    /// Where it is.
    cursor: Cursor,
    /// What it has written.
    shape: Shape,
    /// The pseudo-random state the moves are drawn from.
    seed: u64,
    /// Where the head is in X and Y.
    at: [f64; 2],
}

impl LongPlate {
    /// The plate from its first byte.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            cursor: Cursor::Head,
            shape: Shape {
                bytes: 0,
                lines: 0,
                layers: 0,
                normal_m73: 0,
                silent_m73: 0,
                most_types: 0,
                arcs: 0,
            },
            seed: 0x9e37_79b9_7f4a_7c15,
            at: [0.0, 0.0],
        }
    }

    /// What it has written so far.
    #[must_use]
    pub const fn shape(&self) -> Shape {
        self.shape
    }

    /// Write the next piece into `piece`, cleared first: whole lines, up to
    /// about [`PIECE`] bytes. Nothing is written once the file is over, which
    /// is what an empty piece means.
    pub fn next_piece(&mut self, piece: &mut String) {
        piece.clear();
        while piece.len() < PIECE && self.cursor != Cursor::Done {
            let before = piece.len();
            self.cursor = self.unit(piece);
            let written = &piece[before..];
            self.shape.bytes += u64::try_from(written.len()).unwrap_or(u64::MAX);
            self.shape.lines += u64::try_from(written.matches('\n').count()).unwrap_or(u64::MAX);
        }
    }

    /// Write one unit at the cursor, and answer where the next one is.
    fn unit(&mut self, out: &mut String) -> Cursor {
        match self.cursor {
            Cursor::Head => {
                head(out);
                Cursor::Layer(1)
            }
            Cursor::Layer(layer) => {
                self.layer_start(layer, out);
                Cursor::Run {
                    layer,
                    object: 0,
                    run: 0,
                }
            }
            Cursor::Run { layer, object, run } => {
                let types = types_of(layer);
                if run == 0 {
                    self.object_start(layer, object, out);
                }
                self.moves(layer, object, types[run], types.len(), out);
                if run + 1 < types.len() {
                    return Cursor::Run {
                        layer,
                        object,
                        run: run + 1,
                    };
                }
                object_end(out);
                if object + 1 < OBJECTS.len() {
                    Cursor::Run {
                        layer,
                        object: object + 1,
                        run: 0,
                    }
                } else if layer < LAYERS {
                    Cursor::Layer(layer + 1)
                } else {
                    Cursor::Tail
                }
            }
            Cursor::Tail => {
                tail(out);
                Cursor::Done
            }
            Cursor::Done => Cursor::Done,
        }
    }

    /// A layer's change, its height, and the estimates and fan its start carries.
    fn layer_start(&mut self, layer: u32, out: &mut String) {
        let z = z_of(layer);
        self.shape.layers += 1;
        self.shape.most_types = self
            .shape
            .most_types
            .max(u64::try_from(types_of(layer).len()).unwrap_or(u64::MAX));
        let _ = write!(
            out,
            ";LAYER_CHANGE\n;Z:{z}\n;HEIGHT:0.2\n;BEFORE_LAYER_CHANGE\nG92 E0.0\n;{z}\n\n\
             G1 E-.8 F2700\n;AFTER_LAYER_CHANGE\n;{z}\n",
            z = trimmed(z)
        );
        let done = |count: u32| u64::from(layer * count / LAYERS);
        let left = MINUTES - layer * MINUTES / LAYERS;
        while self.shape.normal_m73 < done(NORMAL_M73) {
            self.shape.normal_m73 += 1;
            let _ = writeln!(out, "M73 P{} R{left}", layer * 100 / LAYERS);
        }
        while self.shape.silent_m73 < done(SILENT_M73) {
            self.shape.silent_m73 += 1;
            let _ = writeln!(out, "M73 Q{} S{}", layer * 100 / LAYERS, left + 4);
        }
        if layer == 2 {
            out.push_str("M106 S255\n");
        } else if layer > 2 && layer.is_multiple_of(40) {
            let _ = writeln!(out, "M106 S{}", 216 + layer % 39);
        }
    }

    /// The travel to an object and its labelling.
    fn object_start(&mut self, layer: u32, object: usize, out: &mut String) {
        let ([x, y], _) = (OBJECTS[object].1, OBJECTS[object].2);
        self.at = [x, y];
        let _ = write!(
            out,
            "M486 S{object}\nG1 X{} Y{} F21000\nG1 Z{} F720\nG1 E.8 F1500\nM204 P500\n",
            trimmed(x),
            trimmed(y),
            trimmed(z_of(layer))
        );
    }

    /// One type's run of moves inside an object's area.
    fn moves(&mut self, layer: u32, object: usize, kind: &str, runs: usize, out: &mut String) {
        let _ = write!(out, ";TYPE:{kind}\n;WIDTH:0.45\nG1 F2700\n");
        let objects = u32::try_from(OBJECTS.len()).unwrap_or(1);
        let runs = u32::try_from(runs).unwrap_or(1);
        // Each layer's lines, less its own and its objects' framing, shared
        // among its objects' type runs.
        let framing = 12 + objects * 10 + objects * runs * 3;
        let count = (LINES_PER_LAYER - framing) / (objects * runs);
        let ([left, bottom], [width, depth]) = (OBJECTS[object].1, OBJECTS[object].2);
        for step in 0..count {
            let x = left + self.unit_draw() * width;
            let y = bottom + self.unit_draw() * depth;
            let fed = 1 + self.draw() % 900;
            if (step + layer).is_multiple_of(41) {
                self.shape.arcs += 1;
                let _ = writeln!(
                    out,
                    "G{} X{x:.3} Y{y:.3} I{:.3} J{:.3} E.{fed:05}",
                    if step.is_multiple_of(2) { 2 } else { 3 },
                    (x - self.at[0]) / 2.0,
                    (y - self.at[1]) / 2.0,
                );
            } else {
                let _ = writeln!(out, "G1 X{} Y{} E.{fed:03}", hundredths(x), hundredths(y));
            }
            self.at = [x, y];
        }
    }

    /// The next pseudo-random draw.
    const fn draw(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// A draw between zero and one.
    fn unit_draw(&mut self) -> f64 {
        f64::from(u32::try_from(self.draw() % 10_000).unwrap_or(0)) / 10_000.0
    }
}

/// The start G-code: the machine's set-up and the plate's object labels.
fn head(out: &mut String) {
    out.push_str(
        "; generated by PrusaSlicer 2.9.6 on 2026-11-01 at 12:00:00 UTC\n\n\
         ; external perimeters extrusion width = 0.45mm\n; perimeters extrusion width = 0.45mm\n\
         M17 ; enable steppers\nM862.1 P0.4 A0 F1\nM862.3 P \"COREONE\"\nM115 U6.4.0+11974\n\
         M555 X2 Y4 W246 H212\nG90 ; use absolute coordinates\nM83 ; extruder relative mode\n\
         M140 S60\nM104 T0 S170\nM109 T0 R170\nM190 S60\nG28 ; home all without mesh bed level\n\
         M104 T0 S230\nM109 T0 S230\nM221 S100 ; set flow to 100%\nG92 E0\n",
    );
    for (number, (name, _, _)) in OBJECTS.iter().enumerate() {
        let _ = write!(out, "M486 S{number}\nM486 A{name}\nM486 S-1\n");
    }
    let _ = write!(
        out,
        "M73 P0 R{MINUTES}\nM73 Q0 S{}\nM107\n;TYPE:Custom\n;WIDTH:0.5\nG1 X20 Y-4 E8 F600\n\
         G1 X60 E10\nG92 E0\n",
        MINUTES + 4
    );
}

/// An object's wipe, retraction and unlabelling.
fn object_end(out: &mut String) {
    out.push_str(";WIPE_START\nG1 X100.5 Y100.5 E-.04\n;WIPE_END\nG1 E-.76 F2700\nM486 S-1\n");
}

/// The end G-code.
fn tail(out: &mut String) {
    out.push_str(
        "M73 P100 R0\nM73 Q100 S0\n;TYPE:Custom\nG1 Z180 F720\nG1 X242 Y211 F10200\nM107\n\
         M140 S0\nM104 S0\nM84 X Y E\n",
    );
}

/// One layer's height.
fn z_of(layer: u32) -> f64 {
    let step = (FINAL_Z_MM - FIRST_Z_MM) / f64::from(LAYERS - 1);
    ((FIRST_Z_MM + f64::from(layer - 1) * step) * 1000.0).round() / 1000.0
}

/// A coordinate to a hundredth, as a slicer writes one: no more decimals than
/// it needs.
fn hundredths(value: f64) -> String {
    let written = format!("{value:.2}");
    written
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

/// A length as a slicer writes one: no more decimals than it needs.
fn trimmed(value: f64) -> String {
    let written = format!("{value:.3}");
    written
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

/// The types one layer lays, in the order it lays them: never more than
/// [`MOST_TYPES`], and that many in a layer every hundred, where supports,
/// their interface, an overhang, solid infill and a bridge meet.
fn types_of(layer: u32) -> Vec<&'static str> {
    let has = |name: &str| match name {
        "Skirt/Brim" => layer == 1,
        "Support material" => layer <= 320 && !layer.is_multiple_of(3),
        "Support material interface" => layer <= 320 && layer % 25 < 2,
        "Perimeter" | "External perimeter" => true,
        "Overhang perimeter" => layer.is_multiple_of(7),
        "Internal infill" => layer > 3,
        "Solid infill" => layer.is_multiple_of(4) || layer <= 3,
        "Top solid infill" => layer.is_multiple_of(50) || layer == LAYERS,
        "Bridge infill" => layer % 25 == 2,
        _ => false,
    };
    let busiest = layer % 100 == 50 && layer <= 320;
    let mut found: Vec<&'static str> = TYPES
        .iter()
        .copied()
        .filter(|name| {
            if busiest {
                !matches!(*name, "Skirt/Brim" | "Top solid infill")
            } else {
                has(name)
            }
        })
        .collect();
    found.truncate(MOST_TYPES);
    if !busiest && found.len() == MOST_TYPES {
        found.pop();
    }
    found
}
