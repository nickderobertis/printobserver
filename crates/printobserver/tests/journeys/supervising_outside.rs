//! The documented outside supervision, carried out over a print at its
//! milestones.
//!
//! An outside supervisor — a person or an operator's agent that started a print
//! and follows it — takes its own frame at the print's first layer, its first
//! overhang layer, its first support layer and its end. The reference document
//! shows how: the context read that says which layer the print is on and what
//! that layer lays, and the look at the printer with no print named.
//!
//! Each of those examples runs here against the real supervisor over the
//! stood-in machine, running a real `PrusaSlicer` slice — the plain copy of
//! the file the `bubbler-guard-and-pad-pla-healthy` case printed, which the
//! server crate's indexing journey serves — and reads it as far as each example
//! needs: into its first layer, which lays support from the bed, for the first
//! read; into the first layer laying an overhang for the second and the look.
//! What each read must name is worked out from the file apart from the server's
//! index, and the look's frame is read back through `image` with the operator's
//! credential.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use printobserver_types::serde_json::{Value, json};

use crate::documenting::{self, OUTSIDE, Ran};
use crate::slicing::Slicing;
use crate::world::{STOOD_IN, World, committed_skill};

use super::running;

/// How far into its first layer the job has read for the first example.
pub const FIRST_LAYER_POSITION: i64 = 7_249;

/// How far into the first layer laying an overhang the job has read for the
/// examples after it.
pub const OVERHANG_POSITION: i64 = 517_652;

/// The extrusion type a layer lays an overhang as.
const OVERHANG: &str = "Overhang perimeter";

/// The extrusion type a layer lays support as.
const SUPPORT: &str = "Support material";

/// The slice the machine runs, where it is committed.
pub fn slice() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../printobserver-server/tests/fixtures/real-prints/bubbler-guard-and-pad.gcode")
}

/// The configuration edit that puts the slice in front of the world's machine,
/// read into its first layer, and keeps the host running it in `slot`.
///
/// # Panics
///
/// Panics when the configuration names no machine, or `slot` already holds one.
pub fn running_the_slice(slot: &OnceLock<Slicing>) -> impl FnOnce(&mut Value) + '_ {
    move |document| {
        let machine = document["octoprint"]["url"]
            .as_str()
            .expect("the configuration names the machine")
            .to_owned();
        let slicing = Slicing::in_front_of(
            &machine,
            std::fs::read(slice()).expect("the slice reads"),
            FIRST_LAYER_POSITION,
        );
        document["octoprint"]["url"] = json!(slicing.url());
        slot.set(slicing)
            .expect("one slice in front of one world's machine");
    }
}

/// Where each example of the section has the job read to, in the order the
/// section shows them: the first layer, then the first overhang layer.
pub fn position_of(example: usize) -> i64 {
    if example == 0 {
        FIRST_LAYER_POSITION
    } else {
        OVERHANG_POSITION
    }
}

/// The lines of a command's output the section shows a reader: the layer and
/// what it lays, its height and the last layer's, and the time left, from a
/// context read; what a look is, the frame it took and where that is.
pub fn shown(command: &str, line: &str) -> bool {
    let key = line.split_once(": ").map_or(line, |(key, _)| key);
    if command.starts_with("printobserver context") {
        return [
            "context.slicer.layer",
            "context.slicer.z_mm",
            "context.slicer.final_z_mm",
            "context.slicer.minutes_left",
        ]
        .contains(&key)
            || key.starts_with("context.slicer.types.");
    }
    key == "event.kind" || key.starts_with("frame.") || key.starts_with("image_")
}

