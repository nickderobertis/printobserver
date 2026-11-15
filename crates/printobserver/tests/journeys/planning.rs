//! The slicer's plan, read through the command a turn runs, against a real
//! `OctoPrint` printing a real slice — and the flow and fan a change made
//! through this program leaves in force.
//!
//! The slice is the real `PrusaSlicer` file the server crate's indexing
//! journey serves (`crates/printobserver-server/tests/fixtures/real-prints/
//! bubbler-guard-and-pad.gcode`, a plain copy of the file the
//! `bubbler-guard-and-pad-pla-healthy` case printed). It is uploaded to the
//! instance `just octoprint-up` started, then started and paused through this
//! program, so the file position the machine reports holds still while the
//! context is read at it; then `set-flowrate-factor` and `set-fan-percent`
//! run and the context is read again. Every command is the compiled program.
//!
//! What the plan should say at the reported position is worked out here from
//! the file, apart from the reader under test: the `;LAYER_CHANGE` lines wholly
//! before the position, the `;Z:` after the last of them, the `;TYPE:`s under
//! which that layer's moves feed filament, and the last `M73 P.. R..`, `M221`
//! and `M106`/`M107` before the position. A line counts when every byte of it,
//! its newline included, lies before the position.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use printobserver_types::serde_json::{Map, Value, json};

use crate::machine::Reports;
use crate::walk;
use crate::world::World;

use super::running;

/// The reason every change here gives.
const REASON: &str = "the planning journey is reading the plan against a real machine";

/// How long the journey waits for the machine to reach the first layer.
const PATIENCE: Duration = Duration::from_secs(180);

/// The one object the file labels, and the bed area its material covers:
/// 83.0 to 167.3 mm in X and 86.8 to 133.3 mm in Y, as the server crate's
/// indexing journey works it out from the same file, widened to whole
/// millimetres.
const OBJECT: [i64; 4] = [83, 86, 168, 134];

/// The slice, where it is committed.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../printobserver-server/tests/fixtures/real-prints/bubbler-guard-and-pad.gcode")
}

/// Upload the slice to the running instance under a name of its own, through
/// the environment's own script, and answer that name.
fn uploaded() -> String {
    let name = format!("planning-{}.gcode", printobserver_types::EventId::new());
    let ran = Command::new("uv")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args([
            "run",
            "-q",
            "python",
            "tools/octoprint-env/octoprint_env.py",
            "upload",
        ])
        .arg("--file")
        .arg(fixture())
        .args(["--as", &name])
        .output()
        .expect("the environment's own script runs");
    assert!(
        ran.status.success(),
        "the slice could not be uploaded: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    name
}

/// Run one change as the operator, which has to succeed, and answer its
/// document.
fn done(world: &World, asked: &[&str]) -> Value {
    let mut given = asked.to_vec();
    given.extend_from_slice(&["--actor", "operator", "--reason", REASON, "--json"]);
    let ran = running::command(world, &given);
    assert_eq!(
        ran.code,
        Some(0),
        "{asked:?} did not succeed: {}",
        ran.said()
    );
    printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("{asked:?} answered {error}: {}", ran.out))
}

/// The number a G-code line's `letter` word carries, its comment aside.
fn word(line: &str, letter: char) -> Option<f64> {
    line.split(';')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .skip(1)
        .find_map(|word| word.strip_prefix(letter))
        .and_then(|number| number.parse().ok())
}

/// What the file says at one position.
#[derive(Debug, Default)]
struct Read {
    /// The `;LAYER_CHANGE` lines wholly before the position.
    layer: u64,
    /// Every `;LAYER_CHANGE` line.
    layers: u64,
    /// The `;Z:` after the last layer change before the position.
    z_mm: Option<f64>,
    /// The file's last `;Z:`.
    final_z_mm: Option<f64>,
    /// The types under which the current layer's moves feed filament.
    types: Vec<String>,
    /// The last `M73 P.. R..`'s minutes before the position.
    minutes_left: Option<i64>,
    /// The last `M221`'s factor before the position.
    flow_factor: Option<f64>,
    /// The last part-cooling fan speed set before the position, in percent.
    fan_percent: Option<f64>,
}

