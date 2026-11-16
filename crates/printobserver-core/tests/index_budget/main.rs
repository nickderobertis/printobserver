//! What building one print's G-code index costs, and what its facts add to a
//! check: the figures of the budgets `gcode-index-seconds`,
//! `gcode-index-peak-memory-mb` and `agent-context-added-tokens`.
//!
//! A committed generator ([`generator`]) streams a slice shaped like the long
//! plate those budgets name into the core's real [`IndexBuilder`], a piece at
//! a time, through [`GcodeSink`] — the interface the `OctoPrint` adapter's
//! whole-file read feeds it. The plate is written into memory first, because
//! the generator stands in for the `OctoPrint` read, which the budget leaves
//! out: timing its writing would measure this test rather than the indexer.
//! The build is then timed as the elapsed wall time from the first piece
//! reaching the builder to the index being ready, each piece handed over as
//! fast as a read delivers one, and the builder's own share of that time is
//! recorded beside it; and the heap is counted over the whole build by the counting
//! global allocator this binary runs under, from a baseline taken once the
//! written plate is held, so the held plate is outside it while the read
//! buffer and the built index are inside. The context answer is then rendered at the layer naming
//! the most extrusion types, through the labelled rendering the program a turn
//! runs prints it in (`printobserver_types::labelled`), and the
//! bytes the plan's facts add are converted to tokens at the ratio the
//! `spike-check-costs` spike measured.
//!
//! Each figure is written to this project's `telemetry/`, which the budgets'
//! commands read; `onebudgetspec check` is the only judge of them. What this
//! asserts is the workload — that the generated slice is the shape the budgets
//! name — and no bound on any figure.
//!
//! A binary and an Nx project of its own, because streaming seventy-eight
//! megabytes is the slowest thing the core's tests would otherwise do.

/// The deterministic long plate.
mod generator;

use core::ops::ControlFlow;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use peak_alloc::PeakAlloc;
use printobserver_core::{GcodeIndex, IndexBuilder, PlanMismatch, PrintContext, Rise};
use printobserver_printer_api::GcodeSink as _;
use printobserver_types::contract::Sample;
use printobserver_types::labelled::{self, fields};
use printobserver_types::serde_json::{self, Value, json};

use crate::generator::{FINAL_Z_MM, LAYERS, LongPlate, PIECE, Shape};

/// The one allocator this binary runs under, so every allocation the build
/// makes is counted.
#[global_allocator]
static ALLOCATOR: PeakAlloc = PeakAlloc;

/// Model input tokens per byte of what a turn reads, on the Claude model the
/// agent side is served by: the `spike-check-costs` spike's measurement
/// (report `print-run-improvements-spike-check-costs-report`, its tokens row),
/// +127 input tokens for +330 bytes on `claude-opus-5-5`, the median of two
/// real `claude-code` turns per variant.
const TOKENS_PER_BYTE: f64 = 0.385;

/// The commit the plan's added bytes are measured from.
const BASE_COMMIT: &str = "0f1c113e";

/// The turn prompt template's bytes at [`BASE_COMMIT`].
const TURN_PROMPT_BYTES_AT_BASE: usize = 2_137;

/// The turn prompt template as it stands.
const TURN_PROMPT: &str = include_str!("../../../printobserver-oneharness/assets/turn-prompt.md");

/// The long plate the budgets name, which the generated slice is held to
/// within a tenth of: bytes, lines, layers, normal-mode and silent-mode `M73`
/// markers, and the most types in one layer.
const LONG_PLATE: [(&str, f64); 6] = [
    ("bytes", 77_974_474.0),
    ("lines", 3_349_278.0),
    ("layers", 850.0),
    ("normal_m73", 661.0),
    ("silent_m73", 745.0),
    ("most_types_in_one_layer", 8.0),
];

/// The plan's facts the budget `agent-context-added-tokens` counts: the layer
/// and its height, the extrusion types, the final height, the slicer's time
/// left, the flow factor and fan speed in force, the objects with their bed
/// areas, the height risen since an earlier frame, and the settings the plan
/// says are wrong.
const COUNTED_FACTS: [&str; 11] = [
    "layer",
    "z_mm",
    "types",
    "final_z_mm",
    "minutes_left",
    "flow_factor",
    "fan_percent",
    "object_count",
    "objects",
    "rise",
    "at_odds",
];

/// How many objects the long plate labels: eight guards and eight pads.
const OBJECTS: usize = 16;

/// Bytes in a megabyte.
const MEGABYTE: f64 = 1_000_000.0;

/// One of this project's telemetry files.
fn telemetry(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("index_budget")
        .join("telemetry")
        .join(name)
}

