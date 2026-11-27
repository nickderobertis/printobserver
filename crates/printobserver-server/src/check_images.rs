//! The image every check is shown, made of the frame the camera gave.
//!
//! The one place that chooses it ([`PanelImages`]). A check of a print is shown
//! `printobserver-panels`' three-panel check image when that image can be
//! trusted: a calibration is configured, the print's G-code index names the
//! object being laid and the layer at the check's file position, the print's
//! first-layer alignment `Passed`, and the frame passes the composer's
//! resolution, stream and drift checks. Every other check, and every look at
//! the printer, is shown the whole frame scaled to [`WHOLE_FRAME_WIDTH`] by
//! [`WHOLE_FRAME_HEIGHT`] and never cropped ([`WholeFrames`]). Either way the
//! image is written under `<state_dir>/check-images/` while the stored frame
//! keeps the camera's size, and one line says which image it is and why.
//!
//! A check shown the three-panel image is also measured against the newest
//! earlier check's frame at least [`MIN_RISE_MM`] below it in the plan
//! ([`measure_growth`]). When the parts this layer lays sit where that frame
//! had them, carried down with the bed, the line says so
//! ([`not_grown_note`]): a stall shows as a few pixels between a drawn line
//! and a part's top, which the eye misreads, and the measurement does not.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::Arc;

use printobserver_core::check_image::PlanPosition;
use printobserver_core::{CheckImageRequest, CheckImages, GcodeIndex, ImageUse, ShownImage};
use printobserver_panels::growth::MIN_RISE_MM;
use printobserver_panels::{
    CameraCalibration, CheckImage, CheckImageInputs, ComposeError, GrowthInputs,
    WHOLE_FRAME_HEIGHT, WHOLE_FRAME_WIDTH, compose_check_image, measure_growth, whole_frame,
};
use printobserver_types::{PrintId, Timestamp};

use printobserver_panels::{InconclusiveReason, ViewCorrection, ViewOutcome};
use printobserver_supervisor_api::{ImageFallback, WholeFrameReasonCode};

use crate::alignment::{
    AlignmentFailure, AlignmentJudging, Alignments, CheckedFrame, Judged, Judging, LastCheckImage,
    PreviousCheck, PrintAlignment, failure_sentence,
};

/// How many scaled frames this process has begun writing, which names each
/// write's partial file apart from any other's.
static WRITES: AtomicU64 = AtomicU64::new(0);

/// The directory under the state directory check images are written in.
pub const CHECK_IMAGES_DIRECTORY: &str = "check-images";

/// Every check shown the whole frame, scaled.
#[derive(Debug, Clone)]
pub struct WholeFrames {
    /// Where the scaled frames are written.
    directory: PathBuf,
}

impl WholeFrames {
    /// Write scaled frames under one state directory.
    #[must_use]
    pub fn under(state_dir: &Path) -> Self {
        Self {
            directory: state_dir.join(CHECK_IMAGES_DIRECTORY),
        }
    }

    /// Where the scaled copy of one stored frame is written: named for the
    /// stored frame's own directory and file, which name it uniquely.
    fn destination(&self, frame: &Path) -> PathBuf {
        let named = |part: Option<&std::ffi::OsStr>| {
            part.map_or_else(String::new, |part| part.to_string_lossy().into_owned())
        };
        let parent = named(frame.parent().and_then(Path::file_name));
        let file = named(frame.file_name());
        self.directory.join(format!(
            "{parent}-{file}-{WHOLE_FRAME_WIDTH}x{WHOLE_FRAME_HEIGHT}.jpg"
        ))
    }
}

impl CheckImages for WholeFrames {
    fn check_image(&self, _print_id: Option<PrintId>, frame: &Path) -> Result<PathBuf, String> {
        let destination = self.destination(frame);
        if destination.is_file() {
            return Ok(destination);
        }
        let bytes = std::fs::read(frame)
            .map_err(|error| format!("{} is unreadable: {error}", frame.display()))?;
        let scaled = whole_frame(&bytes).map_err(|error| error.to_string())?;
        if !scaled.scaled {
            return Ok(frame.to_path_buf());
        }
        write_image(&self.directory, &destination, &scaled.jpeg)?;
        Ok(destination)
    }
}

