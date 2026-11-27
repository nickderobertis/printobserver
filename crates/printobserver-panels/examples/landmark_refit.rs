//! Spike harness, never landed: how far a re-fit of the camera view from the
//! calibration's chassis landmarks recovers the planned outline on one
//! recorded print whose camera had shifted, and how strongly first-layer
//! alignment finds a layer that is there against one that is covered or
//! absent.
//!
//! ```console
//! cargo run --release -p printobserver-panels --example landmark_refit -- \
//!   --evidence <capture folder> --out <output folder> [--model translation]
//! ```
//!
//! The capture folder holds `agent-images/*.jpg` (the print's check frames,
//! named `HHMMSS-<kind>.jpg` in local time) and `composed-if-aligned/` with
//! `inputs.json` (each check's file position, in the frames' order) and
//! `octoprint-copy.gcode` (the G-code those positions index). The calibration
//! is the committed `core-one-2026-09-27.json`, and the controls are the
//! recorded fixture frames the alignment fixture test uses. It writes
//! `results.json` and one composed image per frame into the output folder,
//! and prints the tables the spike report quotes.
//!
//! Landmarks are located by the crate's own `drift::locate_landmarks` with
//! the radius widened; the alignment search is `alignment.rs`'s, copied here
//! because it keeps its score private, and is checked against the crate's own
//! verdict on every uncorrected frame.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use image::{GrayImage, Rgb, RgbImage, imageops::FilterType};
use imageproc::drawing::draw_line_segment_mut;
use nalgebra::{DMatrix, DVector, Matrix3, Vector3};
use printobserver_core::{ExtrusionKind, GcodeIndex, IndexBuilder, LayerRef, LayerToolpath};
use printobserver_panels::alignment::{BAND_PX, SAMPLE_PX, SEARCH_PX};
use printobserver_panels::drift::{HIDDEN_BELOW, Located, locate_landmarks};
use printobserver_panels::{
    AlignmentInputs, BedPose, CameraCalibration, verify_first_layer_alignment,
};
use printobserver_printer_api::GcodeSink as _;
use printobserver_types::serde_json::{self, Value, json};

/// The radii each landmark is searched at, up to the calibration's ceiling.
const RADII: [u32; 8] = [48, 64, 96, 128, 160, 192, 224, 256];
/// How close two landmarks' offsets must be to count as one movement.
const AGREE_PX: f64 = 25.0;
/// The widest search, the calibration's ceiling.
const MAX_RADIUS: u32 = printobserver_panels::calibration::MAX_SEARCH_RADIUS_PX;
/// The alignment search's shift spacing and smoothing, as `alignment.rs`.
const SHIFT_STEP: i32 = 2;
/// Shifts either side each alignment score is averaged over.
const SMOOTHING: i32 = 2;
/// The alignment's blur, as `alignment.rs`.
const BLUR_SIGMA: f32 = 1.5;
/// A second alignment peak counts as separate beyond this many pixels.
const SECOND_PEAK_APART_PX: i32 = 12;
/// The wider search the uncorrected projection is also measured with, to see
/// where the printed layer really lies.
const WIDE_SEARCH_PX: i32 = 160;
/// The longest piece a segment is projected as, as `geometry.rs`.
const PIECE_MM: f64 = 2.0;

/// The four image-space corrections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Model {
    /// Shift only.
    Translation,
    /// Shift, rotation and uniform scale.
    Similarity,
    /// Any linear map and shift.
    Affine,
    /// A plane-to-plane projective map.
    Homography,
}

impl Model {
    /// Every model, least to most freedom.
    const ALL: [Self; 4] = [
        Self::Translation,
        Self::Similarity,
        Self::Affine,
        Self::Homography,
    ];

    /// Its name on the command line and in the results.
    const fn name(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Similarity => "similarity",
            Self::Affine => "affine",
            Self::Homography => "homography",
        }
    }

    /// The fewest point pairs that determine it.
    const fn least_points(self) -> usize {
        match self {
            Self::Translation => 1,
            Self::Similarity => 2,
            Self::Affine => 3,
            Self::Homography => 4,
        }
    }

    /// Its colour in the composed images.
    const fn colour(self) -> Rgb<u8> {
        match self {
            Self::Translation => Rgb([0, 220, 255]),
            Self::Similarity => Rgb([0, 255, 60]),
            Self::Affine => Rgb([255, 230, 0]),
            Self::Homography => Rgb([255, 0, 255]),
        }
    }
}

/// One recorded check frame.
struct Frame {
    /// Its file stem, `HHMMSS-<kind>`.
    stem: String,
    /// Its local time, `HH:MM:SS`.
    local: String,
    /// The file position its check recorded.
    position: u64,
    /// Its JPEG bytes.
    jpeg: Vec<u8>,
    /// Decoded.
    rgb: RgbImage,
}

/// What the alignment search found.
#[derive(Debug, Clone, Copy)]
struct Found {
    /// The best shift's length, in pixels.
    offset_px: f64,
    /// The best shift.
    shift: [i32; 2],
    /// The smoothed contrast there, in grey levels.
    peak: f64,
    /// The median smoothed contrast over the search disc.
    median: f64,
    /// The best smoothed contrast more than [`SECOND_PEAK_APART_PX`] away.
    second: f64,
    /// The smoothed contrast at no shift.
    at_zero: f64,
    /// At the best shift, the path's and band's mean difference over their
    /// pooled standard deviation (Cohen's d): how cleanly the two separate,
    /// rather than how far apart their means lie.
    effect: f64,
    /// At the best shift, the band's standard deviation, in grey levels.
    band_std: f64,
}

impl Found {
    /// As JSON.
    fn json(&self) -> Value {
        json!({
            "offset_px": round(self.offset_px, 1),
            "shift_px": self.shift,
            "peak_contrast": round(self.peak, 2),
            "median_contrast": round(self.median, 2),
            "second_peak_contrast": round(self.second, 2),
            "contrast_at_zero": round(self.at_zero, 2),
            "peak_over_median": round(self.peak / self.median.max(1e-6), 2),
            "peak_over_second": round(self.peak / self.second.max(1e-6), 3),
            "effect_size": round(self.effect, 2),
            "band_std": round(self.band_std, 1),
        })
    }
}

/// A count as a float: every count here is far below `u32::MAX`.
fn count_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// The nearest whole pixel at or above 0 of a coordinate the caller has
/// already bounded inside a frame.
fn pixel(value: f64) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "callers clamp into a frame of a few thousand pixels, and `as` saturates anything else"
    )]
    let whole = value.round().max(0.0) as u32;
    whole
}

/// A value the drawing or the crate's `Located` takes as an f32: a panel
/// coordinate or a correlation, both well inside f32's range.
fn narrow(value: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a panel pixel or a correlation, both representable in f32 to the precision drawn or reported"
    )]
    let narrowed = value as f32;
    narrowed
}

/// Round for the report.
fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

/// The command line.
struct Args {
    /// The capture folder.
    evidence: PathBuf,
    /// Where results and images go.
    out: PathBuf,
    /// The landmark model panel 1 of each composed image draws.
    model: Model,
}

