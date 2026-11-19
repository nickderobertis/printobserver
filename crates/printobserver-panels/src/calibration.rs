//! One printer's camera calibration: the file `tools/camera-calibration`
//! writes, validated as it is read, and the projection it defines.
//!
//! The lens is `OpenCV`'s fisheye (equidistant) model; the bed frame is the
//! G-code's own (X right, Y back, Z up, millimetres), and the pose maps it to
//! the camera in `OpenCV`'s convention. On a bed-z printer the bed drops as the
//! print rises, so with the print at height `z` a point `(x, y, h)` of the part
//! sits at `(x, y, h - z)`: the layer being printed always lies at the
//! nozzle's plane.

use base64::Engine as _;
use image::GrayImage;
use nalgebra::{Rotation3, Vector3};
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// The calibration file's one schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// The furthest a landmark is searched for, in pixels: the drift check scores
/// every offset within it, so a radius past this is no landmark search.
pub const MAX_SEARCH_RADIUS_PX: u32 = 256;

/// How the printer moves its bed: the one motion a calibration describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum Motion {
    /// The bed moves only in Z, dropping as the print rises.
    #[serde(rename = "bed-z")]
    BedZ,
}

/// The lens: focal lengths and principal point in pixels, and the fisheye
/// distortion terms `k1`–`k4`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct Intrinsics {
    /// Focal length along X, in pixels.
    pub fx: f64,
    /// Focal length along Y, in pixels.
    pub fy: f64,
    /// Principal point, X.
    pub cx: f64,
    /// Principal point, Y.
    pub cy: f64,
    /// The equidistant model's `k1`–`k4`; the tool fits `k1` and writes the
    /// rest as 0.
    pub k: [f64; 4],
}

/// Where the bed is, seen from the camera: the transform from the bed frame
/// to the camera's, `X_cam = R · X_bed + t`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct BedPose {
    /// `R` as a Rodrigues vector, in radians.
    pub rotation_rodrigues: [f64; 3],
    /// `t`, in millimetres.
    pub translation_mm: [f64; 3],
}

/// A fixed chassis feature the drift check locates in every frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct Landmark {
    /// What it is.
    pub name: String,
    /// Where its patch's centre was in the calibrated frames, in pixels.
    pub center_px: [f64; 2],
    /// The grey patch around it, as a base64 PNG.
    pub patch_png_base64: String,
    /// How far from `center_px` it is searched for, in pixels.
    pub search_radius_px: u32,
}

/// How well the fit went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct Fit {
    /// The RMS distance of the marks from their projected outlines, in
    /// pixels.
    pub rms_reprojection_px: f64,
    /// How many frames the marks were on.
    pub frames: u32,
    /// The date the frames were taken, `YYYY-MM-DD`.
    // llmlint: ignore[invalid_states_unrepresentable] See suppressions.toml.
    pub calibrated_at: String,
}

/// One printer's camera calibration, as `tools/camera-calibration` writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct CameraCalibration {
    /// Always [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// How the printer moves its bed.
    pub motion: Motion,
    /// The calibrated frames' size, `[width, height]`; a frame of any other
    /// size is refused, never scaled.
    pub image_size_px: [u32; 2],
    /// The lens.
    pub intrinsics: Intrinsics,
    /// The bed's pose.
    pub bed_pose: BedPose,
    /// The drift check's landmarks.
    pub landmarks: Vec<Landmark>,
    /// How far, in pixels, two visible landmarks must both have moved for the
    /// camera to count as moved.
    pub drift_tolerance_px: f64,
    /// The `[camera] snapshot_url` the calibrated frames came from; a frame
    /// from any other is refused.
    // llmlint: ignore[invalid_states_unrepresentable] See suppressions.toml.
    pub camera_snapshot_url: String,
    /// The first-layer alignment check's pass threshold, in pixels.
    pub alignment_max_offset_px: f64,
    /// How well the fit went.
    pub fit: Fit,
    /// Each landmark's patch, decoded once as the file is read.
    #[serde(skip)]
    #[schemars(skip)]
    patches: Vec<GrayImage>,
}

