//! The planned geometry the index answers from real `PrusaSlicer` G-code:
//! object outlines, which object a position lays, and each layer's toolpath
//! read back from the print's spool, through the real core over its printer
//! fake, which counts every whole-file read.
//!
//! The fixtures are plain copies of recorded cases' slices
//! (`tests/fixtures/real-prints/README.md`); each is checked here against an
//! independent direct parse of the same bytes written in this file.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use printobserver_core::{
    ExtrusionKind, GcodeIndex, LayerRef, PrintRecord, ToolpathSegment, block_on, spool_path,
};
use printobserver_printer_api::PrinterState;

use crate::world::{World, permissive_envelope};

/// The PETG jar top: one labelled object, with support, bridge and overhang.
const JAR_TOP: &str = include_str!("../fixtures/real-prints/jar-top-petg.gcode");

/// The first six layers of a seven-object plate.
const SEVEN_OBJECTS: &str =
    include_str!("../fixtures/real-prints/seven-object-plate-first-layers.gcode");

/// The file name the printer fake serves, as `World::open_print` names it.
const FILE: &str = "benchy.gcode";

/// A spool directory of this test's own.
fn spool_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "printobserver-spool-{name}-{}",
        printobserver_types::PrintId::new()
    ));
    std::fs::create_dir_all(&dir).expect("the spool directory is made");
    dir
}

/// A world printing `text`, spooling under `dir` when given one.
fn printing(text: &str, dir: Option<PathBuf>) -> (World, PrintRecord) {
    let world = World::with_config(permissive_envelope(), |config| {
        config.gcode_spool_dir = dir;
    });
    world.printer.holds_gcode(FILE, text);
    world
        .printer
        .reports_job(Some(FILE), PrinterState::Printing);
    world.printer.reports_state(PrinterState::Printing);
    let size = i64::try_from(text.len()).expect("a file fits");
    world.printer.reports_position(Some(size / 3), Some(size));
    let print = world.open_print(7);
    (world, print)
}

/// The print's index, built through the core.
fn index_of(world: &World, print: &PrintRecord) -> Arc<GcodeIndex> {
    block_on(world.core.print_gcode_index(print)).expect("the index is built")
}

/// One move a direct parse finds: a line, or an arc with its centre.
#[derive(Debug, Clone, Copy)]
struct Direct {
    /// Where it starts.
    from: [f64; 2],
    /// Where it ends.
    to: [f64; 2],
    /// The arc's centre, for `G2`/`G3`.
    centre: Option<[f64; 2]>,
    /// The `;TYPE:` in force.
    kind: ExtrusionKind,
    /// The object laying it, by its `M486` number.
    object: Option<i64>,
}

/// One layer of a direct parse.
#[derive(Debug, Default)]
struct DirectLayer {
    /// Its `;Z:`.
    z: f64,
    /// Its extrusion moves.
    moves: Vec<Direct>,
}

/// The number after `letter` in a move's words.
fn word(code: &str, letter: char) -> Option<f64> {
    code.split_whitespace()
        .skip(1)
        .find_map(|token| token.strip_prefix(letter)?.parse().ok())
}

/// The kind each `;TYPE:` label is drawn as, written out here rather than
/// read from the core so that the mapping itself is under test.
fn kind_named(name: &str) -> ExtrusionKind {
    match name {
        "External perimeter" | "Perimeter" => ExtrusionKind::Wall,
        "Internal infill" | "Solid infill" | "Top solid infill" | "Gap fill" => {
            ExtrusionKind::Infill
        }
        "Support material" | "Support material interface" => ExtrusionKind::Support,
        "Bridge infill" => ExtrusionKind::Bridge,
        "Overhang perimeter" => ExtrusionKind::Overhang,
        _ => ExtrusionKind::Other,
    }
}