/// Read and validate the command line.
fn args() -> Result<Args, String> {
    let mut evidence = None;
    let mut out = None;
    let mut model = Model::Translation;
    let mut given = std::env::args().skip(1);
    while let Some(flag) = given.next() {
        let value = given
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--evidence" => evidence = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            "--model" => {
                model = Model::ALL
                    .into_iter()
                    .find(|m| m.name() == value)
                    .ok_or_else(|| format!("--model is one of translation, similarity, affine, homography, not {value}"))?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    let evidence = evidence.ok_or("--evidence <capture folder> is required")?;
    for needed in [
        "agent-images",
        "composed-if-aligned/inputs.json",
        "composed-if-aligned/octoprint-copy.gcode",
    ] {
        if !evidence.join(needed).exists() {
            return Err(format!("{} has no {needed}", evidence.display()));
        }
    }
    let out = out.ok_or("--out <output folder> is required")?;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(Args {
        evidence,
        out,
        model,
    })
}

/// The repository's root.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A calibration file, read and validated by the crate.
fn calibration_at(path: &Path) -> CameraCalibration {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    CameraCalibration::from_json(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The committed calibration with every landmark searched at `radius`.
fn widened(path: &Path, radius: u32) -> CameraCalibration {
    let mut value: Value =
        serde_json::from_slice(&std::fs::read(path).expect("the calibration reads"))
            .expect("the calibration parses");
    for landmark in value["landmarks"].as_array_mut().expect("landmarks") {
        landmark["search_radius_px"] = radius.into();
    }
    CameraCalibration::from_json(&serde_json::to_vec(&value).expect("serialises"))
        .expect("a widened calibration is valid")
}

/// An index over a G-code file, spooled so its layers read back.
fn index_of(gcode: &Path, spool: &Path) -> GcodeIndex {
    let mut builder = IndexBuilder::spooling(spool.to_path_buf()).expect("the spool is made");
    let bytes = std::fs::read(gcode).unwrap_or_else(|e| panic!("{}: {e}", gcode.display()));
    for piece in bytes.chunks(1 << 16) {
        let _ = builder.take(piece);
    }
    builder.finish()
}

/// The capture's frames, paired in order with `inputs.json`'s checks.
fn frames(evidence: &Path) -> Vec<Frame> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(evidence.join("agent-images"))
        .expect("agent-images reads")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    files.sort();
    let inputs: Value = serde_json::from_slice(
        &std::fs::read(evidence.join("composed-if-aligned/inputs.json")).expect("inputs.json"),
    )
    .expect("inputs.json parses");
    let inputs = inputs.as_array().expect("inputs.json is a list");
    assert_eq!(
        files.len(),
        inputs.len(),
        "one frame per recorded check is required"
    );
    files
        .iter()
        .zip(inputs)
        .map(|(path, input)| {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("a frame name")
                .to_owned();
            let hhmmss = &stem[..6];
            // inputs.json is UTC; the frames are named in UTC-6.
            let at = input["at"].as_str().expect("at");
            let utc_hour: u32 = at[11..13].parse().expect("an hour");
            let local_hour = (utc_hour + 24 - 6) % 24;
            assert_eq!(
                format!("{local_hour:02}{}{}", &at[14..16], &at[17..19]),
                hhmmss,
                "inputs.json and the frames pair in order"
            );
            let jpeg = std::fs::read(path).expect("a frame reads");
            let rgb = image::load_from_memory(&jpeg)
                .expect("a frame decodes")
                .to_rgb8();
            Frame {
                local: format!("{}:{}:{}", &hhmmss[..2], &hhmmss[2..4], &hhmmss[4..6]),
                stem,
                position: input["pos"].as_u64().expect("pos"),
                jpeg,
                rgb,
            }
        })
        .collect()
}

/// Each frame's landmarks at every radius, located by the crate's own
/// `drift::locate_landmarks`, frames in parallel: kept to show what the
/// crate's search does when widened.
fn crate_landmarks_by_radius(
    calibration_path: &Path,
    greys: &[GrayImage],
) -> Vec<BTreeMap<u32, Vec<Located>>> {
    let widened: Vec<(u32, CameraCalibration)> = RADII
        .iter()
        .map(|r| (*r, widened(calibration_path, *r)))
        .collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = greys
            .iter()
            .map(|grey| {
                let widened = &widened;
                scope.spawn(move || {
                    widened
                        .iter()
                        .map(|(r, calibration)| (*r, locate_landmarks(calibration, grey)))
                        .collect::<BTreeMap<_, _>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a search finishes"))
            .collect()
    })
}

/// Each landmark's stored patch, decoded.
fn patches(calibration: &CameraCalibration) -> Vec<GrayImage> {
    use base64::Engine as _;
    calibration
        .landmarks
        .iter()
        .map(|l| {
            let png = base64::engine::general_purpose::STANDARD
                .decode(&l.patch_png_base64)
                .expect("a base64 patch");
            image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                .expect("a PNG patch")
                .to_luma8()
        })
        .collect()
}

/// `drift.rs`'s normalized cross-correlation over every offset within
/// [`MAX_RADIUS`], in f64: the crate's f32 `squares - sum^2 / count` cancels
/// on near-flat windows and scores them above 1, which wins any search wide
/// enough to reach one. Offsets off the frame are `-inf`.
fn ncc_map(frame: &GrayImage, patch: &GrayImage, center: [f64; 2]) -> Vec<f64> {
    let radius = i64::from(MAX_RADIUS);
    let (pw, ph) = (i64::from(patch.width()), i64::from(patch.height()));
    let (fw, fh) = (i64::from(frame.width()), i64::from(frame.height()));
    let (left, top) = (
        i64::from(pixel(center[0])) - pw / 2,
        i64::from(pixel(center[1])) - ph / 2,
    );
    let template: Vec<f64> = patch.as_raw().iter().map(|v| f64::from(*v)).collect();
    let count = count_f64(template.len());
    let mean = template.iter().sum::<f64>() / count;
    let centred: Vec<f64> = template.iter().map(|v| v - mean).collect();
    let norm = centred.iter().map(|v| v * v).sum::<f64>().sqrt();
    let side = usize::try_from(2 * radius + 1).unwrap_or(0);
    let mut scores = vec![f64::NEG_INFINITY; side * side];
    let pixels = frame.as_raw();
    let width = usize::try_from(fw).unwrap_or(0);
    let pw_us = usize::try_from(pw).unwrap_or(0);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (x0, y0) = (left + dx, top + dy);
            if x0 < 0 || y0 < 0 || x0 + pw > fw || y0 + ph > fh {
                continue;
            }
            let (x0, y0) = (
                usize::try_from(x0).unwrap_or(0),
                usize::try_from(y0).unwrap_or(0),
            );
            let (mut sum, mut squares, mut cross) = (0.0_f64, 0.0_f64, 0.0_f64);
            for (row, weights) in centred.chunks_exact(pw_us).enumerate() {
                let start = (y0 + row) * width + x0;
                for (pixel, weight) in pixels[start..start + pw_us].iter().zip(weights) {
                    let value = f64::from(*pixel);
                    sum += value;
                    squares += value * value;
                    cross += value * weight;
                }
            }
            let spread = (squares - sum * sum / count).max(0.0).sqrt();
            if spread > 1e-3 {
                let at =
                    usize::try_from((dy + radius) * (2 * radius + 1) + dx + radius).unwrap_or(0);
                scores[at] = cross / (spread * norm);
            }
        }
    }
    scores
}

/// The best match within `radius` of a full map, refined as `drift.rs` does.
fn best_within(scores: &[f64], radius: u32) -> Located {
    let full = i64::from(MAX_RADIUS);
    let side = usize::try_from(2 * full + 1).unwrap_or(0);
    let r = i64::from(radius);
    let mut best: Option<(usize, f64)> = None;
    for dy in -r..=r {
        for dx in -r..=r {
            let at = usize::try_from((dy + full) * (2 * full + 1) + dx + full).unwrap_or(0);
            let score = scores[at];
            if score.is_finite() && best.is_none_or(|(_, b)| score > b) {
                best = Some((at, score));
            }
        }
    }
    let Some((at, score)) = best else {
        return Located {
            offset_px: [0.0, 0.0],
            score: -1.0,
        };
    };
    let (row, column) = (at / side, at % side);
    let refine = |a: f64, c: f64| {
        let curve = a - 2.0 * score + c;
        if a.is_finite() && c.is_finite() && curve.abs() > 1e-12 {
            0.5 * (a - c) / curve
        } else {
            0.0
        }
    };
    let get = |r: usize, c: usize| {
        scores
            .get(r * side + c)
            .copied()
            .unwrap_or(f64::NEG_INFINITY)
    };
    let sub_x = if column > 0 && column + 1 < side {
        refine(get(row, column - 1), get(row, column + 1))
    } else {
        0.0
    };
    let sub_y = if row > 0 && row + 1 < side {
        refine(get(row - 1, column), get(row + 1, column))
    } else {
        0.0
    };
    let whole = |i: usize| f64::from(u32::try_from(i).unwrap_or(0)) - f64::from(MAX_RADIUS);
    let score = narrow(score);
    Located {
        offset_px: [whole(column) + sub_x, whole(row) + sub_y],
        score,
    }
}

/// Each frame's landmarks at every radius, by [`ncc_map`], in parallel.
fn landmarks_by_radius(
    calibration: &CameraCalibration,
    greys: &[GrayImage],
) -> Vec<BTreeMap<u32, Vec<Located>>> {
    let patches = patches(calibration);
    std::thread::scope(|scope| {
        let handles: Vec<Vec<_>> = greys
            .iter()
            .map(|grey| {
                calibration
                    .landmarks
                    .iter()
                    .zip(&patches)
                    .map(|(l, patch)| {
                        let centre = l.center_px;
                        scope.spawn(move || ncc_map(grey, patch, centre))
                    })
                    .collect()
            })
            .collect();
        handles
            .into_iter()
            .map(|per_landmark| {
                let maps: Vec<Vec<f64>> = per_landmark
                    .into_iter()
                    .map(|h| h.join().expect("a search finishes"))
                    .collect();
                RADII
                    .iter()
                    .map(|r| (*r, maps.iter().map(|m| best_within(m, *r)).collect()))
                    .collect()
            })
            .collect()
    })
}

/// The largest set of found landmarks whose offsets agree within
/// [`AGREE_PX`] of one of them, ties to the higher summed score: one moved
/// chassis seen through several landmarks, rather than false matches.
fn consistent(found: &[Located]) -> Vec<usize> {
    let visible: Vec<usize> = (0..found.len())
        .filter(|k| found[*k].score >= HIDDEN_BELOW)
        .collect();
    let mut best: (Vec<usize>, f32) = (Vec::new(), f32::NEG_INFINITY);
    for seed in &visible {
        let near: Vec<usize> = visible
            .iter()
            .copied()
            .filter(|k| {
                let (a, b) = (found[*seed].offset_px, found[*k].offset_px);
                (a[0] - b[0]).hypot(a[1] - b[1]) <= AGREE_PX
            })
            .collect();
        let score: f32 = near.iter().map(|k| found[*k].score).sum();
        if near.len() > best.0.len() || (near.len() == best.0.len() && score > best.1) {
            best = (near, score);
        }
    }
    best.0
}

/// Points scaled about their mean so fits are well conditioned: the scaled
/// points and the map back.
fn normaliser(points: &[[f64; 2]]) -> Matrix3<f64> {
    let n = count_f64(points.len());
    let mean = points
        .iter()
        .fold([0.0, 0.0], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n]);
    let spread = points
        .iter()
        .map(|p| (p[0] - mean[0]).hypot(p[1] - mean[1]))
        .sum::<f64>()
        / n;
    let s = if spread > 1e-9 {
        std::f64::consts::SQRT_2 / spread
    } else {
        1.0
    };
    Matrix3::new(s, 0.0, -s * mean[0], 0.0, s, -s * mean[1], 0.0, 0.0, 1.0)
}

/// Apply a 3x3 map to a point.
fn apply(map: &Matrix3<f64>, p: [f64; 2]) -> [f64; 2] {
    let q = map * Vector3::new(p[0], p[1], 1.0);
    [q[0] / q[2], q[1] / q[2]]
}

/// Fit a model mapping `from` onto `to`; nothing when too few pairs.
fn fit(model: Model, from: &[[f64; 2]], to: &[[f64; 2]]) -> Option<Matrix3<f64>> {
    if from.len() < model.least_points() {
        return None;
    }
    if model == Model::Translation {
        let count = count_f64(from.len());
        let shift = from.iter().zip(to).fold([0.0, 0.0], |s, (p, q)| {
            [s[0] + (q[0] - p[0]) / count, s[1] + (q[1] - p[1]) / count]
        });
        return Some(Matrix3::new(
            1.0, 0.0, shift[0], 0.0, 1.0, shift[1], 0.0, 0.0, 1.0,
        ));
    }
    let (nf, nt) = (normaliser(from), normaliser(to));
    let src: Vec<[f64; 2]> = from.iter().map(|p| apply(&nf, *p)).collect();
    let dst: Vec<[f64; 2]> = to.iter().map(|p| apply(&nt, *p)).collect();
    let n = src.len();
    let normalised = match model {
        // A shift is fitted in pixels, before normalising, above.
        Model::Translation => unreachable!("fitted above"),
        Model::Similarity => {
            let mut m = DMatrix::zeros(2 * n, 4);
            let mut y = DVector::zeros(2 * n);
            for (i, (p, q)) in src.iter().zip(&dst).enumerate() {
                m.row_mut(2 * i).copy_from_slice(&[p[0], -p[1], 1.0, 0.0]);
                m.row_mut(2 * i + 1)
                    .copy_from_slice(&[p[1], p[0], 0.0, 1.0]);
                y[2 * i] = q[0];
                y[2 * i + 1] = q[1];
            }
            let x = m.svd(true, true).solve(&y, 1e-12).ok()?;
            Matrix3::new(x[0], -x[1], x[2], x[1], x[0], x[3], 0.0, 0.0, 1.0)
        }
        Model::Affine => {
            let mut m = DMatrix::zeros(2 * n, 6);
            let mut y = DVector::zeros(2 * n);
            for (i, (p, q)) in src.iter().zip(&dst).enumerate() {
                m.row_mut(2 * i)
                    .copy_from_slice(&[p[0], p[1], 1.0, 0.0, 0.0, 0.0]);
                m.row_mut(2 * i + 1)
                    .copy_from_slice(&[0.0, 0.0, 0.0, p[0], p[1], 1.0]);
                y[2 * i] = q[0];
                y[2 * i + 1] = q[1];
            }
            let x = m.svd(true, true).solve(&y, 1e-12).ok()?;
            Matrix3::new(x[0], x[1], x[2], x[3], x[4], x[5], 0.0, 0.0, 1.0)
        }
        Model::Homography => {
            let mut m = DMatrix::zeros((2 * n).max(9), 9);
            for (i, (p, q)) in src.iter().zip(&dst).enumerate() {
                m.row_mut(2 * i).copy_from_slice(&[
                    -p[0],
                    -p[1],
                    -1.0,
                    0.0,
                    0.0,
                    0.0,
                    q[0] * p[0],
                    q[0] * p[1],
                    q[0],
                ]);
                m.row_mut(2 * i + 1).copy_from_slice(&[
                    0.0,
                    0.0,
                    0.0,
                    -p[0],
                    -p[1],
                    -1.0,
                    q[1] * p[0],
                    q[1] * p[1],
                    q[1],
                ]);
            }
            let svd = m.svd(false, true);
            let v = svd.v_t?;
            let (smallest, _) = svd
                .singular_values
                .iter()
                .enumerate()
                .min_by(|x, y| x.1.total_cmp(y.1))?;
            let h = v.row(smallest);
            Matrix3::new(h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8])
        }
    };
    let back = nt.try_inverse()?;
    Some(back * normalised * nf)
}