/// Write one check image — a scaled frame or a three-panel image — under
/// `directory` at `destination`, beside its name first
/// and moved onto it, so a reader never opens a half-written image.
fn write_image(directory: &Path, destination: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{} cannot be made: {error}", directory.display()))?;
    let partial = destination.with_extension(format!(
        "{}-{}.partial",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let written =
        std::fs::write(&partial, bytes).and_then(|()| std::fs::rename(&partial, destination));
    written.map_err(|error| {
        // Whatever was written is no image a reader may open.
        let _ = std::fs::remove_file(&partial);
        format!("{} cannot be written: {error}", destination.display())
    })
}

/// The first words of every line saying a check was shown the three-panel
/// image, which the real-print suite's record reads the image's kind from.
pub const COMPOSITE_NOTE: &str = "the three-panel check image";

/// The first words of every line saying a check was shown the whole frame.
pub const WHOLE_FRAME_NOTE: &str = "the whole camera frame";

/// Why a check is shown the whole frame rather than the three-panel image.
#[derive(Debug, Clone, PartialEq)]
pub enum WholeFrameReason {
    /// No print is named: a look at the printer, before, between or after
    /// prints, a start's bed check among them.
    NoPrint,
    /// No camera calibration is configured.
    NoCalibration,
    /// No camera snapshot address is configured, so the frame's stream is
    /// unknown.
    NoStream,
    /// The job is not the print's file, reported no position, or its file has
    /// no index.
    NoPosition,
    /// The print's G-code names no extruded object, or none laid yet, at the
    /// check's file position.
    NoObject,
    /// It names no layer there.
    NoLayer,
    /// The layer's planned toolpath could not be read from the print's spool.
    ToolpathUnread(String),
    /// The print's first-layer alignment is not decided yet.
    AlignmentPending,
    /// The frame the print's first-layer alignment was last judged on could
    /// not tell where the layer lies; a later check's is judged again.
    AlignmentInconclusive(InconclusiveReason),
    /// The print's G-code lays no object to align.
    AlignmentUnavailable,
    /// The print's first-layer alignment failed.
    AlignmentFailed(AlignmentFailure),
    /// Fewer than two landmarks are found, so whether the camera shifted is
    /// unknown.
    LandmarksNotFound(String),
    /// The camera moved since the print's correction was taken.
    ViewRefitRejected(String),
    /// The frame is not the calibration's size or stream.
    FrameMismatch(String),
    /// The printer reports the print paused, its bed moved off the plan's
    /// height.
    Paused,
    /// The composer refused this frame.
    Refused(ComposeError),
}

impl WholeFrameReason {
    /// The reason as a code.
    #[must_use]
    pub const fn code(&self) -> WholeFrameReasonCode {
        match self {
            Self::NoPrint => WholeFrameReasonCode::NoPrint,
            Self::NoCalibration => WholeFrameReasonCode::NoCalibration,
            Self::NoStream => WholeFrameReasonCode::NoStream,
            Self::NoPosition => WholeFrameReasonCode::NoPosition,
            Self::NoObject => WholeFrameReasonCode::NoObject,
            Self::NoLayer => WholeFrameReasonCode::NoLayer,
            Self::ToolpathUnread(_) => WholeFrameReasonCode::ToolpathUnread,
            Self::AlignmentPending => WholeFrameReasonCode::AlignmentPending,
            Self::AlignmentInconclusive(_) => WholeFrameReasonCode::AlignmentInconclusive,
            Self::AlignmentUnavailable => WholeFrameReasonCode::AlignmentUnavailable,
            Self::AlignmentFailed(_) => WholeFrameReasonCode::AlignmentFailed,
            Self::LandmarksNotFound(_) | Self::Refused(ComposeError::DriftUnverified) => {
                WholeFrameReasonCode::LandmarksNotFound
            }
            Self::ViewRefitRejected(_) | Self::Refused(ComposeError::CameraMoved { .. }) => {
                WholeFrameReasonCode::ViewRefitRejected
            }
            Self::FrameMismatch(_) | Self::Refused(ComposeError::FrameMismatch { .. }) => {
                WholeFrameReasonCode::FrameMismatch
            }
            Self::Paused => WholeFrameReasonCode::Paused,
            Self::Refused(_) => WholeFrameReasonCode::Refused,
        }
    }
}

impl core::fmt::Display for WholeFrameReason {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoPrint => formatter
                .write_str("no print is named, so there is no planned toolpath to draw on it"),
            Self::NoCalibration => formatter.write_str("no camera calibration is configured"),
            Self::NoStream => formatter.write_str(
                "no camera snapshot address is configured, so the frame's stream is unknown",
            ),
            Self::NoPosition => formatter.write_str(
                "the print's G-code index gives no file position for this check",
            ),
            Self::NoObject => formatter.write_str(
                "the print's G-code names no extruded object laid at this point",
            ),
            Self::NoLayer => formatter.write_str("the print's G-code names no layer at this point"),
            Self::ToolpathUnread(detail) => write!(
                formatter,
                "the layer's planned toolpath could not be read from the print's spool: {detail}"
            ),
            Self::AlignmentPending => formatter.write_str(
                "the print's first-layer alignment is pending: its first object layer is not down, or no frame of it has been judged yet",
            ),
            Self::AlignmentInconclusive(why) => write!(
                formatter,
                "the print's first-layer alignment is inconclusive on the frame judged last: {}, so a later check's frame is judged again; this is no evidence of a defect",
                match why {
                    InconclusiveReason::Covered => "the toolhead covers the part",
                    InconclusiveReason::NotClearlyFound => "the printed first layer was not clearly found",
                }
            ),
            Self::AlignmentUnavailable => {
                formatter.write_str("the print's G-code lays no object to align")
            }
            Self::AlignmentFailed(failure) => write!(
                formatter,
                "the print's first-layer alignment failed: {}",
                failure_sentence(failure)
            ),
            Self::LandmarksNotFound(reason) | Self::ViewRefitRejected(reason) | Self::FrameMismatch(reason) => {
                formatter.write_str(reason)
            }
            Self::Paused => formatter.write_str(
                "the printer reports the print paused, so its bed may stand off the plan's height and nothing is drawn",
            ),
            Self::Refused(error) => error.fmt(formatter),
        }
    }
}

