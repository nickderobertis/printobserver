//! Image bytes on the filesystem, addressed by what they are.
//!
//! An image is written under the state directory at a path derived from the
//! SHA-256 of its bytes, so the same image arriving twice is one file with two
//! rows naming it, and two different images can never land on one path. The
//! write is a write and a rename, so a crash between them leaves nothing at the
//! addressed path for a row to point at — and the row is written only after the
//! rename, so a crash leaves no row either.

use core::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use printobserver_store_api::StoreError;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::hold::HoldPoints;
use crate::values::io_error;

/// The directory image bytes live under, relative to the state directory.
pub const IMAGE_DIRECTORY: &str = "images";

/// The directory a write in progress lives under, beside the addressed ones.
///
/// Under the image directory rather than under the system's temporary one, so
/// that putting the bytes in place is a rename within one filesystem.
const PARTIAL_DIRECTORY: &str = ".partial";

/// How many characters of the digest name the directory a file is under.
const FANOUT: usize = 2;

/// One image's bytes, as the store recorded them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredImage {
    /// The SHA-256 of the bytes, lowercase hexadecimal.
    pub sha256: String,
    /// Where the bytes are, relative to the state directory.
    pub relative_path: String,
    /// How many bytes they are.
    pub byte_len: i64,
}

/// The SHA-256 of these bytes, lowercase hexadecimal.
#[must_use]
pub fn digest_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Where bytes of this digest live, relative to the state directory.
fn relative_path_for(digest: &str) -> String {
    format!("{IMAGE_DIRECTORY}/{}/{digest}", &digest[..FANOUT])
}

/// One relative path, resolved against the state directory.
pub(crate) fn resolve(state_dir: &Path, relative_path: &str) -> PathBuf {
    relative_path
        .split('/')
        .fold(state_dir.to_path_buf(), |path, part| path.join(part))
}

/// Store these bytes, once, at the address their content gives them.
///
/// Bytes already at that address are left exactly as they are: the address is a
/// function of the content, so what is there is what would have been written.
///
/// # Errors
///
/// Returns [`StoreError::Io`] when the bytes cannot be written or put in place.
pub(crate) fn store_bytes(
    state_dir: &Path,
    bytes: &[u8],
    points: &HoldPoints,
) -> Result<StoredImage, StoreError> {
    let sha256 = digest_of(bytes);
    let relative_path = relative_path_for(&sha256);
    let byte_len = i64::try_from(bytes.len()).map_err(|_| StoreError::Io {
        detail: format!(
            "{} bytes is more than an image record can hold",
            bytes.len()
        ),
    })?;
    let stored = StoredImage {
        sha256: sha256.clone(),
        relative_path: relative_path.clone(),
        byte_len,
    };

    let addressed = resolve(state_dir, &relative_path);
    if addressed.is_file() {
        return Ok(stored);
    }

    let partial_dir = state_dir.join(IMAGE_DIRECTORY).join(PARTIAL_DIRECTORY);
    std::fs::create_dir_all(&partial_dir).map_err(|error| io_error(&partial_dir, &error))?;
    let partial = partial_dir.join(format!("{}.part", Uuid::now_v7().as_hyphenated()));
    write_and_sync(&partial, bytes)?;

    // Between the bytes being written and their being put in place: a process
    // that ends here has left nothing at the addressed path and no row at all.
    points.image_write().reach(&sha256);

    if let Some(parent) = addressed.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    std::fs::rename(&partial, &addressed).map_err(|error| io_error(&addressed, &error))?;
    Ok(stored)
}

/// Write bytes to a file and get them onto the device before it is renamed.
fn write_and_sync(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = std::fs::File::create(path).map_err(|error| io_error(path, &error))?;
    file.write_all(bytes)
        .map_err(|error| io_error(path, &error))?;
    file.sync_all().map_err(|error| io_error(path, &error))?;
    Ok(())
}