/// A fitted model's error at its own points and left out one at a time.
fn residuals(model: Model, from: &[[f64; 2]], to: &[[f64; 2]]) -> Value {
    let Some(map) = fit(model, from, to) else {
        return json!({"fitted": false});
    };
    let errors: Vec<f64> = from
        .iter()
        .zip(to)
        .map(|(p, q)| {
            let r = apply(&map, *p);
            (r[0] - q[0]).hypot(r[1] - q[1])
        })
        .collect();
    let mut left_out = Vec::new();
    let trained_on = from.len() - 1;
    for i in 0..from.len() {
        let (f, t): (Vec<_>, Vec<_>) = from
            .iter()
            .zip(to)
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, (p, q))| (*p, *q))
            .unzip();
        if let Some(map) = fit(model, &f, &t) {
            let r = apply(&map, from[i]);
            left_out.push((r[0] - to[i][0]).hypot(r[1] - to[i][1]));
        }
    }
    // A fit left with fewer points than its model needs predicts nothing: it
    // is reported as underdetermined, never given a number.
    let status = if left_out.len() == from.len() {
        "computed".to_owned()
    } else {
        format!(
            "underdetermined: each leave-one-out fit has {trained_on} training points and a {} needs {}",
            model.name(),
            model.least_points()
        )
    };
    json!({
        "fitted": true,
        "points": from.len(),
        "landmark_each_px": errors.iter().map(|e| round(*e, 1)).collect::<Vec<_>>(),
        "leave_one_out_status": status,
        "landmark_rms_px": round(rms(&errors), 2),
        "landmark_max_px": round(errors.iter().copied().fold(0.0, f64::max), 2),
        "leave_one_out_rms_px": (left_out.len() == from.len()).then(|| round(rms(&left_out), 2)),
        "leave_one_out_max_px": (left_out.len() == from.len()).then(|| round(left_out.iter().copied().fold(0.0, f64::max), 2)),
        "leave_one_out_each_px": left_out.iter().map(|e| round(*e, 1)).collect::<Vec<_>>(),
    })
}