/// The line saying a check was shown the whole frame, and why.
#[must_use]
pub fn whole_frame_note(reason: &WholeFrameReason) -> String {
    format!(
        "{WHOLE_FRAME_NOTE}, scaled to fit {WHOLE_FRAME_WIDTH}x{WHOLE_FRAME_HEIGHT} and uncropped, because {reason}"
    )
}

/// What a check's note says of the drawing before it says what a part off it
/// means: an agent shown a part off its walls otherwise explains the
/// difference as the camera or the bed having shifted in view.
const CAMERA_HOLDS: &str = "the camera does not shift in regular operation and the drawing already accounts for the bed's travel, so ";

/// What a check's note says of the drawing instead when it went through the
/// print's correction while the landmarks had moved: the camera has shifted,
/// and the drawing is corrected for it.
const CAMERA_CORRECTED: &str = "the camera has shifted slightly since it was calibrated, and the drawing is corrected for that shift from this print's own first layer and already accounts for the bed's travel, so ";

/// The line saying a check or a look was shown the three-panel image of the
/// frame captured at `current_at`, beside the frame `previous_at` names: panel
/// 3 empty when no earlier check's frame is at hand — the print's first check,
/// or one whose earlier frame is gone from the store.
///
/// A check's names which frame each panel is and when each was captured, and
/// says what the skill says: the camera does not shift — or, `corrected`, that
/// it shifted and the drawing went through the print's correction for it — and
/// the drawing follows the bed, so a part off it has moved unless there is strong evidence
/// otherwise, and no later frame undoes it. A turn handed a check's image decides on it, with no look to
/// stall on, so this is what it decides from. A look's says only what each
/// panel is.
#[must_use]
pub fn composite_note(
    made_for: ImageUse,
    current_at: Timestamp,
    previous_at: Option<Timestamp>,
    corrected: bool,
) -> String {
    let holds = if corrected {
        CAMERA_CORRECTED
    } else {
        CAMERA_HOLDS
    };
    let (current, previous, moved) = match (made_for, previous_at) {
        (ImageUse::Look, at) => (
            "this layer's planned toolpath drawn on the frame".to_owned(),
            if at.is_none() {
                "panel 3 empty, no earlier check's frame being at hand".to_owned()
            } else {
                "panel 3 the previous check's frame".to_owned()
            },
            String::new(),
        ),
        (ImageUse::Check, None) => (
            format!(
                "this check's own frame, captured {current_at}, with this layer's planned toolpath drawn on it"
            ),
            "panel 3 empty, no earlier check's frame being at hand".to_owned(),
            format!(
                "; {holds}a part off its drawn walls in panel 1 has moved unless there is strong evidence otherwise, and no later frame undoes that"
            ),
        ),
        (ImageUse::Check, Some(at)) => (
            format!(
                "this check's own frame, captured {current_at}, with this layer's planned toolpath drawn on it"
            ),
            format!("panel 3 the previous check's frame, captured {at}"),
            format!(
                "; {holds}a part off its drawn walls in panel 1, or moved since panel 3, has moved unless there is strong evidence otherwise, and no later frame undoes that"
            ),
        ),
    };
    format!(
        "{COMPOSITE_NOTE}, {}x{}: panel 1 {current}, panel 2 a close-up of the part being printed, {previous}{moved}",
        printobserver_panels::compose::PANEL_WIDTH,
        printobserver_panels::compose::IMAGE_HEIGHT
    )
}