/// Parse a slice straight from its text: every extrusion move of every layer,
/// independently of the index's reader.
fn direct(text: &str) -> Vec<DirectLayer> {
    let mut layers: Vec<DirectLayer> = Vec::new();
    let (mut x, mut y, mut e) = (0.0, 0.0, 0.0);
    let (mut relative_axes, mut relative_e) = (false, false);
    let mut kind = ExtrusionKind::Other;
    let mut object = None;
    for line in text.lines() {
        let line = line.trim();
        if line == ";LAYER_CHANGE" {
            layers.push(DirectLayer::default());
            continue;
        }
        if let Some(z) = line.strip_prefix(";Z:") {
            if let Some(layer) = layers.last_mut() {
                layer.z = z.parse().expect("a height");
            }
            continue;
        }
        if let Some(name) = line.strip_prefix(";TYPE:") {
            kind = kind_named(name.trim());
            continue;
        }
        let code = line.split(';').next().unwrap_or_default().trim();
        match code.split_whitespace().next() {
            Some("G90") => relative_axes = false,
            Some("G91") => relative_axes = true,
            Some("M82") => relative_e = false,
            Some("M83") => relative_e = true,
            Some("G92") => {
                if let Some(value) = word(code, 'E') {
                    e = value;
                }
            }
            Some("M486") => {
                if let Some(number) = code
                    .split_whitespace()
                    .find_map(|token| token.strip_prefix('S')?.parse::<i64>().ok())
                {
                    object = (number >= 0).then_some(number);
                }
            }
            Some(head @ ("G0" | "G1" | "G2" | "G3")) => {
                let (nx, ny) = if relative_axes {
                    (
                        x + word(code, 'X').unwrap_or(0.0),
                        y + word(code, 'Y').unwrap_or(0.0),
                    )
                } else {
                    (word(code, 'X').unwrap_or(x), word(code, 'Y').unwrap_or(y))
                };
                let fed = word(code, 'E').map_or(0.0, |value| {
                    if relative_e || relative_axes {
                        e += value;
                        value
                    } else {
                        let fed = value - e;
                        e = value;
                        fed
                    }
                });
                let arc = matches!(head, "G2" | "G3");
                let moved = (nx - x).hypot(ny - y) > 1e-6;
                if fed > 0.0 && (moved || arc) {
                    let centre = arc.then(|| {
                        [
                            x + word(code, 'I').unwrap_or(0.0),
                            y + word(code, 'J').unwrap_or(0.0),
                        ]
                    });
                    if let Some(layer) = layers.last_mut() {
                        layer.moves.push(Direct {
                            from: [x, y],
                            to: [nx, ny],
                            centre,
                            kind,
                            object,
                        });
                    }
                }
                x = nx;
                y = ny;
            }
            _ => {}
        }
    }
    layers
}

/// Whether two points are the same to within an f32's rounding of bed
/// millimetres.
fn same(a: [f32; 2], b: [f64; 2]) -> bool {
    (f64::from(a[0]) - b[0]).abs() < 1e-3 && (f64::from(a[1]) - b[1]).abs() < 1e-3
}

/// Hold a toolpath's segments to a direct parse's moves: a line is one equal
/// segment; an arc is a chain of chords from its start to its end, every
/// vertex on its circle, each the arc's kind.
fn assert_toolpath_matches(segments: &[ToolpathSegment], moves: &[Direct]) {
    let mut at = 0;
    for (number, expected) in moves.iter().enumerate() {
        let first = segments
            .get(at)
            .unwrap_or_else(|| panic!("move {number} has a segment"));
        assert!(
            same(first.from_mm, expected.from),
            "move {number} starts where parsed"
        );
        assert_eq!(first.kind, expected.kind, "move {number} keeps its kind");
        if let Some(centre) = expected.centre {
            let radius = (expected.from[0] - centre[0]).hypot(expected.from[1] - centre[1]);
            loop {
                let chord = segments[at];
                assert_eq!(chord.kind, expected.kind);
                let off = (f64::from(chord.to_mm[0]) - centre[0])
                    .hypot(f64::from(chord.to_mm[1]) - centre[1])
                    - radius;
                assert!(
                    off.abs() < 1e-2,
                    "move {number}'s chord leaves its circle by {off}"
                );
                at += 1;
                if same(chord.to_mm, expected.to) {
                    break;
                }
            }
        } else {
            assert!(
                same(first.to_mm, expected.to),
                "move {number} ends where parsed"
            );
            at += 1;
        }
    }
    assert_eq!(at, segments.len(), "no segment beyond the parsed moves");
}

