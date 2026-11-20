//! The image every check is shown, made of the frame the camera gave.
//!
//! The one place that chooses it: every check is shown the whole frame scaled
//! to [`WHOLE_FRAME_WIDTH`] by [`WHOLE_FRAME_HEIGHT`] and never cropped, written
//! once per stored frame under `<state_dir>/check-images/`, while the stored
//! frame keeps the camera's size. A frame that fits already is shown as stored.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use printobserver_core::CheckImages;
use printobserver_panels::{WHOLE_FRAME_HEIGHT, WHOLE_FRAME_WIDTH, whole_frame};
use printobserver_types::PrintId;

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
        std::fs::create_dir_all(&self.directory)
            .map_err(|error| format!("{} cannot be made: {error}", self.directory.display()))?;
        // Written beside its name and moved onto it, so a reader never opens a
        // half-written image.
        let partial = destination.with_extension(format!(
            "{}-{}.partial",
            std::process::id(),
            WRITES.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&partial, &scaled.jpeg)
            .and_then(|()| std::fs::rename(&partial, &destination))
            .map_err(|error| format!("{} cannot be written: {error}", destination.display()))?;
        Ok(destination)
    }
}
