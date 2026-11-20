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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::Arc;

use printobserver_core::{CheckImageRequest, CheckImages, GcodeIndex, ImageUse, ShownImage};
use printobserver_panels::{
    CameraCalibration, CheckImage, CheckImageInputs, ComposeError, WHOLE_FRAME_HEIGHT,
    WHOLE_FRAME_WIDTH, compose_check_image, whole_frame,
};
use printobserver_types::{PrintId, Timestamp};

use crate::alignment::{AlignmentFailure, Alignments, PreviousCheck, PrintAlignment};

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
    /// The print's G-code lays no object to align.
    AlignmentUnavailable,
    /// The print's first-layer alignment failed.
    AlignmentFailed(AlignmentFailure),
    /// The composer refused this frame.
    Refused(ComposeError),
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
            Self::AlignmentUnavailable => {
                formatter.write_str("the print's G-code lays no object to align")
            }
            Self::AlignmentFailed(AlignmentFailure::Offset { offset_px }) => write!(
                formatter,
                "the print's first-layer alignment failed: the printed first layer lies {offset_px:.1} px from where the calibration projects it"
            ),
            Self::AlignmentFailed(AlignmentFailure::Resolution { expected, got }) => write!(
                formatter,
                "the print's first-layer alignment failed: the camera's frames are {}x{} and the calibration's {}x{}",
                got[0], got[1], expected[0], expected[1]
            ),
            Self::AlignmentFailed(AlignmentFailure::Stream) => formatter.write_str(
                "the print's first-layer alignment failed: the camera is not the stream the calibration was fitted on",
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

/// The line saying a check was shown the three-panel image: panel 3 empty
/// when no earlier check's frame is at hand — the print's first check, or one
/// whose earlier frame is gone from the store.
#[must_use]
pub fn composite_note(no_previous: bool) -> String {
    let previous = if no_previous {
        "panel 3 empty, no earlier check's frame being at hand"
    } else {
        "panel 3 the previous check's frame"
    };
    format!(
        "{COMPOSITE_NOTE}, {}x{}: panel 1 this layer's planned toolpath drawn on the frame, panel 2 a close-up of the part being printed, {previous}",
        printobserver_panels::compose::PANEL_WIDTH,
        printobserver_panels::compose::IMAGE_HEIGHT
    )
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
/// every check of a print whose alignment passed is shown it: the active
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
    })
    .map_err(WholeFrameReason::Refused)
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

    /// The whole frame, with the line saying why.
    fn whole_frame(&self, frame: &Path, reason: &WholeFrameReason) -> Result<ShownImage, String> {
        Ok(ShownImage {
            path: self.whole.check_image(None, frame)?,
            note: Some(whole_frame_note(reason)),
        })
    }

    /// The three-panel image of one request of a print, or why it is shown
    /// the whole frame instead.
    fn composite(
        &self,
        print_id: PrintId,
        request: &CheckImageRequest<'_>,
        previous: Option<&PreviousCheck>,
    ) -> Result<Result<ShownImage, WholeFrameReason>, String> {
        let Some(calibration) = self.alignments.calibration() else {
            return Ok(Err(WholeFrameReason::NoCalibration));
        };
        let Some(url) = self.camera_snapshot_url.as_deref() else {
            return Ok(Err(WholeFrameReason::NoStream));
        };
        let Some(plan) = &request.plan else {
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
        match self.alignments.print_alignment(print_id) {
            PrintAlignment::Passed { .. } => {}
            PrintAlignment::Pending => return Ok(Err(WholeFrameReason::AlignmentPending)),
            PrintAlignment::Unavailable => {
                return Ok(Err(WholeFrameReason::AlignmentUnavailable));
            }
            PrintAlignment::Failed { reason } => {
                return Ok(Err(WholeFrameReason::AlignmentFailed(reason)));
            }
        }
        let current = std::fs::read(request.frame)
            .map_err(|error| format!("{} is unreadable: {error}", request.frame.display()))?;
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
            (&current, request.captured_at),
            earlier.as_ref().map(|(bytes, at)| (bytes.as_slice(), *at)),
            url,
            calibration,
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
        let destination = self.composite_destination(request.frame, previous);
        write_image(&self.whole.directory, &destination, &composed.jpeg)?;
        Ok(Ok(ShownImage {
            path: destination,
            note: Some(composite_note(earlier.is_none())),
        }))
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

    fn shown_image(&self, request: &CheckImageRequest<'_>) -> Result<ShownImage, String> {
        let Some(print_id) = request.print_id else {
            return self.whole_frame(request.frame, &WholeFrameReason::NoPrint);
        };
        let previous = self.alignments.previous_check(print_id);
        let shown = match self.composite(print_id, request, previous.as_ref())? {
            Ok(composite) => composite,
            Err(reason) => self.whole_frame(request.frame, &reason)?,
        };
        if request.made_for == ImageUse::Check {
            self.alignments.record_check(
                print_id,
                &PreviousCheck {
                    frame: request.frame.to_path_buf(),
                    captured_at: request.captured_at,
                },
            );
        }
        Ok(shown)
    }
}