/// Root mean square.
fn rms(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let n = count_f64(values.len());
    (values.iter().map(|v| v * v).sum::<f64>() / n).sqrt()
}

/// A layer projected with the print at `print_z`, in `geometry.rs`'s pieces:
/// each segment of a drawn kind (all kinds when `all`) as pixel pieces.
fn project_pieces(
    calibration: &CameraCalibration,
    layer: &LayerToolpath,
    print_z: f64,
    all: bool,
) -> Vec<[[f64; 2]; 2]> {
    let mut out = Vec::new();
    let z = f64::from(layer.z_mm);
    for segment in &layer.segments {
        if !all && matches!(segment.kind, ExtrusionKind::Infill) {
            continue;
        }
        let from = segment.from_mm.map(f64::from);
        let to = segment.to_mm.map(f64::from);
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let pieces = (length / PIECE_MM).ceil().clamp(1.0, 1024.0);
        let mut points: Vec<[f64; 3]> = Vec::new();
        let mut k = 0.0;
        while k <= pieces {
            let t = k / pieces;
            points.push([
                from[0] + t * (to[0] - from[0]),
                from[1] + t * (to[1] - from[1]),
                z,
            ]);
            k += 1.0;
        }
        let projected = calibration.project(&points, print_z, [100_000, 100_000]);
        for pair in projected.windows(2) {
            if let (Some(a), Some(b)) = (pair[0], pair[1]) {
                out.push([a, b]);
            }
        }
    }
    out
}