/// Why a calibration file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationError {
    /// It is not JSON of the calibration's shape.
    Shape {
        /// What the parser said.
        detail: String,
    },
    /// A value is out of what a calibration can hold.
    Invalid {
        /// The key, as the file spells it.
        field: &'static str,
        /// Why.
        reason: String,
    },
}

impl core::fmt::Display for CalibrationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Shape { detail } => write!(formatter, "not a camera calibration: {detail}"),
            Self::Invalid { field, reason } => write!(formatter, "`{field}` {reason}"),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// Refuse one field.
fn invalid(field: &'static str, reason: impl Into<String>) -> CalibrationError {
    CalibrationError::Invalid {
        field,
        reason: reason.into(),
    }
}

/// Whether text is a calendar date written `YYYY-MM-DD`.
fn is_date(text: &str) -> bool {
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok() && text.len() == 10
}

/// Refuse a number that is not finite, or not above zero when it must be.
fn finite(field: &'static str, values: &[f64], positive: bool) -> Result<(), CalibrationError> {
    for value in values {
        if !value.is_finite() {
            return Err(invalid(field, "is not a finite number"));
        }
        if positive && *value <= 0.0 {
            return Err(invalid(field, "must be above zero"));
        }
    }
    Ok(())
}

impl CameraCalibration {
    /// Read and validate a calibration file.
    ///
    /// # Errors
    ///
    /// [`CalibrationError::Shape`] for anything that is not the file's shape,
    /// and [`CalibrationError::Invalid`] naming the key that holds a value no
    /// calibration can: another schema version, an empty or non-finite size,
    /// focal length or tolerance, a snapshot URL that is no address, or a
    /// landmark whose patch is not a PNG or whose search radius is zero.
    pub fn from_json(bytes: &[u8]) -> Result<Self, CalibrationError> {
        let mut calibration: Self =
            printobserver_types::serde_json::from_slice(bytes).map_err(|error| {
                CalibrationError::Shape {
                    detail: error.to_string(),
                }
            })?;
        if calibration.schema_version != SCHEMA_VERSION {
            return Err(invalid(
                "schema_version",
                format!(
                    "is {}, and this program reads version {SCHEMA_VERSION}",
                    calibration.schema_version
                ),
            ));
        }
        if calibration.image_size_px.contains(&0) {
            return Err(invalid("image_size_px", "names an empty frame"));
        }
        let lens = calibration.intrinsics;
        finite("intrinsics.fx", &[lens.fx, lens.fy], true)?;
        finite("intrinsics.cx", &[lens.cx, lens.cy], false)?;
        finite("intrinsics.k", &lens.k, false)?;
        finite(
            "bed_pose.rotation_rodrigues",
            &calibration.bed_pose.rotation_rodrigues,
            false,
        )?;
        finite(
            "bed_pose.translation_mm",
            &calibration.bed_pose.translation_mm,
            false,
        )?;
        finite(
            "drift_tolerance_px",
            &[calibration.drift_tolerance_px],
            true,
        )?;
        finite(
            "alignment_max_offset_px",
            &[calibration.alignment_max_offset_px],
            true,
        )?;
        printobserver_vision_api::WebAddress::new(&calibration.camera_snapshot_url)
            .map_err(|why| invalid("camera_snapshot_url", why))?;
        if !is_date(&calibration.fit.calibrated_at) {
            return Err(invalid("fit.calibrated_at", "is not a YYYY-MM-DD date"));
        }
        let mut patches = Vec::with_capacity(calibration.landmarks.len());
        for landmark in &calibration.landmarks {
            finite("landmarks.center_px", &landmark.center_px, false)?;
            let [w, h] = calibration.image_size_px.map(f64::from);
            if !((0.0..w).contains(&landmark.center_px[0])
                && (0.0..h).contains(&landmark.center_px[1]))
            {
                return Err(invalid("landmarks.center_px", "lies outside the frame"));
            }
            if !(1..=MAX_SEARCH_RADIUS_PX).contains(&landmark.search_radius_px) {
                return Err(invalid(
                    "landmarks.search_radius_px",
                    format!("must be from 1 to {MAX_SEARCH_RADIUS_PX}"),
                ));
            }
            let png = base64::engine::general_purpose::STANDARD
                .decode(&landmark.patch_png_base64)
                .map_err(|error| invalid("landmarks.patch_png_base64", error.to_string()))?;
            let patch = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                .map_err(|error| invalid("landmarks.patch_png_base64", error.to_string()))?
                .to_luma8();
            if patch.width() < 2 || patch.height() < 2 {
                return Err(invalid("landmarks.patch_png_base64", "is no patch"));
            }
            patches.push(patch);
        }
        calibration.patches = patches;
        Ok(calibration)
    }