/// Read the file at `position`, line by line.
fn read_at(file: &str, position: u64) -> Read {
    let mut read = Read::default();
    let mut end = 0_u64;
    let mut kind: Option<String> = None;
    for line in file.split_inclusive('\n') {
        end += u64::try_from(line.len()).expect("a line's length");
        let line = line.trim_end();
        let before = end <= position;
        if line == ";LAYER_CHANGE" {
            read.layers += 1;
            if before {
                read.layer += 1;
                read.types.clear();
            }
        } else if let Some(z) = line.strip_prefix(";Z:") {
            read.final_z_mm = z.parse().ok();
            if before {
                read.z_mm = read.final_z_mm;
            }
        } else if let Some(named) = line.strip_prefix(";TYPE:") {
            kind = Some(named.to_owned()).filter(|named| named != "Custom");
        } else if read.layer > 0
            && read.layers == read.layer
            && ["G1 ", "G2 ", "G3 "]
                .iter()
                .any(|code| line.starts_with(code))
            && word(line, 'E').is_some_and(|fed| fed > 0.0)
        {
            if let Some(kind) = &kind
                && !read.types.contains(kind)
            {
                read.types.push(kind.clone());
            }
        } else if before {
            setting(&mut read, line);
        }
    }
    read
}

/// Read one setting line the position has passed: a time estimate, a flow
/// factor or a part-cooling fan speed.
fn setting(read: &mut Read, line: &str) {
    let part_cooling = || word(line, 'P').is_none_or(|fan| fan == 0.0);
    if line.starts_with("M73 ") && word(line, 'P').is_some() {
        if let Some(minutes) = word(line, 'R') {
            read.minutes_left = format!("{minutes:.0}").parse().ok();
        }
    } else if line.starts_with("M221") {
        read.flow_factor = word(line, 'S').map(|percent| percent / 100.0);
    } else if line.starts_with("M106") && part_cooling() {
        let speed = word(line, 'S').unwrap_or(255.0).clamp(0.0, 255.0);
        read.fan_percent = Some((speed / 255.0 * 100.0).round());
    } else if line.starts_with("M107") && part_cooling() {
        read.fan_percent = Some(0.0);
    }
}

/// A list of names, sorted: the order the plan names its types in is the
/// reader's to choose.
fn sorted(names: &Value) -> Vec<String> {
    let mut names: Vec<String> = names
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// An adjustable as a context states it: the file's value and a change's,
/// each where there is one, and nothing where there is neither.
fn adjustable(file: Option<f64>, changed: Option<f64>) -> Value {
    let mut set = Map::new();
    if let Some(file) = file {
        set.insert("file".to_owned(), json!(file));
    }
    if let Some(changed) = changed {
        set.insert("changed".to_owned(), json!(changed));
    }
    if set.is_empty() {
        Value::Null
    } else {
        Value::Object(set)
    }
}

/// The context of the print the world follows, and the file position the
/// machine reported in it.
fn context(world: &World) -> (Value, u64) {
    let answer = running::read(world, &["context", "--print-id", &world.print_id()]);
    let context = answer["context"].clone();
    let position = context
        .pointer("/job/file_position_bytes")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("the context names no file position: {context}"));
    (context, position)
}

