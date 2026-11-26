//! The image a check is shown, chosen in one place.
//!
//! A frame is stored at the size the camera took it, and the operator's own
//! image read serves exactly that. What a supervision turn is shown — the
//! image attached to its prompt and the image a look answers — is whatever the
//! installed [`CheckImages`] makes of the stored frame, because every call of a
//! turn re-sends the images it holds. With none installed, a check is shown the
//! stored frame itself.
//!
//! What makes the image is told what it is for — a check, or a look — and
//! where the print stood in its own G-code when the check began, so that it can
//! draw that plan on the frame; it answers the image and one line saying what
//! that image is, which the turn's prompt carries.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_printer_api::JobSnapshot;
use printobserver_types::{EventRecord, ImageRef, PrintId, Timestamp};

use crate::gcode_index::GcodeIndex;
use crate::records::PrintRecord;
use crate::supervisor::Supervisor;

/// What a check's image is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageUse {
    /// The image attached to a supervision turn's prompt: the print's next
    /// check is shown this one's frame as the previous.
    Check,
    /// The image a look answers, which moves no check along.
    Look,
}

/// Where a print stood in its own G-code: the index of its file, and the
/// job's file position in it.
#[derive(Debug, Clone)]
pub struct PlanPosition {
    /// The print's G-code index.
    pub index: Arc<GcodeIndex>,
    /// The job's file position, in bytes.
    pub file_position_bytes: u64,
}

/// One request for a check's image of a stored frame.
#[derive(Debug, Clone)]
pub struct CheckImageRequest<'a> {
    /// The print the image is of; none for a look at the printer.
    pub print_id: Option<PrintId>,
    /// Where the stored frame is.
    pub frame: &'a Path,
    /// When the frame was captured.
    pub captured_at: Timestamp,
    /// What the image is for.
    pub made_for: ImageUse,
    /// Where the print stood in its G-code, when its job is its own file and
    /// reported a position the index was read for.
    pub plan: Option<PlanPosition>,
}

/// A check's image, and the line saying what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownImage {
    /// The image's path.
    pub path: PathBuf,
    /// What the image is, and why it is that one, in one line; none when what
    /// made it says nothing.
    pub note: Option<String>,
    /// Whether it is the three-panel image, drawn from the print's plan,
    /// rather than a frame as the camera took it or scaled whole.
    pub composite: bool,
}

/// What makes a check's image of one stored frame.
pub trait CheckImages: Send + Sync {
    /// The path of the image a check of `print_id` — or a look at the printer,
    /// when it names none — is shown of the frame stored at `frame`, or why
    /// none could be made.
    ///
    /// # Errors
    ///
    /// Answers why the image could not be made: the frame unreadable, or the
    /// image unwritable.
    fn check_image(&self, print_id: Option<PrintId>, frame: &Path) -> Result<PathBuf, String>;

    /// The image one request is shown and the line saying what it is: by
    /// default [`check_image`](Self::check_image)'s, saying nothing.
    ///
    /// # Errors
    ///
    /// As [`check_image`](Self::check_image)'s.
    fn shown_image(&self, request: &CheckImageRequest<'_>) -> Result<ShownImage, String> {
        self.check_image(request.print_id, request.frame)
            .map(|path| ShownImage {
                path,
                note: None,
                composite: false,
            })
    }

    /// Whether a check of `print_id` would be shown the three-panel image of a
    /// frame from the server's own camera: by default, never.
    fn composes(&self, print_id: PrintId) -> bool {
        let _ = print_id;
        false
    }

    /// Measure `print_id`'s growth only from frames checked after now: it
    /// waited at a planned change, and a frame from before the wait would
    /// count the wait as no growth. By default there is nothing to forget.
    fn restart_growth(&self, print_id: PrintId) {
        let _ = print_id;
    }
}

/// When an event's frame was captured: what the arrival knew, else what its
/// payload says, else when the event was received.
pub(crate) fn captured_at_of(event: &EventRecord, known: Option<Timestamp>) -> Timestamp {
    known
        .or_else(|| {
            event
                .body
                .payload
                .get("frame_captured_at")
                .cloned()
                .and_then(|value| printobserver_types::serde_json::from_value(value).ok())
        })
        .unwrap_or(event.received_at)
}

impl Supervisor {
    /// Install what makes every check's image. The first installation stands;
    /// answers whether this one did.
    pub fn install_check_images(&self, images: std::sync::Arc<dyn CheckImages>) -> bool {
        self.check_images.set(images).is_ok()
    }

    /// Where a print stood in its own G-code by a job read: nothing when the
    /// job runs another file, reported no position, or the file has no index.
    pub(crate) async fn plan_position(
        &self,
        print: &PrintRecord,
        job: Option<&JobSnapshot>,
    ) -> Option<PlanPosition> {
        let job = job?;
        if job.file_name.is_none() || job.file_name != print.file_name {
            return None;
        }
        let file_position_bytes = u64::try_from(job.file_position_bytes?).ok()?;
        let index = self.print_gcode_index(print.id).await?;
        Some(PlanPosition {
            index,
            file_position_bytes,
        })
    }

    /// The image a check is shown of one stored frame, and the line saying
    /// what it is: the stored frame itself when nothing that makes one is
    /// installed, or when what is installed could not make it, which is a
    /// check shown more than it needs rather than a check shown nothing.
    pub(crate) async fn shown_image_of(
        &self,
        image: &ImageRef,
        print_id: Option<PrintId>,
        captured_at: Timestamp,
        made_for: ImageUse,
        plan: Option<PlanPosition>,
    ) -> Option<ShownImage> {
        let stored = self.image_path(image).await?;
        let Some(images) = self.check_images.get() else {
            return Some(ShownImage {
                path: stored,
                note: None,
                composite: false,
            });
        };
        let request = CheckImageRequest {
            print_id,
            frame: &stored,
            captured_at,
            made_for,
            plan,
        };
        Some(
            images
                .shown_image(&request)
                .unwrap_or_else(|error| ShownImage {
                    note: Some(format!(
                        "the frame as the camera took it, because its check image could not be made: {error}"
                    )),
                    path: stored.clone(),
                    composite: false,
                }),
        )
    }
}