/// What a check's line adds when the measurement finds the parts this layer
/// lays have not grown since the frame captured at `since`, `rise_mm` lower in
/// the plan: the skill's own words for it, `not risen as planned`.
#[must_use]
pub fn not_grown_note(since: Timestamp, rise_mm: f64) -> String {
    format!(
        "; measured against the check frame captured {since}, {rise_mm:.1} mm lower in the plan: the parts this layer lays have not risen as planned, sitting where that frame had them, carried down with the bed, so nothing has been laid on them since"
    )
}

/// The print's height in its plan at a check's position.
fn plan_z(plan: &PlanPosition) -> Option<f64> {
    plan.index
        .layer_at(plan.file_position_bytes)
        .and_then(|layer| plan.index.layer_z_mm(layer))
}

/// What a check image is composed from, beyond the frames: the print's plan at
/// one file position.
#[derive(Debug, Clone, Copy)]
pub struct PlanAt<'a> {
    /// The print's G-code index, its toolpaths read from its spool.
    pub index: &'a GcodeIndex,
    /// The check's file position.
    pub file_position_bytes: u64,
}

/// Compose the three-panel check image of one frame at one file position, as
/// every check of a print whose alignment passed is shown it, through the
/// print's correction when one is given: the active
/// object at that position, the layer there — and, when that layer lays
/// supports alone, the nearest object layer below it — over the frame, with
/// `previous` as panel 3.
///
/// # Errors
///
/// [`WholeFrameReason`] naming what the index lacks at that position, or the
/// composer's own refusal.
pub fn compose_at(
    plan: PlanAt<'_>,
    current: (&[u8], Timestamp),
    previous: Option<(&[u8], Timestamp)>,
    camera_snapshot_url: &str,
    calibration: &CameraCalibration,
    correction: Option<&ViewCorrection>,
) -> Result<CheckImage, WholeFrameReason> {
    let index = plan.index;
    let position = plan.file_position_bytes;
    let active_object = index
        .object_at(position)
        .filter(|object| *object < index.object_outlines().len())
        .ok_or(WholeFrameReason::NoObject)?;
    let layer = index.layer_at(position).ok_or(WholeFrameReason::NoLayer)?;
    let print_z_mm = index.layer_z_mm(layer).ok_or(WholeFrameReason::NoLayer)?;
    let unread =
        |error: printobserver_core::SpoolError| WholeFrameReason::ToolpathUnread(error.to_string());
    let toolpath = index.layer_toolpath(layer).map_err(unread)?;
    let below = if toolpath.carries_object_extrusion() {
        None
    } else {
        index
            .object_layer_at_or_below(layer)
            .map(|below| index.layer_toolpath(below))
            .transpose()
            .map_err(unread)?
    };
    compose_check_image(&CheckImageInputs {
        current_jpeg: current.0,
        current_at: current.1,
        camera_snapshot_url,
        previous,
        outlines: index.object_outlines(),
        active_object,
        toolpath: &toolpath,
        object_layer_below: below.as_ref(),
        print_z_mm,
        calibration,
        correction,
    })
    .map_err(WholeFrameReason::Refused)
}