/// One answer this program printed, read as the labelled lines it prints.
pub fn labelled(printed: &str) -> Vec<(String, String)> {
    printed
        .lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// The value one label of an answer carries.
fn value_of(printed: &str, key: &str) -> Option<String> {
    labelled(printed)
        .into_iter()
        .find_map(|(label, value)| (label == key).then_some(value))
}

/// What a context read says the current layer lays material as.
fn types_of(printed: &str) -> Vec<String> {
    labelled(printed)
        .into_iter()
        .filter_map(|(label, value)| label.starts_with("context.slicer.types.").then_some(value))
        .collect()
}

/// One layer of the slice as the file itself states it: the layer a position
/// is in, counting the `;LAYER_CHANGE` lines wholly before it, and every
/// `;TYPE:` written between that layer's change and the next.
fn layer_of(bytes: &[u8], position: i64) -> (usize, Vec<String>) {
    let position = usize::try_from(position).expect("a position inside the file");
    let mut offset = 0;
    let mut layer = 0;
    let mut types: Vec<String> = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let text = String::from_utf8_lossy(line);
        if text.starts_with(";LAYER_CHANGE") {
            if offset + line.len() > position {
                break;
            }
            layer += 1;
            types.clear();
        } else if let Some(kind) = text.strip_prefix(";TYPE:") {
            let kind = kind.trim().to_owned();
            if !types.contains(&kind) {
                types.push(kind);
            }
        }
        offset += line.len();
    }
    (layer, types)
}

/// The first layer of the slice whose own lines name one extrusion type.
fn first_layer_naming(bytes: &[u8], kind: &str) -> usize {
    let mut layer = 0;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let text = String::from_utf8_lossy(line);
        if text.starts_with(";LAYER_CHANGE") {
            layer += 1;
        } else if layer > 0 && text.trim() == format!(";TYPE:{kind}") {
            return layer;
        }
    }
    panic!("the slice lays no {kind}")
}

/// Run the section over a world whose machine runs the slice, and hold each
/// example to what it shows a reader it does.
///
/// # Panics
///
/// Panics when an example prints anything other than what the section shows,
/// when a context read names a layer other than the file's own first support
/// and first overhang layers, or when the look's frame is not one the
/// operator's credential reads back through `image`.
pub fn carries_out_the_section(world: &World, machine: &Slicing) -> Vec<Vec<Ran>> {
    let bytes = std::fs::read(slice()).expect("the slice reads");
    let ran = documenting::runs_the_section(
        world,
        OUTSIDE,
        |example| machine.reads_to(position_of(example)),
        shown,
    );
    let [first, overhang, look] = ran.as_slice() else {
        panic!(
            "`## {}` shows {} examples, and this journey runs three: the first layer's \
             context, the first overhang layer's context and a look at the printer",
            OUTSIDE.heading,
            ran.len()
        );
    };
    for (read, position, kind) in [
        (first, FIRST_LAYER_POSITION, SUPPORT),
        (overhang, OVERHANG_POSITION, OVERHANG),
    ] {
        let printed = &read[0].printed;
        let (layer, planned) = layer_of(&bytes, position);
        assert_eq!(
            layer,
            first_layer_naming(&bytes, kind),
            "the job at {position} is not in the slice's first layer laying {kind}"
        );
        assert_eq!(
            value_of(printed, "context.slicer.layer"),
            Some(layer.to_string()),
            "the context names another layer than the file's at {position}:\n{printed}"
        );
        let named = types_of(printed);
        assert!(
            named.iter().any(|named| named == kind),
            "the context's layer lays no {kind}:\n{printed}"
        );
        assert!(
            named.iter().all(|named| planned.contains(named)),
            "the context names a type the layer's own lines do not ({planned:?}):\n{printed}"
        );
    }
    let looked = &look[0];
    assert_eq!(
        looked.command, "printobserver look",
        "the section's look names more than the look at the printer"
    );
    let path = value_of(&looked.printed, "image_path").expect("the look's frame path");
    assert_eq!(
        std::fs::read(&path).expect("the look's frame opens"),
        crate::world::FRAME_BYTES,
        "the look's frame is not the camera's"
    );
    let frame = value_of(&looked.printed, "frame.id").expect("the look's frame");
    let image = running::read(world, &["image", "--image-id", &frame]);
    assert_eq!(
        image["path"].as_str(),
        Some(path.as_str()),
        "`image` serves the operator another file than the look's frame: {image}"
    );
    assert!(
        image["record"].get("print_id").is_none_or(Value::is_null),
        "the printer's frame belongs to a print: {image}"
    );
    ran
}

/// Every example of the section, over a world whose machine runs the slice.
pub fn runs_over_a_print_at_its_milestones() {
    let slot = OnceLock::new();
    let world = World::configured(STOOD_IN, &committed_skill(), None, running_the_slice(&slot));
    carries_out_the_section(&world, slot.get().expect("the slice runs"));
}