/// Each layer chosen to carry support, bridge and overhang moves reads back
/// from the spool as exactly the moves a direct parse of that layer gives, each
/// with its `;TYPE:`'s kind, while the printer serves one whole-file read for
/// the index and every toolpath asked; and the context's plan is the same with
/// a spool as without one.
#[test]
fn a_layers_toolpath_is_read_from_the_spool_as_the_file_plans_it() {
    let dir = spool_dir("toolpath");
    let (world, print) = printing(JAR_TOP, Some(dir.clone()));
    let index = index_of(&world, &print);
    let parsed = direct(JAR_TOP);
    assert_eq!(parsed.len(), index.layer_count());

    let wanted = [
        ExtrusionKind::Support,
        ExtrusionKind::Bridge,
        ExtrusionKind::Overhang,
    ];
    let mut seen = BTreeMap::new();
    for (number, layer) in parsed.iter().enumerate() {
        for kind in wanted {
            if layer.moves.iter().any(|one| one.kind == kind) {
                seen.entry(format!("{kind:?}")).or_insert(number);
            }
        }
    }
    assert_eq!(
        seen.len(),
        wanted.len(),
        "the slice plans every kind asked for"
    );
    for number in seen.into_values() {
        let toolpath = index
            .layer_toolpath(LayerRef::of_index(number))
            .expect("the layer reads back from the spool");
        assert!((f64::from(toolpath.z_mm) - parsed[number].z).abs() < 1e-4);
        assert_toolpath_matches(&toolpath.segments, &parsed[number].moves);
    }
    let first = index
        .layer_toolpath(LayerRef::of_index(0))
        .expect("the first layer reads back");
    assert_toolpath_matches(&first.segments, &parsed[0].moves);
    assert!(
        spool_path(&dir, print.id).exists(),
        "the spool is there while the print is open"
    );

    let spooled = world.context(print.id).expect("the context reads").slicer;
    assert_eq!(
        world.printer.gcode_reads(FILE),
        1,
        "one read across the index and every toolpath"
    );

    let (plain, plain_print) = printing(JAR_TOP, None);
    let unspooled = plain
        .context(plain_print.id)
        .expect("the context reads")
        .slicer;
    assert!(spooled.is_some());
    assert_eq!(
        printobserver_types::serde_json::to_vec(&spooled).expect("serialises"),
        printobserver_types::serde_json::to_vec(&unspooled).expect("serialises"),
        "the context's plan is byte for byte the same with a spool"
    );
    assert_eq!(
        index_of(&plain, &plain_print).layer_toolpath(LayerRef::of_index(0)),
        Err(printobserver_core::SpoolError::NoSpool)
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A support-only layer's nearest object layer is the one below that lays the
/// part, any other layer is its own, and the first object layer ends where
/// the next layer's `;LAYER_CHANGE` begins.
#[test]
fn a_support_only_layer_shows_the_part_below_it() {
    let dir = spool_dir("support-only");
    let (world, print) = printing(JAR_TOP, Some(dir.clone()));
    let index = index_of(&world, &print);
    let parsed = direct(JAR_TOP);
    let lays_part = |layer: &DirectLayer| layer.moves.iter().any(|one| !one.kind.is_support());
    let support_only = parsed
        .iter()
        .position(|layer| !layer.moves.is_empty() && !lays_part(layer))
        .expect("the slice has a support-only layer");
    let below = parsed[..support_only]
        .iter()
        .rposition(lays_part)
        .expect("a part layer below it");
    assert_eq!(
        index.object_layer_at_or_below(LayerRef::of_index(support_only)),
        Some(LayerRef::of_index(below))
    );
    assert_eq!(
        index.object_layer_at_or_below(LayerRef::of_index(below)),
        Some(LayerRef::of_index(below))
    );
    let toolpath = index
        .layer_toolpath(LayerRef::of_index(support_only))
        .expect("the layer reads back");
    assert!(!toolpath.carries_object_extrusion());

    let first = parsed.iter().position(lays_part).expect("a part layer");
    let (layer, ends) = index.first_object_layer().expect("the part is laid");
    assert_eq!(layer, LayerRef::of_index(first));
    let next_change = JAR_TOP
        .match_indices(";LAYER_CHANGE")
        .nth(first + 1)
        .map(|(at, _)| u64::try_from(at).expect("an offset"))
        .expect("a next layer");
    assert_eq!(ends, next_change);
    assert_eq!(
        index.layer_at(ends),
        Some(layer),
        "its last byte is still it"
    );
    assert_eq!(
        index.layer_at(ends + 20),
        Some(LayerRef::of_index(first + 1)),
        "past the next change line is the next layer"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Whether a point lies inside a counter-clockwise polygon, within `slack`.
fn inside(hull: &[[f32; 2]], point: [f64; 2], slack: f64) -> bool {
    hull.iter().enumerate().all(|(at, a)| {
        let b = hull[(at + 1) % hull.len()];
        let (ax, ay) = (f64::from(a[0]), f64::from(a[1]));
        let (bx, by) = (f64::from(b[0]), f64::from(b[1]));
        let cross = (bx - ax) * (point[1] - ay) - (by - ay) * (point[0] - ax);
        cross / (bx - ax).hypot(by - ay).max(1e-9) >= -slack
    })
}

/// A real seven-object plate gives one outline per labelled object, in the
/// plan's object order, each within that object's bed rectangle, holding every
/// point laid for it, at most 64 vertices; every position inside a labelled
/// block names that block's object; and the same plate with its labels taken
/// out gives one outline over all of it.
#[test]
fn each_labelled_object_has_its_outline_and_owns_its_blocks() {
    let dir = spool_dir("objects");
    let (world, print) = printing(SEVEN_OBJECTS, Some(dir.clone()));
    let index = index_of(&world, &print);
    let plan = index.plan_at(0);
    let outlines = index.object_outlines();
    assert_eq!(outlines.len(), 7);
    assert_eq!(outlines.len(), plan.objects.len());

    let mut laid: BTreeMap<i64, Vec<[f64; 2]>> = BTreeMap::new();
    for layer in direct(SEVEN_OBJECTS) {
        for one in layer.moves {
            if let Some(object) = one.object {
                laid.entry(object).or_default().extend([one.from, one.to]);
            }
        }
    }
    let numbers: Vec<i64> = laid.keys().copied().collect();
    for ((outline, area), number) in outlines.iter().zip(&plan.objects).zip(&numbers) {
        assert!(outline.hull_mm.len() >= 3 && outline.hull_mm.len() <= 64);
        for vertex in &outline.hull_mm {
            let edge = |at: usize| f64::from(i32::try_from(area[at]).expect("a bed edge"));
            let within = f64::from(vertex[0]) >= edge(0) - 0.1
                && f64::from(vertex[0]) <= edge(2) + 0.1
                && f64::from(vertex[1]) >= edge(1) - 0.1
                && f64::from(vertex[1]) <= edge(3) + 0.1;
            assert!(within, "object {number}'s outline lies inside its bed area");
        }
        for point in &laid[number] {
            assert!(
                inside(&outline.hull_mm, *point, 1e-3),
                "object {number} holds {point:?}"
            );
        }
    }

    let mut blocks = 0;
    let mut cursor = 0;
    let lines: Vec<&str> = SEVEN_OBJECTS.split_inclusive('\n').collect();
    let mut offsets = Vec::with_capacity(lines.len());
    for line in &lines {
        offsets.push(cursor);
        cursor += line.len();
    }
    for (at, line) in lines.iter().enumerate() {
        let Some(number) = line
            .trim()
            .strip_prefix("M486 S")
            .and_then(|n| n.parse::<i64>().ok())
        else {
            continue;
        };
        let Some(rank) = numbers.iter().position(|held| *held == number) else {
            continue;
        };
        let start = offsets[at] + line.len();
        let end = lines[at + 1..]
            .iter()
            .position(|next| next.starts_with("M486 S"))
            .map_or(SEVEN_OBJECTS.len(), |next| offsets[at + 1 + next]);
        for position in [
            start,
            usize::midpoint(start, end),
            end.saturating_sub(1).max(start),
        ] {
            let position = u64::try_from(position).expect("an offset");
            assert_eq!(
                index.object_at(position),
                Some(rank),
                "inside object {number}'s block"
            );
        }
        blocks += 1;
    }
    assert!(blocks >= 7, "every object's blocks were checked");
    assert_eq!(
        index.object_at(0),
        None,
        "nothing is laid before the first block"
    );

    let unlabelled: String = SEVEN_OBJECTS
        .split_inclusive('\n')
        .filter(|line| !line.starts_with("M486"))
        .collect();
    let (unlabelled_world, unlabelled_print) = printing(&unlabelled, None);
    let whole = index_of(&unlabelled_world, &unlabelled_print);
    assert_eq!(whole.object_outlines().len(), 1);
    assert_eq!(whole.object_at(12_345), Some(0));
    for outline in outlines {
        for vertex in &outline.hull_mm {
            let corner = [f64::from(vertex[0]), f64::from(vertex[1])];
            assert!(inside(&whole.object_outlines()[0].hull_mm, corner, 0.5));
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The spool is deleted when the print closes, and a start-up sweep deletes
/// any left for a print that is no longer open.
#[test]
fn the_spool_goes_when_the_print_closes_and_strays_are_swept() {
    let dir = spool_dir("closing");
    let (world, print) = printing(SEVEN_OBJECTS, Some(dir.clone()));
    let _ = index_of(&world, &print);
    let spool = spool_path(&dir, print.id);
    assert!(spool.exists());

    let stray = spool_path(&dir, printobserver_types::PrintId::new());
    std::fs::write(&stray, b"G1 X0").expect("a stray is written");
    block_on(world.core.sweep_gcode_spools()).expect("the sweep runs");
    assert!(!stray.exists(), "a spool of no open print is swept");
    assert!(spool.exists(), "the open print's spool stays");

    world
        .printer
        .reports_job(Some(FILE), PrinterState::Operational);
    world.printer.reports_state(PrinterState::Operational);
    let _ = block_on(world.core.settle_with_printer()).expect("the print is settled");
    assert!(
        world
            .store
            .print_now(print.id)
            .and_then(|held| held.ended_at)
            .is_some(),
        "the print closed"
    );
    assert!(!spool.exists(), "the spool went with the print");
    let _ = std::fs::remove_dir_all(dir);
}