/// One judged frame whose image is chosen.
#[derive(Debug, Clone, Copy)]
pub struct Shown<'a> {
    /// Where the stored frame is.
    pub frame: &'a Path,
    /// When it was captured.
    pub captured_at: Timestamp,
    /// What the image is for.
    pub made_for: ImageUse,
    /// Where the print stood in its plan.
    pub plan: Option<&'a PlanPosition>,
    /// Whether the printer reported the print paused.
    pub paused: bool,
}

/// Every check's image, the three-panel one where it can be trusted and the
/// whole frame otherwise, each with the line saying which and why.
pub struct PanelImages {
    /// The whole frames, and where every image is written.
    whole: WholeFrames,
    /// Each print's alignment verdict and last check, and the calibration.
    alignments: Arc<Alignments>,
    /// The configured camera's snapshot address: the stream every frame of a
    /// print is of, an alert's included, which the camera's own feed relays.
    camera_snapshot_url: Option<String>,
}

impl core::fmt::Debug for PanelImages {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PanelImages")
            .field("whole", &self.whole)
            .field("camera_snapshot_url", &self.camera_snapshot_url)
            .finish_non_exhaustive()
    }
}

impl PanelImages {
    /// The images of one state directory's frames.
    #[must_use]
    pub fn new(
        state_dir: &Path,
        alignments: Arc<Alignments>,
        camera_snapshot_url: Option<String>,
    ) -> Self {
        Self {
            whole: WholeFrames::under(state_dir),
            alignments,
            camera_snapshot_url,
        }
    }

    /// One print's first-layer alignment as it stands.
    #[must_use]
    pub fn alignment(&self, print_id: PrintId) -> PrintAlignment {
        self.alignments.print_alignment(print_id)
    }

    /// The image one print's last check was shown; nothing before its first.
    #[must_use]
    pub fn last_check_image(&self, print_id: PrintId) -> Option<LastCheckImage> {
        self.alignments.last_check_image(print_id)
    }

    /// The whole frame, with the line saying why.
    fn whole_frame(&self, frame: &Path, reason: &WholeFrameReason) -> Result<ShownImage, String> {
        Ok(ShownImage {
            fallback: Some(ImageFallback {
                code: reason.code(),
                message: reason.to_string(),
            }),
            ..ShownImage::plain(
                self.whole.check_image(None, frame)?,
                Some(whole_frame_note(reason)),
            )
        })
    }

