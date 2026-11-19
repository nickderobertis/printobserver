//! What every fixture test reads: the committed calibration, the recorded
//! frames copied as plain fixtures, the hand marks, and the outlines and
//! toolpaths the real `GcodeIndex` builds and spools from each case's own
//! G-code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use image::RgbImage;
use printobserver_core::{ExtrusionKind, GcodeIndex, IndexBuilder, LayerRef, LayerToolpath};
use printobserver_panels::CameraCalibration;
use printobserver_printer_api::GcodeSink as _;
use printobserver_types::serde_json::{self, Value};

/// The repository's root.
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The committed calibration's bytes.
pub fn calibration_bytes() -> Vec<u8> {
    std::fs::read(root().join("tests/real-prints/cameras/core-one-2026-09-27.json"))
        .expect("the committed calibration is there")
}

/// The committed calibration.
pub fn calibration() -> CameraCalibration {
    CameraCalibration::from_json(&calibration_bytes()).expect("the committed calibration is valid")
}

/// The fixtures directory.
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A recorded frame's bytes.
pub fn frame(case: &str, file: &str) -> Vec<u8> {
    std::fs::read(fixtures().join("frames").join(case).join(file))
        .unwrap_or_else(|error| panic!("{case}/{file}: {error}"))
}

/// A recorded frame, decoded.
pub fn decoded(case: &str, file: &str) -> RgbImage {
    image::load_from_memory(&frame(case, file))
        .expect("a recorded frame decodes")
        .to_rgb8()
}

/// The print height a frame was recorded at, from the fixtures' manifest.
pub fn recorded_z(case: &str, file: &str) -> f64 {
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(fixtures().join("frames.json")).expect("the manifest is there"),
    )
    .expect("the manifest parses");
    manifest["frames"]
        .as_array()
        .expect("frames")
        .iter()
        .find(|entry| entry["case"] == case && entry["file"] == file)
        .and_then(|entry| entry["z_mm"].as_f64())
        .unwrap_or_else(|| panic!("{case}/{file} records a height"))
}

/// The hand marks.
pub fn marks() -> Value {
    serde_json::from_slice(&std::fs::read(fixtures().join("marks.json")).expect("the marks"))
        .expect("the marks parse")
}

/// The snapshot URL the real-prints service configuration names, which the
/// committed calibration's frames came from.
pub fn snapshot_url() -> String {
    calibration().camera_snapshot_url
}

/// The plain copy of each case's slice.
fn gcode_of(case: &str) -> PathBuf {
    let root = root();
    let core = root.join("crates/printobserver-core/tests/fixtures/real-prints");
    match case {
        "jar-top-wrong-material-first-layer-detach" => {
            core.join("jar-top-first-layer-detach.gcode")
        }
        "jar-top-wrong-material-detach-spaghetti" => core.join("jar-top-brim.gcode"),
        "jar-top-petg-healthy" => core.join("jar-top-petg.gcode"),
        "fan-cut-bridge" => core.join("fan-cut-test.gcode"),
        "bubbler-guard-and-pad-pla-healthy" => root.join(
            "crates/printobserver-server/tests/fixtures/real-prints/bubbler-guard-and-pad.gcode",
        ),
        other => panic!("no slice for {other}"),
    }
}

/// A case's index, built once per test process by the real builder, its
/// G-code streamed in 64 KiB pieces and spooled as the server spools it.
pub fn index(case: &str) -> Arc<GcodeIndex> {
    static HELD: OnceLock<Mutex<BTreeMap<String, Arc<GcodeIndex>>>> = OnceLock::new();
    let held = HELD.get_or_init(Mutex::default);
    if let Some(index) = held.lock().expect("held").get(case) {
        return Arc::clone(index);
    }
    let spool = spool_dir().join(format!("{case}.gcode"));
    let mut builder = IndexBuilder::spooling(spool).expect("the spool is created");
    let bytes = std::fs::read(gcode_of(case)).expect("the slice is there");
    for piece in bytes.chunks(1 << 16) {
        let _ = builder.take(piece);
    }
    let index = Arc::new(builder.finish());
    held.lock()
        .expect("held")
        .insert(case.to_owned(), Arc::clone(&index));
    index
}

/// This process's spool directory.
fn spool_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "printobserver-panels-fixtures-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("the spool directory is made");
        dir
    })
}

/// The layer a frame recorded at height `z` shows: the last layer at that
/// height, else the layer laying the part nearest it. Chosen by Z rather than
/// by the recorded file position, because the copy `OctoPrint` served differs
/// from the committed upload by a few kilobytes.
pub fn layer_at_z(index: &GcodeIndex, z: f64) -> LayerRef {
    let layers: Vec<(LayerRef, f64)> = (0..index.layer_count())
        .map(LayerRef::of_index)
        .filter_map(|layer| index.layer_z_mm(layer).map(|held| (layer, held)))
        .collect();
    if let Some((layer, _)) = layers
        .iter()
        .rev()
        .find(|(_, held)| (held - z).abs() < 1e-3)
    {
        return *layer;
    }
    layers
        .iter()
        .filter(|(layer, _)| index.object_layer_at_or_below(*layer) == Some(*layer))
        .min_by(|a, b| (a.1 - z).abs().total_cmp(&(b.1 - z).abs()))
        .map(|(layer, _)| *layer)
        .expect("a layer near that height")
}

