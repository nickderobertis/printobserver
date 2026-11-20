//! The image a check is shown, chosen in one place.
//!
//! A frame is stored at the size the camera took it, and the operator's own
//! image read serves exactly that. What a supervision turn is shown — the
//! image attached to its prompt and the image a look answers — is whatever the
//! installed [`CheckImages`] makes of the stored frame, because every call of a
//! turn re-sends the images it holds. With none installed, a check is shown the
//! stored frame itself.

use std::path::{Path, PathBuf};

use printobserver_types::{ImageRef, PrintId};

use crate::supervisor::Supervisor;

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
}

impl Supervisor {
    /// Install what makes every check's image. The first installation stands;
    /// answers whether this one did.
    pub fn install_check_images(&self, images: std::sync::Arc<dyn CheckImages>) -> bool {
        self.check_images.set(images).is_ok()
    }

    /// The path of the image a check is shown of one stored frame: the stored
    /// frame itself when nothing that makes one is installed, or when what is
    /// installed could not make it, which is a check shown more than it needs
    /// rather than a check shown nothing.
    pub(crate) async fn check_image_path(
        &self,
        print_id: Option<PrintId>,
        image: &ImageRef,
    ) -> Option<PathBuf> {
        let stored = self.image_path(image).await?;
        match self.check_images.get() {
            Some(images) => Some(images.check_image(print_id, &stored).unwrap_or(stored)),
            None => Some(stored),
        }
    }
}