    /// Judge one frame of a print — its view, and its first-layer alignment
    /// as `alignment` asks — keeping what that decides. Nothing when no
    /// calibration or no camera stream is configured, which leaves nothing to
    /// judge a frame against.
    #[must_use]
    pub fn judge_frame(
        &self,
        print_id: PrintId,
        frame: &[u8],
        frame_sha256: &str,
        plan: Option<&PlanPosition>,
        alignment: AlignmentJudging,
    ) -> Option<Judged> {
        self.alignments.calibration()?;
        let url = self.camera_snapshot_url.as_deref()?;
        Some(self.alignments.judge(
            print_id,
            &Judging {
                frame_jpeg: frame,
                frame_sha256,
                camera_snapshot_url: url,
                plan,
                alignment,
            },
        ))
    }

    /// The image one frame of a print is shown once it has been judged: the
    /// three-panel image where the judgement allows it, and the whole frame
    /// with the reason otherwise.
    ///
    /// # Errors
    ///
    /// Why no image could be made at all: the frame unreadable, or the image
    /// unwritable.
    pub fn image_of(
        &self,
        print_id: PrintId,
        shown: &Shown<'_>,
        judged: Option<&Judged>,
    ) -> Result<ShownImage, String> {
        let previous = self.alignments.previous_check(print_id);
        match self.chosen(print_id, shown, judged, previous.as_ref())? {
            Ok(composite) => Ok(composite),
            Err(reason) => self.whole_frame(shown.frame, &reason),
        }
    }

    /// The three-panel image of one judged frame of a print, or why it is
    /// shown the whole frame instead.
    fn chosen(
        &self,
        print_id: PrintId,
        shown: &Shown<'_>,
        judged: Option<&Judged>,
        previous: Option<&PreviousCheck>,
    ) -> Result<Result<ShownImage, WholeFrameReason>, String> {
        let Some(calibration) = self.alignments.calibration() else {
            return Ok(Err(WholeFrameReason::NoCalibration));
        };
        let Some(url) = self.camera_snapshot_url.as_deref() else {
            return Ok(Err(WholeFrameReason::NoStream));
        };
        if shown.paused {
            return Ok(Err(WholeFrameReason::Paused));
        }
        let Some(judged) = judged else {
            return Ok(Err(WholeFrameReason::AlignmentPending));
        };
        if let Some(detail) = &judged.unreadable {
            return Err(detail.clone());
        }
        let Some(plan) = shown.plan else {
            return Ok(Err(WholeFrameReason::NoPosition));
        };
        let at = PlanAt {
            index: &plan.index,
            file_position_bytes: plan.file_position_bytes,
        };
        if at.index.object_at(at.file_position_bytes).is_none()
            || at.index.object_outlines().is_empty()
        {
            return Ok(Err(WholeFrameReason::NoObject));
        }
        if at.index.layer_at(at.file_position_bytes).is_none() {
            return Ok(Err(WholeFrameReason::NoLayer));
        }
        if let Some(reason) = &judged.rejected {
            return Ok(Err(WholeFrameReason::ViewRefitRejected(reason.clone())));
        }
        match &judged.alignment {
            PrintAlignment::Passed { .. } => {}
            PrintAlignment::Pending => {
                return Ok(Err(judged.inconclusive.map_or(
                    WholeFrameReason::AlignmentPending,
                    WholeFrameReason::AlignmentInconclusive,
                )));
            }
            PrintAlignment::Unavailable => {
                return Ok(Err(WholeFrameReason::AlignmentUnavailable));
            }
            PrintAlignment::Failed { reason } => {
                return Ok(Err(WholeFrameReason::AlignmentFailed(reason.clone())));
            }
        }
        if let Some(reason) = &judged.mismatch {
            return Ok(Err(WholeFrameReason::FrameMismatch(reason.clone())));
        }
        let refit = match &judged.view {
            Some((ViewOutcome::LandmarksNotFound, reason)) => {
                return Ok(Err(WholeFrameReason::LandmarksNotFound(
                    reason.clone().unwrap_or_default(),
                )));
            }
            Some((ViewOutcome::ViewRefitRejected, reason)) => {
                return Ok(Err(WholeFrameReason::ViewRefitRejected(
                    reason.clone().unwrap_or_default(),
                )));
            }
            Some((ViewOutcome::FrameMismatch, reason)) => {
                return Ok(Err(WholeFrameReason::FrameMismatch(
                    reason.clone().unwrap_or_default(),
                )));
            }
            Some((ViewOutcome::Refit, _)) => true,
            Some((ViewOutcome::Unshifted, _)) | None => false,
        };
        let current = std::fs::read(shown.frame)
            .map_err(|error| format!("{} is unreadable: {error}", shown.frame.display()))?;
        // A previous frame gone from the store, or one that no longer decodes,
        // is one the panel cannot show, which leaves panel 3 empty rather than
        // the check without an image.
        let earlier = previous.and_then(|check| {
            std::fs::read(&check.frame)
                .ok()
                .filter(|bytes| whole_frame(bytes).is_ok())
                .map(|bytes| (bytes, check.captured_at))
        });
        let composed = match compose_at(
            at,
            (&current, shown.captured_at),
            earlier.as_ref().map(|(bytes, at)| (bytes.as_slice(), *at)),
            url,
            calibration,
            judged.correction.as_ref(),
        ) {
            Ok(composed) => composed,
            // The current frame was judged whole by the vision port before it
            // was stored, and the previous one was decoded above, so neither
            // refusal is reached by a frame the camera gave; one that is is a
            // check shown the stored frame, as any check image that cannot be
            // made is, which `a check image that cannot be written` proves.
            Err(WholeFrameReason::Refused(
                ComposeError::Undecodable { detail } | ComposeError::Encode { detail },
            )) => return Err(detail),
            Err(reason) => return Ok(Err(reason)),
        };
        let destination = self.composite_destination(shown.frame, previous);
        write_image(&self.whole.directory, &destination, &composed.jpeg)?;
        let mut note = composite_note(
            shown.made_for,
            shown.captured_at,
            earlier.as_ref().map(|(_, at)| *at),
            refit,
        );
        if shown.made_for == ImageUse::Check
            && let Some(clause) = self.growth_clause(print_id, plan, &current, url, calibration)
        {
            note.push_str(&clause);
        }
        Ok(Ok(ShownImage {
            composite: true,
            ..ShownImage::plain(destination, Some(note))
        }))
    }