/// Points every `step` pixels along pieces, as `geometry.rs`.
fn sample_along(pieces: &[[[f64; 2]; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut points = Vec::new();
    for [from, to] in pieces {
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let count = (length / step).floor().max(1.0);
        let mut k = 0.0;
        while k < count {
            let t = k / count;
            points.push([
                from[0] + t * (to[0] - from[0]),
                from[1] + t * (to[1] - from[1]),
            ]);
            k += 1.0;
        }
    }
    points
}

/// Andrew's monotone chain, as `geometry.rs`.
fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
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
        let ordered: Vec<&[f64; 2]> = if pass {
            sorted.iter().rev().collect()
        } else {
            sorted.iter().collect()
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

/// The band outside a hull, as `geometry.rs`.
fn band_outside(hull: &[[f64; 2]], out: f64, step: f64) -> Vec<[f64; 2]> {
    if hull.len() < 2 {
        return Vec::new();
    }
    let count = count_f64(hull.len());
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

/// The mean grey at shifted points, clamped into the frame.
fn mean_at(grey: &GrayImage, points: &[[f64; 2]], dx: i32, dy: i32) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let (w, h) = (f64::from(grey.width() - 1), f64::from(grey.height() - 1));
    let mut sum = 0.0;
    for [x, y] in points {
        let (px, py) = (
            pixel((x + f64::from(dx)).clamp(0.0, w)),
            pixel((y + f64::from(dy)).clamp(0.0, h)),
        );
        sum += f64::from(grey.get_pixel(px, py).0[0]);
    }
    let n = count_f64(points.len());
    sum / n
}

/// `alignment.rs`'s search over a blurred grey frame, at a chosen radius,
/// keeping the score: the path's samples already corrected. Nothing when
/// fewer than three samples land in the frame.
fn align(blurred: &GrayImage, samples: &[[f64; 2]], search: i32) -> Option<Found> {
    let (w, h) = (f64::from(blurred.width()), f64::from(blurred.height()));
    let path: Vec<[f64; 2]> = samples
        .iter()
        .copied()
        .filter(|[x, y]| (0.0..w).contains(x) && (0.0..h).contains(y))
        .collect();
    if path.len() < 3 {
        return None;
    }
    let band = band_outside(&convex_hull(&path), BAND_PX, SAMPLE_PX);
    let side = usize::try_from(2 * search / SHIFT_STEP + 1).ok()?;
    let index = |shift: i32| usize::try_from((shift + search) / SHIFT_STEP).unwrap_or(0);
    let shifts = || (-search..=search).step_by(usize::try_from(SHIFT_STEP).unwrap_or(1));
    let mut contrast = vec![0.0_f64; side * side];
    for dx in shifts() {
        for dy in shifts() {
            contrast[index(dy) * side + index(dx)] =
                (mean_at(blurred, &path, dx, dy) - mean_at(blurred, &band, dx, dy)).abs();
        }
    }
    let mut smoothed: Vec<(f64, i32, i32)> = Vec::new();
    for dx in shifts() {
        for dy in shifts() {
            if dx * dx + dy * dy > search * search {
                continue;
            }
            let (mut sum, mut count) = (0.0, 0.0);
            for ox in -SMOOTHING..=SMOOTHING {
                for oy in -SMOOTHING..=SMOOTHING {
                    let clamp = |v: usize, o: i32| {
                        (v.cast_signed() + isize::try_from(o).unwrap_or(0))
                            .clamp(0, side.cast_signed() - 1)
                            .cast_unsigned()
                    };
                    sum += contrast[clamp(index(dy), oy) * side + clamp(index(dx), ox)];
                    count += 1.0;
                }
            }
            smoothed.push((sum / count, dx, dy));
        }
    }
    // The first strictly greatest in `alignment.rs`'s dx-outer order.
    let best =
        smoothed.iter().copied().fold(
            (f64::NEG_INFINITY, 0, 0),
            |b, n| if n.0 > b.0 { n } else { b },
        );
    let mut values: Vec<f64> = smoothed.iter().map(|s| s.0).collect();
    values.sort_by(f64::total_cmp);
    let median = values[values.len() / 2];
    let apart = SECOND_PEAK_APART_PX * SECOND_PEAK_APART_PX;
    let second = smoothed
        .iter()
        .filter(|(_, dx, dy)| (dx - best.1).pow(2) + (dy - best.2).pow(2) > apart)
        .map(|s| s.0)
        .fold(0.0, f64::max);
    let at_zero = smoothed
        .iter()
        .find(|(_, dx, dy)| *dx == 0 && *dy == 0)
        .map_or(0.0, |s| s.0);
    let spread = |points: &[[f64; 2]]| {
        let values: Vec<f64> = points
            .iter()
            .map(|p| mean_at(blurred, std::slice::from_ref(p), best.1, best.2))
            .collect();
        let n = count_f64(values.len().max(1));
        let mean = values.iter().sum::<f64>() / n;
        (
            mean,
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n,
        )
    };
    let ((path_mean, path_var), (band_mean, band_var)) = (spread(&path), spread(&band));
    let effect = (path_mean - band_mean).abs() / f64::midpoint(path_var, band_var).sqrt().max(1e-6);
    Some(Found {
        effect,
        band_std: band_var.sqrt(),
        offset_px: f64::from(best.1).hypot(f64::from(best.2)),
        shift: [best.1, best.2],
        peak: best.0,
        median,
        second,
        at_zero,
    })
}

/// The blurred grey an alignment reads.
fn blurred_of(rgb: &RgbImage) -> GrayImage {
    imageproc::filter::gaussian_blur_f32(&image::imageops::grayscale(rgb), BLUR_SIGMA)
}

/// The first object layer's samples with the print at `print_z`, corrected.
fn corrected_samples(
    calibration: &CameraCalibration,
    first: &LayerToolpath,
    print_z: f64,
    map: &Matrix3<f64>,
) -> Vec<[f64; 2]> {
    sample_along(
        &project_pieces(calibration, first, print_z, true),
        SAMPLE_PX,
    )
    .into_iter()
    .map(|p| apply(map, p))
    .collect()
}

/// The control cases: `alignment.rs`'s fixture test's four.
const CONTROLS: [(&str, &str, &str, &str); 4] = [
    (
        "jar-top-wrong-material-first-layer-detach",
        "agent-007-obs-182248-p00.3.jpg",
        "agent-001-obs-181445-p00.0.jpg",
        "crates/printobserver-core/tests/fixtures/real-prints/jar-top-first-layer-detach.gcode",
    ),
    (
        "jar-top-wrong-material-detach-spaghetti",
        "agent-006-obs-191658-p04.3.jpg",
        "agent-001-obs-190853-p00.0.jpg",
        "crates/printobserver-core/tests/fixtures/real-prints/jar-top-brim.gcode",
    ),
    (
        "jar-top-petg-healthy",
        "agent-007-obs-121230-p01.3.jpg",
        "agent-001-obs-120024-p00.0.jpg",
        "crates/printobserver-core/tests/fixtures/real-prints/jar-top-petg.gcode",
    ),
    (
        "bubbler-guard-and-pad-pla-healthy",
        "agent-005-obs-165325-p01.7.jpg",
        "agent-001-obs-164721-p00.0.jpg",
        "crates/printobserver-server/tests/fixtures/real-prints/bubbler-guard-and-pad.gcode",
    ),
];

/// The mean pixel move of a first layer's projection between calibrations.
fn moved_px(a: &CameraCalibration, b: &CameraCalibration, first: &LayerToolpath, z: f64) -> f64 {
    let points: Vec<[f64; 3]> = first
        .segments
        .iter()
        .step_by(7)
        .map(|s| {
            [
                f64::from(s.from_mm[0]),
                f64::from(s.from_mm[1]),
                f64::from(first.z_mm),
            ]
        })
        .collect();
    let size = [100_000, 100_000];
    let moves: Vec<f64> = a
        .project(&points, z, size)
        .iter()
        .zip(&b.project(&points, z, size))
        .filter_map(|(p, q)| Some((p.as_ref()?, q.as_ref()?)))
        .map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]))
        .collect();
    let n = count_f64(moves.len());
    moves.iter().sum::<f64>() / n
}

/// The calibration with its bed moved along the camera's X until the first
/// layer's projection moves `target` pixels, as the fixture test does.
fn moved_by(
    c: &CameraCalibration,
    first: &LayerToolpath,
    z: f64,
    target: f64,
) -> CameraCalibration {
    let at = |shift: f64| {
        let mut t = c.bed_pose.translation_mm;
        t[0] += shift;
        c.with_bed_pose(BedPose {
            rotation_rodrigues: c.bed_pose.rotation_rodrigues,
            translation_mm: t,
        })
    };
    let (mut low, mut high) = (0.0, 40.0);
    for _ in 0..60 {
        let mid = f64::midpoint(low, high);
        if moved_px(c, &at(mid), first, z) < target {
            low = mid;
        } else {
            high = mid;
        }
    }
    at(low)
}

/// The recorded height of a fixture frame.
fn fixture_z(case: &str, file: &str) -> f64 {
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(root().join("crates/printobserver-panels/tests/fixtures/frames.json"))
            .expect("frames.json"),
    )
    .expect("frames.json parses");
    manifest["frames"]
        .as_array()
        .expect("frames")
        .iter()
        .find(|e| e["case"] == case && e["file"] == file)
        .and_then(|e| e["z_mm"].as_f64())
        .expect("a recorded height")
}