/// The plan a context states agrees with the file read at the position the
/// same context reports, the flow and fan standing as `changed` left them.
fn agrees(context: &Value, file: &str, position: u64, changed: (Option<f64>, Option<f64>)) {
    let read = read_at(file, position);
    let plan = &context["slicer"];
    assert_eq!(plan["layer"], json!(read.layer), "at {position}: {plan}");
    assert_eq!(plan["layers"], json!(read.layers), "{plan}");
    assert_eq!(plan["z_mm"], json!(read.z_mm), "at {position}: {plan}");
    assert_eq!(plan["final_z_mm"], json!(read.final_z_mm), "{plan}");
    let mut types = read.types;
    types.sort();
    assert_eq!(sorted(&plan["types"]), types, "at {position}: {plan}");
    assert_eq!(
        plan["minutes_left"],
        json!(read.minutes_left),
        "at {position}: {plan}"
    );
    assert_eq!(
        plan["flow_factor"],
        adjustable(read.flow_factor, changed.0),
        "at {position}: {plan}"
    );
    assert_eq!(
        plan["fan_percent"],
        adjustable(read.fan_percent, changed.1),
        "at {position}: {plan}"
    );
    assert_eq!(plan["object_count"], json!(1), "{plan}");
    assert_eq!(plan["objects"], json!([OBJECT]), "{plan}");
}

/// A real slice started and paused through this program: `printobserver
/// context` states the plan the file holds at the position the machine
/// reports, and once `set-flowrate-factor` and `set-fan-percent` have run it
/// states the values they set beside the file's, its labelled rendering — what
/// a turn reads — carrying each on a line of its own and every object's bed
/// area on one. The print is cancelled and the machine put back printing the
/// hold print the environment's own suite expects.
pub fn the_context_states_the_plan_and_the_flow_and_fan_set_since(world: &World) {
    let file = std::fs::read_to_string(fixture()).expect("the slice is committed text");
    world.wants(Reports::Operational);
    let name = uploaded();
    let manifest = walk::manifest_of("the planning journey", 1.3, &name).to_string();
    let started = done(
        world,
        &[
            "start-print",
            "--print-id",
            &world.print_id(),
            "--file-name",
            &name,
            "--manifest",
            &manifest,
            "--override-filament-check",
            "true",
        ],
    );
    world.follows(
        started
            .pointer("/record/print_id")
            .and_then(Value::as_str)
            .expect("a start answers the print it opened"),
    );
    world.settles_at(Reports::Printing, "the planning start");

    // Past the first layer change, so that there is a layer to plan.
    let marker = ";LAYER_CHANGE\n";
    let first_layer =
        u64::try_from(file.find(marker).expect("the slice marks layers") + marker.len())
            .expect("an offset");
    let deadline = Instant::now() + PATIENCE;
    while context(world).1 < first_layer {
        assert!(
            Instant::now() < deadline,
            "the machine never reached the slice's first layer"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    done(world, &["pause", "--print-id", &world.print_id()]);
    world.settles_at(Reports::Paused, "pause");

    let (paused, position) = context(world);
    agrees(&paused, &file, position, (None, None));

    let print_id = world.print_id();
    done(
        world,
        &[
            "set-flowrate-factor",
            "--print-id",
            &print_id,
            "--factor",
            "0.95",
        ],
    );
    done(
        world,
        &[
            "set-fan-percent",
            "--print-id",
            &print_id,
            "--percent",
            "80",
        ],
    );
    let (changed, position) = context(world);
    agrees(&changed, &file, position, (Some(0.95), Some(80.0)));

    let labelled = running::command(world, &["context", "--print-id", &print_id]);
    assert_eq!(labelled.code, Some(0), "{}", labelled.said());
    let [x_min, y_min, x_max, y_max] = OBJECT;
    for line in [
        format!("context.slicer.objects: [[{x_min},{y_min},{x_max},{y_max}]]"),
        "context.slicer.object_count: 1".to_owned(),
        "context.slicer.flow_factor.changed: 0.95".to_owned(),
        "context.slicer.fan_percent.changed: 80.0".to_owned(),
    ] {
        assert!(
            labelled.out.lines().any(|said| said == line),
            "the labelled context carries no `{line}`:\n{}",
            labelled.out
        );
    }

    done(world, &["cancel", "--print-id", &print_id]);
    world.settles_at(Reports::Operational, "cancel");
    world.wants(Reports::Printing);
}