/// A count, as a figure.
fn figure(count: impl TryInto<u32>) -> f64 {
    f64::from(count.try_into().unwrap_or(u32::MAX))
}

/// A byte count, as megabytes.
fn megabytes(bytes: usize) -> f64 {
    let kilobytes = u32::try_from(bytes / 1_000).unwrap_or(u32::MAX);
    f64::from(kilobytes) * 1_000.0 / MEGABYTE
}

/// Write one telemetry file.
fn record(name: &str, figures: &Value, detail: &Value) {
    let path = telemetry(name);
    std::fs::create_dir_all(path.parent().expect("a telemetry directory"))
        .expect("the telemetry directory is writable");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({ "figures": figures, "detail": detail }))
            .expect("a record renders"),
    )
    .expect("the telemetry file is writable");
}

/// One build of the long plate's index, and what it cost.
struct Built {
    /// The index.
    index: GcodeIndex,
    /// What the generator wrote.
    shape: Shape,
    /// The builder's own share of [`Built::wall`]: the time spent inside its
    /// calls, the pieces' arrival between them left out.
    building: Duration,
    /// The elapsed wall time from the first byte reaching the builder to the
    /// index being ready: what the budget `gcode-index-seconds` measures.
    wall: Duration,
    /// The peak heap over the build, beyond what was held before it.
    peak: usize,
}

/// The long plate, written a piece at a time, every piece held.
fn written() -> (Vec<String>, Shape) {
    let mut plate = LongPlate::new();
    let mut pieces = Vec::new();
    loop {
        let mut piece = String::with_capacity(PIECE * 2);
        plate.next_piece(&mut piece);
        if piece.is_empty() {
            return (pieces, plate.shape());
        }
        pieces.push(piece);
    }
}

/// Stream the long plate into the core's index builder, a piece at a time.
fn built() -> Built {
    let (pieces, shape) = written();
    ALLOCATOR.reset_peak_usage();
    let baseline = ALLOCATOR.current_usage();
    // The read buffer a streamed read fills, one piece at a time.
    let mut piece = Vec::with_capacity(PIECE * 2);
    let mut builder = IndexBuilder::new();
    let mut building = Duration::ZERO;
    let started = Instant::now();
    for held in &pieces {
        piece.clear();
        piece.extend_from_slice(held.as_bytes());
        let taking = Instant::now();
        assert_eq!(builder.take(&piece), ControlFlow::Continue(()));
        building += taking.elapsed();
    }
    let finishing = Instant::now();
    let index = builder.finish();
    building += finishing.elapsed();
    let wall = started.elapsed();
    let peak = ALLOCATOR.peak_usage().saturating_sub(baseline);
    Built {
        index,
        shape,
        building,
        wall,
        peak,
    }
}

/// The context answer at the layer naming the most types, as a turn reads it.
struct Rendered {
    /// The layer.
    layer: Option<usize>,
    /// How many types it names.
    most_types: usize,
    /// The bytes each plan field's lines take.
    per_field: serde_json::Map<String, Value>,
    /// The bytes the budget's counted facts take.
    counted: usize,
    /// The bytes every plan field takes.
    every: usize,
    /// How many objects the plan names.
    objects: usize,
}

/// Render the context at the layer naming the most types, and measure what
/// the plan's lines take.
fn rendered(index: &GcodeIndex) -> Rendered {
    let (most_types, position) = index.busiest_layer().expect("the plate plans layers");
    let plan = index.plan_at(position);
    assert_eq!(plan.types.len(), most_types);
    assert_eq!(plan.final_z_mm, Some(FINAL_Z_MM));
    assert_eq!(plan.object_count, Some(OBJECTS));
    assert!(
        plan.flow_factor.is_some() && plan.fan_percent.is_some(),
        "the plate's flow and fan are not in force at the layer measured"
    );
    let objects = plan.objects.len();
    let layer = plan.layer;
    let mut plan = plan;
    // The two facts read beside the index, at their longest: a rise since a
    // frame half an hour back, named by a capture time to the nanosecond, from
    // a top as high as the plate's own, and both settings named as wrong for
    // the plan.
    plan.rise = Some(Rise {
        since: "2026-10-08T20:48:30.123456789Z"
            .parse()
            .expect("a capture time"),
        mm: [129.6, 12.4],
    });
    plan.at_odds = vec![
        PlanMismatch::FanOffOverBridges,
        PlanMismatch::FlowLowOverWalls,
    ];
    let mut with_plan = PrintContext::sample_full();
    with_plan.slicer = Some(plan);
    // The labelled lines `printobserver context` prints, through the one
    // rendering the program itself uses.
    let lines = fields(&json!({
        "context": serde_json::to_value(&with_plan).expect("a context renders")
    }));
    let mut per_field = serde_json::Map::new();
    let mut counted = 0_usize;
    let mut every = 0_usize;
    for line in &lines {
        let Some(field) = line.0.strip_prefix("context.slicer.") else {
            continue;
        };
        let field = field.split('.').next().unwrap_or(field);
        let bytes = labelled::line(line).len();
        every += bytes;
        if COUNTED_FACTS.contains(&field) {
            counted += bytes;
        }
        let held = per_field.entry(field).or_insert(json!(0));
        *held = json!(held.as_u64().unwrap_or(0) + u64::try_from(bytes).expect("a count"));
    }
    // Every counted fact is one the plan answers at that layer: a field
    // renamed or dropped would otherwise leave the count short in silence.
    for fact in COUNTED_FACTS {
        assert!(
            per_field.contains_key(fact),
            "the plan answers no {fact} at the layer measured: {per_field:?}"
        );
    }
    Rendered {
        layer,
        most_types,
        per_field,
        counted,
        every,
        objects,
    }
}