/// The healthy, empty-bed and moved-projection controls.
fn controls(calibration: &CameraCalibration, spool_dir: &Path) -> Value {
    let identity = Matrix3::identity();
    let mut rows = Vec::new();
    for (case, past, empty, gcode) in CONTROLS {
        let index = index_of(
            &root().join(gcode),
            &spool_dir.join(format!("{case}.gcode")),
        );
        let first = index
            .layer_toolpath(index.first_object_layer().expect("a first layer").0)
            .expect("reads back");
        let z = fixture_z(case, past);
        let read = |file: &str| {
            image::open(
                root()
                    .join("crates/printobserver-panels/tests/fixtures/frames")
                    .join(case)
                    .join(file),
            )
            .expect("a fixture frame")
            .to_rgb8()
        };
        let (healthy, bare) = (blurred_of(&read(past)), blurred_of(&read(empty)));
        let samples = corrected_samples(calibration, &first, z, &identity);
        let wrong = moved_by(calibration, &first, z, 49.0);
        let moved = corrected_samples(&wrong, &first, z, &identity);
        let found = |grey: &GrayImage, s: &[[f64; 2]]| {
            align(grey, s, SEARCH_PX).map_or(Value::Null, |f| f.json())
        };
        rows.push(json!({
            "case": case,
            "healthy": found(&healthy, &samples),
            "empty_bed": found(&bare, &samples),
            "moved_49px": found(&healthy, &moved),
            "moved_49px_wide_120px": align(&healthy, &moved, 120).map_or(Value::Null, |f| f.json()),
            "empty_bed_wide_120px": align(&bare, &samples, 120).map_or(Value::Null, |f| f.json()),
        }));
    }
    Value::Array(rows)
}

/// Draw pieces into a panel scaled from source pixels by `scale` after
/// subtracting `origin`.
fn draw(
    panel: &mut RgbImage,
    pieces: &[[[f64; 2]; 2]],
    map: &Matrix3<f64>,
    origin: [f64; 2],
    scale: f64,
    colour: Rgb<u8>,
) {
    let at = |p: [f64; 2]| {
        let q = apply(map, p);
        (
            narrow((q[0] - origin[0]) * scale),
            narrow((q[1] - origin[1]) * scale),
        )
    };
    for [a, b] in pieces {
        draw_line_segment_mut(panel, at(*a), at(*b), colour);
    }
}

/// The colour the alignment's own found shift is drawn in.
const FOUND_SHIFT: Rgb<u8> = Rgb([255, 40, 40]);

/// The composed image for one frame: the whole frame with the uncorrected
/// outline in grey and each of `top`'s corrections over it, above a 2x2 of
/// close-ups, one per entry of `tiles`, each centred on that correction's
/// outline with the uncorrected one in grey beneath.
fn compose(
    rgb: &RgbImage,
    pieces: &[[[f64; 2]; 2]],
    top: &[(Matrix3<f64>, Rgb<u8>)],
    tiles: &[(Matrix3<f64>, Rgb<u8>)],
) -> RgbImage {
    let identity = Matrix3::identity();
    let grey = Rgb([150, 150, 150]);
    let mut image = RgbImage::from_pixel(1024, 1152, Rgb([24, 24, 24]));
    let scale = 1024.0 / f64::from(rgb.width());
    let mut whole = image::imageops::resize(rgb, 1024, 576, FilterType::Triangle);
    draw(&mut whole, pieces, &identity, [0.0, 0.0], scale, grey);
    for (map, colour) in top {
        draw(&mut whole, pieces, map, [0.0, 0.0], scale, *colour);
    }
    image::imageops::replace(&mut image, &whole, 0, 0);
    let centre = {
        let pts: Vec<[f64; 2]> = pieces.iter().map(|p| p[0]).collect();
        let n = count_f64(pts.len().max(1));
        pts.iter()
            .fold([0.0, 0.0], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n])
    };
    for (k, (map, colour)) in tiles.iter().take(4).enumerate() {
        let c = apply(map, centre);
        let (cw, ch) = (384.0, 216.0);
        let x0 = (c[0] - cw / 2.0).clamp(0.0, f64::from(rgb.width()) - cw);
        let y0 = (c[1] - ch / 2.0).clamp(0.0, f64::from(rgb.height()) - ch);
        let crop = image::imageops::crop_imm(rgb, pixel(x0), pixel(y0), 384, 216).to_image();
        let mut tile = image::imageops::resize(&crop, 512, 288, FilterType::Triangle);
        let s = 512.0 / cw;
        draw(&mut tile, pieces, &identity, [x0, y0], s, grey);
        draw(&mut tile, pieces, map, [x0, y0], s, *colour);
        let (tx, ty) = (512 * (k % 2), 576 + 288 * (k / 2));
        image::imageops::replace(
            &mut image,
            &tile,
            i64::try_from(tx).unwrap_or(0),
            i64::try_from(ty).unwrap_or(0),
        );
    }
    image
}