/// One layer's toolpath, read back from the spool.
pub fn toolpath(case: &str, layer: LayerRef) -> LayerToolpath {
    index(case)
        .layer_toolpath(layer)
        .expect("the layer reads back from the spool")
}

/// The segments of some kinds, in bed millimetres.
pub fn segments_of(toolpath: &LayerToolpath, kinds: &[ExtrusionKind]) -> Vec<[f64; 4]> {
    toolpath
        .segments
        .iter()
        .filter(|segment| kinds.contains(&segment.kind))
        .map(|segment| {
            [
                f64::from(segment.from_mm[0]),
                f64::from(segment.from_mm[1]),
                f64::from(segment.to_mm[0]),
                f64::from(segment.to_mm[1]),
            ]
        })
        .collect()
}

/// Where a pixel lands on the plane of a part's height `h` with the print at
/// `print_z`: the bed point whose projection is that pixel, by Gauss-Newton
/// over the calibration's own projection.
pub fn back_project(
    calibration: &CameraCalibration,
    pixel: [f64; 2],
    h: f64,
    print_z: f64,
) -> [f64; 2] {
    let size = [100_000, 100_000];
    let at = |p: [f64; 2]| {
        calibration.project(&[[p[0], p[1], h]], print_z, size)[0].expect("projects into view")
    };
    let mut point = [125.0, 110.0];
    for _ in 0..50 {
        let here = at(point);
        let residual = [here[0] - pixel[0], here[1] - pixel[1]];
        let step = 1e-3;
        let dx = at([point[0] + step, point[1]]);
        let dy = at([point[0], point[1] + step]);
        let j = [
            [(dx[0] - here[0]) / step, (dy[0] - here[0]) / step],
            [(dx[1] - here[1]) / step, (dy[1] - here[1]) / step],
        ];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        let delta = [
            (j[1][1] * residual[0] - j[0][1] * residual[1]) / det,
            (-j[1][0] * residual[0] + j[0][0] * residual[1]) / det,
        ];
        point = [point[0] - delta[0], point[1] - delta[1]];
        if delta[0].hypot(delta[1]) < 1e-6 {
            break;
        }
    }
    point
}

/// A point's distance from the nearest of some segments.
pub fn distance_to(point: [f64; 2], segments: &[[f64; 4]]) -> f64 {
    segments
        .iter()
        .map(|[x0, y0, x1, y1]| {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let length = dx * dx + dy * dy;
            let t = if length > 0.0 {
                (((point[0] - x0) * dx + (point[1] - y0) * dy) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (point[0] - (x0 + t * dx)).hypot(point[1] - (y0 + t * dy))
        })
        .fold(f64::INFINITY, f64::min)
}

/// The median of some values.
pub fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        f64::midpoint(values[n / 2 - 1], values[n / 2])
    }
}

/// Split segments into islands: each segment joins every other with an end
/// within `gap` millimetres of one of its own, transitively.
pub fn islands(segments: &[[f64; 4]], gap: f64) -> Vec<Vec<[f64; 4]>> {
    let near = |a: &[f64; 4], b: &[f64; 4]| {
        [[a[0], a[1]], [a[2], a[3]]].iter().any(|p| {
            [[b[0], b[1]], [b[2], b[3]]]
                .iter()
                .any(|q| (p[0] - q[0]).hypot(p[1] - q[1]) < gap)
        })
    };
    let mut label = vec![usize::MAX; segments.len()];
    let mut count = 0;
    for start in 0..segments.len() {
        if label[start] != usize::MAX {
            continue;
        }
        label[start] = count;
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            for (next, segment) in segments.iter().enumerate() {
                if label[next] == usize::MAX && near(&segments[at], segment) {
                    label[next] = count;
                    stack.push(next);
                }
            }
        }
        count += 1;
    }
    (0..count)
        .map(|island| {
            segments
                .iter()
                .zip(&label)
                .filter(|(_, held)| **held == island)
                .map(|(segment, _)| *segment)
                .collect()
        })
        .collect()
}

/// The island nearest a bed point.
pub fn island_nearest(islands: &[Vec<[f64; 4]>], point: [f64; 2]) -> Vec<[f64; 4]> {
    islands
        .iter()
        .min_by(|a, b| distance_to(point, a).total_cmp(&distance_to(point, b)))
        .expect("an island")
        .clone()
}

/// A JPEG of an image, as a camera would serve it.
pub fn jpeg_of(image: &RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
        .encode_image(image)
        .expect("a frame encodes");
    bytes
}