    /// Each landmark's decoded patch, in `landmarks`' order.
    pub(crate) fn patches(&self) -> &[GrayImage] {
        &self.patches
    }

    /// Where bed points appear in a frame, in the calibration's pixels: each
    /// `[x, y, h]` of the part, with the print at `print_z_mm`. `None` for a
    /// point behind the camera or outside a frame of `frame_size_px`.
    #[must_use]
    pub fn project(
        &self,
        points_mm: &[[f64; 3]],
        print_z_mm: f64,
        frame_size_px: [u32; 2],
    ) -> Vec<Option<[f64; 2]>> {
        let [w, h] = frame_size_px.map(f64::from);
        let projector = self.projector();
        points_mm
            .iter()
            .map(|point| {
                projector
                    .pixel([point[0], point[1], point[2] - print_z_mm])
                    .filter(|[u, v]| (0.0..w).contains(u) && (0.0..h).contains(v))
            })
            .collect()
    }

    /// The projection, ready to map many points.
    pub(crate) fn projector(&self) -> Projector {
        let [rx, ry, rz] = self.bed_pose.rotation_rodrigues;
        let [tx, ty, tz] = self.bed_pose.translation_mm;
        Projector {
            rotation: Rotation3::new(Vector3::new(rx, ry, rz)),
            translation: Vector3::new(tx, ty, tz),
            lens: self.intrinsics,
        }
    }

    /// A copy whose bed pose is another: what a test moves a projection with.
    #[must_use]
    pub fn with_bed_pose(&self, bed_pose: BedPose) -> Self {
        Self {
            bed_pose,
            ..self.clone()
        }
    }
}

/// The fisheye projection of one calibration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Projector {
    /// `R`.
    rotation: Rotation3<f64>,
    /// `t`.
    translation: Vector3<f64>,
    /// The lens.
    lens: Intrinsics,
}

impl Projector {
    /// Where a world point (bed frame, already lowered by the print height)
    /// lands, unbounded by any frame; `None` behind the camera.
    pub(crate) fn pixel(&self, world: [f64; 3]) -> Option<[f64; 2]> {
        let camera = self.rotation * Vector3::new(world[0], world[1], world[2]) + self.translation;
        if camera.z <= 1e-6 {
            return None;
        }
        let (a, b) = (camera.x / camera.z, camera.y / camera.z);
        let r = a.hypot(b);
        let theta = r.atan();
        let t2 = theta * theta;
        let [k1, k2, k3, k4] = self.lens.k;
        let theta_d = theta * (1.0 + t2 * (k1 + t2 * (k2 + t2 * (k3 + t2 * k4))));
        let scale = if r > 1e-12 { theta_d / r } else { 1.0 };
        Some([
            self.lens.fx * a * scale + self.lens.cx,
            self.lens.fy * b * scale + self.lens.cy,
        ])
    }
}