#[expect(
    clippy::too_many_lines,
    reason = "a throwaway spike harness, read top to bottom in the order the report states its measurements"
)]
fn main() {
    let args = match args() {
        Ok(args) => args,
        Err(why) => {
            eprintln!("landmark_refit: {why}");
            std::process::exit(2);
        }
    };
    let calibration_path = root().join("tests/real-prints/cameras/core-one-2026-09-27.json");
    let calibration = calibration_at(&calibration_path);
    let snapshot = calibration.camera_snapshot_url.clone();
    let spool_dir = args.out.join("spool");
    std::fs::create_dir_all(&spool_dir).expect("the spool folder is made");
    let index = index_of(
        &args
            .evidence
            .join("composed-if-aligned/octoprint-copy.gcode"),
        &spool_dir.join("print.gcode"),
    );
    let (first_ref, first_ends) = index.first_object_layer().expect("a first object layer");
    let first = index
        .layer_toolpath(first_ref)
        .expect("the first layer reads back");
    println!(
        "first object layer: index {} at Z {:.2} mm, {} segments, ends at byte {first_ends}",
        first_ref.index(),
        first.z_mm,
        first.segments.len()
    );

    let frames = frames(&args.evidence);
    let greys: Vec<GrayImage> = frames
        .iter()
        .map(|f| image::imageops::grayscale(&f.rgb))
        .collect();
    let blurred: Vec<GrayImage> = frames.iter().map(|f| blurred_of(&f.rgb)).collect();
    eprintln!(
        "locating landmarks at {} radii over {} frames",
        RADII.len(),
        frames.len()
    );
    let located = landmarks_by_radius(&calibration, &greys);
    let crate_located = crate_landmarks_by_radius(&calibration_path, &greys);
    let names: Vec<&str> = calibration
        .landmarks
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    let centres: Vec<[f64; 2]> = calibration.landmarks.iter().map(|l| l.center_px).collect();
    let first_centre = {
        let s = sample_along(
            &project_pieces(&calibration, &first, f64::from(first.z_mm), true),
            SAMPLE_PX,
        );
        let n = count_f64(s.len());
        s.iter()
            .fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n])
    };
    println!(
        "first layer's projected centre: ({:.0}, {:.0})",
        first_centre[0], first_centre[1]
    );

    let mut results = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        let at = |r: u32| &located[i][&r];
        let widest = at(256);
        // Per landmark: offset and score at every radius, and the smallest
        // radius at which it is found where the widest search finds it.
        let mut per_landmark = Vec::new();
        let mut all_found_at: Option<u32> = None;
        for r in RADII {
            let all = at(r).iter().zip(widest).all(|(l, w)| {
                l.score >= HIDDEN_BELOW
                    && (l.offset_px[0] - w.offset_px[0]).hypot(l.offset_px[1] - w.offset_px[1])
                        < 1.0
            });
            if all && all_found_at.is_none() {
                all_found_at = Some(r);
            }
        }
        for (k, name) in names.iter().enumerate() {
            let w = widest[k];
            let smallest = RADII.iter().copied().find(|r| {
                let l = at(*r)[k];
                l.score >= HIDDEN_BELOW
                    && (l.offset_px[0] - w.offset_px[0]).hypot(l.offset_px[1] - w.offset_px[1])
                        < 1.0
            });
            per_landmark.push(json!({
                "name": name,
                "at_radius": RADII.iter().map(|r| {
                    let l = at(*r)[k];
                    json!({"radius_px": r, "offset_px": [round(l.offset_px[0], 1), round(l.offset_px[1], 1)], "score": round(f64::from(l.score), 3)})
                }).collect::<Vec<_>>(),
                "offset_px": [round(w.offset_px[0], 1), round(w.offset_px[1], 1)],
                "score": round(f64::from(w.score), 3),
                "found": w.score >= HIDDEN_BELOW,
                "smallest_radius_found_px": smallest,
                "crate_search": ([48, 256].iter().map(|r| {
                    let l = crate_located[i][r][k];
                    json!({"radius_px": r, "offset_px": [round(l.offset_px[0], 1), round(l.offset_px[1], 1)], "score": round(f64::from(l.score), 3)})
                }).collect::<Vec<_>>()),
            }));
        }
        let pairs = |keep: &[usize]| -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
            keep.iter()
                .map(|k| {
                    let (c, l) = (centres[*k], widest[*k]);
                    (c, [c[0] + l.offset_px[0], c[1] + l.offset_px[1]])
                })
                .unzip()
        };
        let all_found: Vec<usize> = (0..widest.len())
            .filter(|k| widest[*k].score >= HIDDEN_BELOW)
            .collect();
        let agreeing = consistent(widest);
        let fits_of = |keep: &[usize]| {
            let (from, to) = pairs(keep);
            let mut maps = BTreeMap::new();
            let mut fits = serde_json::Map::new();
            for model in Model::ALL {
                let mut entry = residuals(model, &from, &to);
                if let Some(map) = fit(model, &from, &to) {
                    let moved = apply(&map, first_centre);
                    entry["moves_first_layer_centre_px"] = json!([
                        round(moved[0] - first_centre[0], 1),
                        round(moved[1] - first_centre[1], 1)
                    ]);
                    maps.insert(model, map);
                }
                fits.insert(model.name().to_owned(), entry);
            }
            (maps, fits)
        };
        let (_, fits_all_found) = fits_of(&all_found);
        let all_four: Vec<usize> = (0..widest.len()).collect();
        let (maps_all_four, fits_all_four) = fits_of(&all_four);
        // Each candidate's standing in the four-landmark fit.
        for (k, entry) in per_landmark.iter_mut().enumerate() {
            entry["reliability"] = if agreeing.len() >= 2 && agreeing.contains(&k) {
                "reliable: agrees with another found landmark within 25 px".into()
            } else if widest[k].score < HIDDEN_BELOW {
                format!(
                    "unreliable: best score {:.2} is below {HIDDEN_BELOW}",
                    widest[k].score
                )
                .into()
            } else {
                "unreliable: found, but its offset disagrees with the agreeing landmarks by more than 25 px".into()
            };
        }
        let (maps, fits) = fits_of(&agreeing);

        // The layer the frame shows and the print height its check recorded.
        let layer = index.layer_at(frame.position);
        let print_z = layer
            .and_then(|l| index.layer_z_mm(l))
            .unwrap_or(f64::from(first.z_mm))
            .max(f64::from(first.z_mm));
        let past_first = frame.position > first_ends;
        let identity = Matrix3::identity();
        let samples = |map: &Matrix3<f64>| corrected_samples(&calibration, &first, print_z, map);
        let samples_at = |z: f64| corrected_samples(&calibration, &first, z, &Matrix3::identity());
        let production = verify_first_layer_alignment(&AlignmentInputs {
            frame_jpeg: &frame.jpeg,
            camera_snapshot_url: &snapshot,
            first_layer: &first,
            print_z_mm: print_z,
            calibration: &calibration,
        })
        .ok();
        let uncorrected = align(&blurred[i], &samples(&identity), SEARCH_PX);
        if let (Some(p), Some(u)) = (production, uncorrected) {
            assert!(
                (p.offset_px - u.offset_px).abs() < 1e-6,
                "{}: the copied search ({:.1}) differs from the crate's ({:.1})",
                frame.stem,
                u.offset_px,
                p.offset_px
            );
        }
        let wide = align(&blurred[i], &samples(&identity), WIDE_SEARCH_PX);
        let mut corrected_all_four = serde_json::Map::new();
        for (model, map) in &maps_all_four {
            corrected_all_four.insert(
                model.name().to_owned(),
                align(&blurred[i], &samples(map), SEARCH_PX).map_or(Value::Null, |f| f.json()),
            );
        }
        let mut corrected = serde_json::Map::new();
        for (model, map) in &maps {
            corrected.insert(
                model.name().to_owned(),
                align(&blurred[i], &samples(map), SEARCH_PX).map_or(Value::Null, |f| f.json()),
            );
        }

        // The bed height the layer is clearest at, for frames whose bed is not
        // at the layer's height (a pause parks it).
        let z_search = past_first
            .then(|| {
                (0..=125)
                    .filter_map(|z| {
                        let z = f64::from(z) * 2.0;
                        align(&blurred[i], &samples_at(z), SEARCH_PX).map(|f| (z, f))
                    })
                    .max_by(|a, b| a.1.peak.total_cmp(&b.1.peak))
            })
            .flatten();
        let found_shift = uncorrected.map(|f| {
            Matrix3::new(
                1.0,
                0.0,
                f64::from(f.shift[0]),
                0.0,
                1.0,
                f64::from(f.shift[1]),
                0.0,
                0.0,
                1.0,
            )
        });
        let shown = layer
            .and_then(|l| index.object_layer_at_or_below(l))
            .unwrap_or(first_ref);
        let toolpath = index.layer_toolpath(shown).expect("the layer reads back");
        let pieces = project_pieces(&calibration, &toolpath, print_z, false);
        let mut top = Vec::new();
        if let Some(map) = maps.get(&args.model) {
            top.push((*map, args.model.colour()));
        }
        if let Some(map) = found_shift {
            top.push((map, FOUND_SHIFT));
        }
        let mut tiles: Vec<(Matrix3<f64>, Rgb<u8>)> =
            [Model::Translation, Model::Similarity, Model::Affine]
                .iter()
                .filter_map(|m| maps.get(m).map(|map| (*map, m.colour())))
                .collect();
        if let Some(map) = found_shift {
            tiles.push((map, FOUND_SHIFT));
        }
        compose(&frame.rgb, &pieces, &top, &tiles)
            .save(args.out.join(format!("{}.jpg", frame.stem)))
            .expect("the composed image is written");

        results.push(json!({
            "frame": format!("{}.jpg", frame.stem),
            "local_time": frame.local,
            "file_position": frame.position,
            "layer_index": layer.map(LayerRef::index),
            "print_z_mm": round(print_z, 2),
            "past_first_layer": past_first,
            "landmarks": per_landmark,
            "smallest_radius_all_four_px": all_found_at,
            "fit_landmarks": agreeing.iter().map(|k| names[*k]).collect::<Vec<_>>(),
            "fits": fits,
            "fits_all_found": fits_all_found,
            "fits_all_four": fits_all_four,
            "alignment": {
                "crate_verdict_offset_px": production.map(|p| round(p.offset_px, 1)),
                "uncorrected": uncorrected.map_or(Value::Null, |f| f.json()),
                "uncorrected_wide_160px": wide.map_or(Value::Null, |f| f.json()),
                "clearest_bed_z": z_search.map_or(Value::Null, |(z, f)| json!({"print_z_mm": z, "found": f.json()})),
                "corrected": corrected,
                "corrected_all_four": corrected_all_four,
            },
        }));
    }

    let controls = controls(&calibration, &spool_dir);
    std::fs::remove_dir_all(&spool_dir).expect("the spool folder is removed");
    let out = json!({
        "calibration": "tests/real-prints/cameras/core-one-2026-09-27.json",
        "radii_px": RADII,
        "hidden_below": HIDDEN_BELOW,
        "alignment_search_px": SEARCH_PX,
        "first_layer": {"index": first_ref.index(), "z_mm": first.z_mm, "ends_at_byte": first_ends, "projected_centre_px": [round(first_centre[0], 0), round(first_centre[1], 0)]},
        "frames": results,
        "controls": controls,
    });
    std::fs::write(
        args.out.join("results.json"),
        serde_json::to_vec_pretty(&out).expect("serialises"),
    )
    .expect("results.json is written");
    print_tables(&out);
}

