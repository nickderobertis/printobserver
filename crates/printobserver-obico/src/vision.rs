//! The vision port, as the self-hosted Obico webhook plugin fills it.

use core::fmt;
use core::time::Duration;

use printobserver_types::{RawBytes, Timestamp};
use printobserver_vision_api::{BoxFuture, FetchedImage, NormalizedAlert, VisionError, VisionPort};

use crate::{fetch, normalize};

/// How long this adapter waits for a snapshot before giving up.
///
/// Long enough for a camera snapshot over a home network and short enough that
/// a supervision decision is not held behind a host that has stopped answering.
pub const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The most bytes of snapshot this adapter will read.
///
/// Eight mebibytes: comfortably above any webcam still a printer's camera
/// produces, and far below anything that would exhaust the small machine beside
/// the printer.
pub const DEFAULT_MAX_IMAGE_BYTES: i64 = 8 * 1024 * 1024;

/// The two bounds a snapshot fetch runs under.
///
/// Both have defaults here rather than only at a call site, so that a reader
/// can see what they are; [`ObicoVision::new`] takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObicoVisionConfig {
    /// How long to wait for the whole response.
    pub fetch_timeout: Duration,
    /// The most bytes to read of it.
    pub max_image_bytes: i64,
}

impl Default for ObicoVisionConfig {
    fn default() -> Self {
        Self {
            fetch_timeout: DEFAULT_FETCH_TIMEOUT,
            max_image_bytes: DEFAULT_MAX_IMAGE_BYTES,
        }
    }
}

/// Why this adapter could not be built at all.
///
/// Distinct from [`VisionError`], which says why one *body* or one *image* was
/// refused: this is the HTTP client itself failing to come up, which is a
/// misconfigured host rather than anything an alert did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObicoVisionError {
    /// What went wrong building the client.
    detail: String,
}

impl ObicoVisionError {
    /// What went wrong building the client.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ObicoVisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the Obico adapter could not be built: {}",
            self.detail
        )
    }
}

impl core::error::Error for ObicoVisionError {}

/// The Obico adapter: one body read, one snapshot retrieved.
#[derive(Debug, Clone)]
pub struct ObicoVision {
    /// The HTTP client the snapshot is fetched with, carrying the timeout.
    client: reqwest::Client,
    /// The bounds the fetch runs under.
    config: ObicoVisionConfig,
}

impl ObicoVision {
    /// Build the adapter under the bounds given.
    ///
    /// # Errors
    ///
    /// Returns [`ObicoVisionError`] when the HTTP client cannot be built, which
    /// is the host's TLS or proxy configuration rather than anything an alert
    /// did.
    pub fn new(config: ObicoVisionConfig) -> Result<Self, ObicoVisionError> {
        let client = reqwest::Client::builder()
            .timeout(config.fetch_timeout)
            .build()
            .map_err(|error| ObicoVisionError {
                detail: error.to_string(),
            })?;
        Ok(Self { client, config })
    }

    /// The bounds this adapter runs under.
    #[must_use]
    pub const fn config(&self) -> &ObicoVisionConfig {
        &self.config
    }
}

impl VisionPort for ObicoVision {
    fn normalize(
        &self,
        body: RawBytes,
        content_type: Option<String>,
    ) -> BoxFuture<'_, Result<NormalizedAlert, VisionError>> {
        // Reading a body reaches nothing and waits on nothing, so it is done
        // here and the future answers what it produced.
        let read = normalize::read(&body, content_type.as_deref(), Timestamp::now());
        Box::pin(async move { read })
    }

    fn fetch_image(&self, source_url: String) -> BoxFuture<'_, Result<FetchedImage, VisionError>> {
        Box::pin(async move {
            fetch::image(&self.client, &source_url, self.config.max_image_bytes).await
        })
    }
}