/// The long plate streamed once into the core's index builder: the build's
/// time and peak heap, and the tokens the plan's facts add to a check at the
/// layer naming the most types, are recorded for the budgets to judge.
#[test]
fn one_long_plate_is_indexed_and_its_plan_rendered() {
    let Built {
        index,
        shape,
        building,
        wall,
        peak,
    } = built();
    let generated = [
        shape.bytes,
        shape.lines,
        shape.layers,
        shape.normal_m73,
        shape.silent_m73,
        shape.most_types,
    ];
    for ((name, wanted), had) in LONG_PLATE.iter().zip(generated) {
        let had = figure(had);
        assert!(
            (had - wanted).abs() <= wanted / 10.0,
            "the generated plate's {name} is {had}, not within a tenth of the long plate's {wanted}"
        );
    }
    assert!(shape.arcs > 0, "the generated plate carries no arcs");
    assert_eq!(
        index.bytes(),
        shape.bytes,
        "the builder did not read every byte"
    );
    assert_eq!(
        index.layer_count(),
        usize::try_from(LAYERS).expect("a count")
    );
    let rendered = rendered(&index);
    let prompt_added = TURN_PROMPT.len().saturating_sub(TURN_PROMPT_BYTES_AT_BASE);
    let added_bytes = rendered.counted + prompt_added;
    let added_tokens = figure(added_bytes) * TOKENS_PER_BYTE;

    let built = json!({
        "bytes": shape.bytes,
        "lines": shape.lines,
        "layers": shape.layers,
        "normal_m73": shape.normal_m73,
        "silent_m73": shape.silent_m73,
        "most_types_in_one_layer": shape.most_types,
        "arcs": shape.arcs,
        "index_entries": index.entries(),
        "index_resident_bytes": index.resident_bytes(),
        "peak_heap_bytes": peak,
        "wall_seconds": wall.as_secs_f64(),
        "builder_seconds": building.as_secs_f64(),
        "throughput_mb_per_s": figure(shape.bytes / 1_000) / 1_000.0 / wall.as_secs_f64(),
        "build": if cfg!(debug_assertions) { "unoptimised" } else { "optimised" },
    });
    record(
        "gcode-index.json",
        &json!({ "seconds": wall.as_secs_f64(), "peak_mb": megabytes(peak) }),
        &built,
    );
    record(
        "agent-context.json",
        &json!({ "added_tokens": added_tokens }),
        &json!({
            "rendering": "labelled lines, as `printobserver context` prints them",
            "layer": rendered.layer,
            "types_at_that_layer": rendered.most_types,
            "added_bytes_per_field": rendered.per_field,
            "counted_fields": COUNTED_FACTS,
            "counted_bytes": rendered.counted,
            "objects": rendered.objects,
            "bytes_per_object": figure(
                rendered.per_field["objects"].as_u64().unwrap_or(0)
            ) / figure(rendered.objects.max(1)),
            "every_plan_field_bytes": rendered.every,
            "every_plan_field_tokens": figure(rendered.every) * TOKENS_PER_BYTE,
            "turn_prompt_bytes": TURN_PROMPT.len(),
            "turn_prompt_bytes_at_base": TURN_PROMPT_BYTES_AT_BASE,
            "turn_prompt_added_bytes": prompt_added,
            "base_commit": BASE_COMMIT,
            "tokens_per_byte": TOKENS_PER_BYTE,
            "tokens_per_byte_source": "spike-check-costs: print-run-improvements-spike-check-costs-report, tokens row",
        }),
    );
}
