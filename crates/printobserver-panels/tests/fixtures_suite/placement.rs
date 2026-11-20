//! Where the committed calibration puts each part's planned walls against
//! where the part is: hand-marked part-edge points back-projected to the bed
//! at their layer's height, and their distances measured there in
//! millimetres.

use printobserver_core::ExtrusionKind;
use printobserver_types::serde_json::Value;

use crate::support::{
    back_project, calibration, distance_to, index, island_nearest, islands, layer_at_z, marks,
    median, recorded_z, segments_of, toolpath,
};

/// A wall is laid as a wall or an overhang perimeter.
const WALLS: [ExtrusionKind; 2] = [ExtrusionKind::Wall, ExtrusionKind::Overhang];

/// The marks' pixels.
fn pixels(points: &Value) -> Vec<[f64; 2]> {
    points
        .as_array()
        .expect("pixels")
        .iter()
        .map(|point| [point[0].as_f64().expect("x"), point[1].as_f64().expect("y")])
        .collect()
}

/// The median distance, in millimetres, of marks on a part's edge at height
/// `h` from some walls, with the print at `print_z`.
fn median_mm(pixels: &[[f64; 2]], walls: &[[f64; 4]], h: f64, print_z: f64) -> f64 {
    let calibration = calibration();
    let mut distances: Vec<f64> = pixels
        .iter()
        .map(|pixel| distance_to(back_project(&calibration, *pixel, h, print_z), walls))
        .collect();
    median(&mut distances)
}

/// The dragged ring's edge lies far from its planned walls on the first frame
/// showing the drag, and on them on the frame before.
#[test]
fn the_dragged_ring_lies_off_its_walls_only_once_it_is_dragged() {
    let ring = &marks()["ring"];
    let case = ring["case"].as_str().expect("a case");
    let top = ring["ring_top_mm"].as_f64().expect("a height");
    let walls = segments_of(&toolpath(case, layer_at_z(&index(case), top)), &WALLS);
    let mut found = Vec::new();
    for (file, points) in ring["frames"].as_object().expect("frames") {
        let print_z = recorded_z(case, file);
        found.push((
            file.clone(),
            median_mm(&pixels(points), &walls, top, print_z),
        ));
    }
    let before = found
        .iter()
        .find(|(file, _)| file.starts_with("agent-008"))
        .expect("the frame before");
    let dragged = found
        .iter()
        .find(|(file, _)| file.starts_with("agent-009"))
        .expect("the frame showing the drag");
    assert!(
        before.1 <= 5.0,
        "the ring before the drag lies a median {:.2} mm from its walls; at most 5 mm",
        before.1
    );
    assert!(
        dragged.1 >= 15.0,
        "the dragged ring lies a median {:.2} mm from its walls; at least 15 mm",
        dragged.1
    );
}

/// On frames no fit used, each printed part visible — the bubbler plate's
/// guard and pad each on its own, split by the islands of its layer's walls
/// and never pooled — lies on its own planned walls: its held-out marks a
/// median of at most 5 mm from them.
#[test]
fn each_part_lies_on_its_own_walls_on_held_out_frames() {
    let marks = marks();
    let parts = &marks["parts"];
    let mut measured = Vec::new();
    for entry in marks["held_out"].as_array().expect("held-out marks") {
        let case = entry["case"].as_str().expect("a case");
        let file = entry["file"].as_str().expect("a file");
        let part = entry["part"].as_str().expect("a part");
        let print_z = recorded_z(case, file);
        let index = index(case);
        let layer = if entry["outline"] == "first" {
            index.first_object_layer().expect("a first layer").0
        } else {
            layer_at_z(&index, print_z)
        };
        let path = toolpath(case, layer);
        let kinds: &[ExtrusionKind] = if entry["outline"] == "first" {
            &[
                ExtrusionKind::Wall,
                ExtrusionKind::Overhang,
                ExtrusionKind::Other,
            ]
        } else {
            &WALLS
        };
        let walls = segments_of(&path, kinds);
        let own = if case == "bubbler-guard-and-pad-pla-healthy" {
            let centre = &parts[part];
            island_nearest(
                &islands(&walls, 2.0),
                [
                    centre[0].as_f64().expect("x"),
                    centre[1].as_f64().expect("y"),
                ],
            )
        } else {
            walls
        };
        let mm = median_mm(&pixels(&entry["px"]), &own, f64::from(path.z_mm), print_z);
        measured.push(format!("{case}/{file} {part}: {mm:.2} mm"));
        assert!(
            mm <= 5.0,
            "{case}/{file}: the {part}'s marks lie a median {mm:.2} mm from its walls; at most 5 mm"
        );
    }
    let bubbler: Vec<&String> = measured
        .iter()
        .filter(|line| line.starts_with("bubbler"))
        .collect();
    for part in ["guard", "pad"] {
        assert!(
            bubbler
                .iter()
                .any(|line| line.contains(&format!(" {part}:"))),
            "the bubbler plate's {part} is measured: {measured:#?}"
        );
    }
}