    /// What a check's line adds of whether the parts grew: the
    /// [`not_grown_note`] when the measurement against the newest earlier
    /// check's frame at least [`MIN_RISE_MM`] below finds they did not, and
    /// nothing otherwise — no such frame, one gone from the store, a layer
    /// laying supports alone, or a measurement that found growth or could not
    /// tell. Each measurement made is said on the log.
    fn growth_clause(
        &self,
        print_id: PrintId,
        plan: &PlanPosition,
        current: &[u8],
        url: &str,
        calibration: &CameraCalibration,
    ) -> Option<String> {
        let layer = plan.index.layer_at(plan.file_position_bytes)?;
        let print_z_mm = plan.index.layer_z_mm(layer)?;
        let reference = self
            .alignments
            .growth_reference(print_id, print_z_mm, MIN_RISE_MM)?;
        let earlier = std::fs::read(&reference.frame).ok()?;
        let toolpath = plan.index.layer_toolpath(layer).ok()?;
        let measured = measure_growth(&GrowthInputs {
            earlier_jpeg: &earlier,
            earlier_print_z_mm: reference.print_z_mm,
            current_jpeg: current,
            current_print_z_mm: print_z_mm,
            camera_snapshot_url: url,
            toolpath: &toolpath,
            calibration,
        })
        .ok()??;
        let not_grown = measured.not_grown();
        eprintln!(
            "printobserver: print {print_id}: growth from Z {:.1} ({}) to Z {print_z_mm:.1}: carried {:.2}, held {:.2}, {:.0}% carried over {} samples {:.1} px apart: {}",
            reference.print_z_mm,
            reference.captured_at,
            measured.carried,
            measured.held,
            measured.carried_share * 100.0,
            measured.samples,
            measured.separation_px,
            if not_grown { "not grown" } else { "no verdict" }
        );
        not_grown.then(|| not_grown_note(reference.captured_at, print_z_mm - reference.print_z_mm))
    }