/// The report's tables, from the results.
#[expect(
    clippy::too_many_lines,
    reason = "a throwaway spike harness, read top to bottom in the order the report states its measurements"
)]
fn print_tables(out: &Value) {
    let f = |v: &Value| v.as_f64().map_or("-".to_owned(), |x| format!("{x:.1}"));
    let s = |v: &Value| v.as_f64().map_or("-".to_owned(), |x| format!("{x:.2}"));
    println!("\n## landmarks at radius 256: dx,dy score; smallest radius all four");
    for frame in out["frames"].as_array().into_iter().flatten() {
        let cells: Vec<String> = frame["landmarks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| {
                format!(
                    "{:+.1},{:+.1} {:.2} (r{})",
                    l["offset_px"][0].as_f64().unwrap_or(f64::NAN),
                    l["offset_px"][1].as_f64().unwrap_or(f64::NAN),
                    l["score"].as_f64().unwrap_or(f64::NAN),
                    l["smallest_radius_found_px"]
                        .as_u64()
                        .map_or("-".to_owned(), |r| r.to_string())
                )
            })
            .collect();
        println!(
            "{} | {} | all four r{}",
            frame["local_time"].as_str().unwrap_or("?"),
            cells.join(" | "),
            frame["smallest_radius_all_four_px"]
                .as_u64()
                .map_or("-".to_owned(), |r| r.to_string())
        );
    }
    println!("\n## fits: landmark rms / leave-one-out rms (px); move at the first layer's centre");
    for frame in out["frames"].as_array().into_iter().flatten() {
        let cells: Vec<String> = Model::ALL
            .iter()
            .map(|m| {
                let e = &frame["fits"][m.name()];
                format!(
                    "{} {}/{} [{},{}]",
                    &m.name()[..3],
                    s(&e["landmark_rms_px"]),
                    s(&e["leave_one_out_rms_px"]),
                    f(&e["moves_first_layer_centre_px"][0]),
                    f(&e["moves_first_layer_centre_px"][1])
                )
            })
            .collect();
        println!(
            "{} {:?} | {}",
            frame["local_time"].as_str().unwrap_or("?"),
            frame["fit_landmarks"]
                .as_array()
                .map(|l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            cells.join(" | ")
        );
    }
    println!(
        "\n## fits on all four best candidates: landmark rms / leave-one-out rms (px), und = underdetermined; per-landmark residuals; move at the first layer's centre"
    );
    for frame in out["frames"].as_array().into_iter().flatten() {
        let marks: Vec<String> = frame["landmarks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| {
                let r = l["reliability"].as_str().unwrap_or("?");
                (if r.starts_with("reliable") { "R" } else { "u" }).to_owned()
            })
            .collect();
        let cells: Vec<String> = Model::ALL
            .iter()
            .map(|m| {
                let e = &frame["fits_all_four"][m.name()];
                let loo = if e["leave_one_out_status"].as_str() == Some("computed") {
                    s(&e["leave_one_out_rms_px"])
                } else {
                    "und".to_owned()
                };
                let each: Vec<String> = e["landmark_each_px"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(f)
                    .collect();
                format!(
                    "{} {}/{} ({}) [{},{}]",
                    &m.name()[..3],
                    s(&e["landmark_rms_px"]),
                    loo,
                    each.join(","),
                    f(&e["moves_first_layer_centre_px"][0]),
                    f(&e["moves_first_layer_centre_px"][1])
                )
            })
            .collect();
        println!(
            "{} {} | {}",
            frame["local_time"].as_str().unwrap_or("?"),
            marks.join(""),
            cells.join(" | ")
        );
    }
    println!(
        "\n## alignment: offset px (shift) c=peak contrast d=effect size m=peak/median s=peak/second"
    );
    let cell = |a: &Value| {
        if a.is_null() {
            return "-".to_owned();
        }
        format!(
            "{} ({:+},{:+}) c{} d{} m{} s{}",
            f(&a["offset_px"]),
            a["shift_px"][0].as_i64().unwrap_or(0),
            a["shift_px"][1].as_i64().unwrap_or(0),
            f(&a["peak_contrast"]),
            s(&a["effect_size"]),
            f(&a["peak_over_median"]),
            s(&a["peak_over_second"])
        )
    };
    for frame in out["frames"].as_array().into_iter().flatten() {
        let a = &frame["alignment"];
        let corrected: Vec<String> = Model::ALL
            .iter()
            .map(|m| format!("{} {}", &m.name()[..3], cell(&a["corrected"][m.name()])))
            .chain(Model::ALL.iter().map(|m| {
                format!(
                    "four-{} {}",
                    &m.name()[..3],
                    cell(&a["corrected_all_four"][m.name()])
                )
            }))
            .collect();
        println!(
            "{} z{} past{} | none {} | wide {} | clearest z{} {} | {}",
            frame["local_time"].as_str().unwrap_or("?"),
            s(&frame["print_z_mm"]),
            frame["past_first_layer"],
            cell(&a["uncorrected"]),
            cell(&a["uncorrected_wide_160px"]),
            f(&a["clearest_bed_z"]["print_z_mm"]),
            cell(&a["clearest_bed_z"]["found"]),
            corrected.join(" | ")
        );
    }
    println!(
        "\n## controls: healthy | empty bed | moved 49 px | moved, search 120 | empty, search 120"
    );
    for c in out["controls"].as_array().into_iter().flatten() {
        println!(
            "{} | {} | {} | {} | {} | {}",
            c["case"].as_str().unwrap_or("?"),
            cell(&c["healthy"]),
            cell(&c["empty_bed"]),
            cell(&c["moved_49px"]),
            cell(&c["moved_49px_wide_120px"]),
            cell(&c["empty_bed_wide_120px"])
        );
    }
}