    /// Where the three-panel image of one stored frame, after one previous
    /// check, is written: named for both, which name it uniquely.
    fn composite_destination(&self, frame: &Path, previous: Option<&PreviousCheck>) -> PathBuf {
        let named = |frame: &Path| {
            let part = |part: Option<&std::ffi::OsStr>| {
                part.map_or_else(String::new, |part| part.to_string_lossy().into_owned())
            };
            format!(
                "{}-{}",
                part(frame.parent().and_then(Path::file_name)),
                part(frame.file_name())
            )
        };
        let after = previous.map_or_else(|| "first".to_owned(), |check| named(&check.frame));
        self.whole
            .directory
            .join(format!("{}-composite-after-{after}.jpg", named(frame)))
    }
}

impl CheckImages for PanelImages {
    fn check_image(&self, print_id: Option<PrintId>, frame: &Path) -> Result<PathBuf, String> {
        self.whole.check_image(print_id, frame)
    }

    fn restart_growth(&self, print_id: PrintId) {
        self.alignments.restart_growth(print_id);
    }

    fn composes(&self, print_id: PrintId) -> bool {
        self.alignments.calibration().is_some()
            && self.camera_snapshot_url.is_some()
            && matches!(
                self.alignments.print_alignment(print_id),
                PrintAlignment::Passed { .. }
            )
    }

    fn shown_image(&self, request: &CheckImageRequest<'_>) -> Result<ShownImage, String> {
        let Some(print_id) = request.print_id else {
            return self.whole_frame(request.frame, &WholeFrameReason::NoPrint);
        };
        // A check taken while the printer reports the print paused is not
        // judged: its bed is lowered off the plan's height.
        let judged = if request.paused {
            None
        } else {
            let alignment = if request.made_for == ImageUse::Check && request.own_camera {
                AlignmentJudging::WhilePending
            } else {
                AlignmentJudging::Never
            };
            match std::fs::read(request.frame) {
                Ok(current) => self.judge_frame(
                    print_id,
                    &current,
                    request.frame_sha256,
                    request.plan.as_ref(),
                    alignment,
                ),
                Err(error) => {
                    return Err(format!(
                        "{} is unreadable: {error}",
                        request.frame.display()
                    ));
                }
            }
        };
        let shown = Shown {
            frame: request.frame,
            captured_at: request.captured_at,
            made_for: request.made_for,
            plan: request.plan.as_ref(),
            paused: request.paused,
        };
        let mut image = self.image_of(print_id, &shown, judged.as_ref())?;
        image.records = judged.map(|judged| judged.records).unwrap_or_default();
        if request.made_for == ImageUse::Check {
            self.alignments.record_check(
                print_id,
                &PreviousCheck {
                    frame: request.frame.to_path_buf(),
                    captured_at: request.captured_at,
                },
            );
            if let Some(print_z_mm) = request.plan.as_ref().and_then(plan_z) {
                self.alignments.record_checked_frame(
                    print_id,
                    CheckedFrame {
                        frame: request.frame.to_path_buf(),
                        captured_at: request.captured_at,
                        print_z_mm,
                    },
                    MIN_RISE_MM,
                );
            }
            image.records.extend(self.alignments.note_check_image(
                print_id,
                LastCheckImage {
                    at: request.captured_at,
                    kind: image.kind(),
                    fallback: image.fallback.clone(),
                },
                request.frame_sha256,
            ));
        }
        Ok(image)
    }
}
