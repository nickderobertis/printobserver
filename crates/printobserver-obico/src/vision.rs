//! The vision port, as the self-hosted Obico webhook plugin fills it, and the
//! one call this adapter makes to Obico's own API: acknowledging an alert.

use core::fmt;
use core::time::Duration;

use printobserver_types::{RawBytes, Timestamp};
use printobserver_vision_api::{
    BoxFuture, Detection, FetchedImage, NormalizedAlert, VisionError, VisionPort, WebAddress,
};

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

/// The bearer token Obico's own API is reached with.
///
/// Obico's user API accepts an `OAuth2` bearer token or a browser session and
/// nothing else, so this is a token a self-hosted instance's own administration
/// issued. There is no way to hold an empty one, and neither rendering of this
/// type shows it: the one place its text is read is the request it
/// authenticates.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken(String);

impl AccessToken {
    /// The token this text names.
    ///
    /// # Errors
    ///
    /// Answers why the text cannot be a bearer token, in words that never
    /// quote it: empty or nothing but whitespace, or carrying a space or a
    /// character outside printable ASCII, which no `Authorization` header
    /// carries intact.
    pub fn new(text: &str) -> Result<Self, &'static str> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("it is empty, and Obico's API refuses a request carrying no token");
        }
        if !trimmed.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(
                "it carries a space or a character outside printable ASCII, which no \
                 `Authorization` header carries intact",
            );
        }
        Ok(Self(trimmed.to_owned()))
    }
}

impl fmt::Display for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessToken(<redacted>)")
    }
}

/// Where Obico's own API answers, and the token it is reached with.
///
/// Built from an address this system has ruled on and a token it has, so a
/// configuration carrying one carries both; the debug form shows the address
/// and never the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObicoApi {
    /// The server's own address, such as `http://127.0.0.1:3334`.
    url: WebAddress,
    /// The bearer token its API is reached with.
    access_token: AccessToken,
}

impl ObicoApi {
    /// Obico's API at one address, reached with one token.
    #[must_use]
    pub const fn new(url: WebAddress, access_token: AccessToken) -> Self {
        Self { url, access_token }
    }

    /// The server's own address.
    #[must_use]
    pub const fn url(&self) -> &WebAddress {
        &self.url
    }
}

/// The alert overwrite a handled detection is acknowledged with.
///
/// `FAILED` rather than `NOT_FAILED`: the detection was right, and it was
/// handled by adjusting the print rather than by dismissing it. Obico's
/// suppression reads only that an acknowledgement happened, so either would
/// re-arm it; this one is the truthful one.
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
pub const HANDLED_OVERWRITE: &str = "FAILED";

/// The path, under Obico's own address, one printer's alert is acknowledged at.
fn acknowledgement_path(provider_printer_id: i64) -> String {
    format!(
        // llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
        "/api/v1/printers/{provider_printer_id}/acknowledge_alert/?alert_overwrite={HANDLED_OVERWRITE}"
    )
}

/// The Obico adapter: one body read, one snapshot retrieved, one alert
/// acknowledged.
#[derive(Debug, Clone)]
pub struct ObicoVision {
    /// The HTTP client the snapshot is fetched with, carrying the timeout.
    client: reqwest::Client,
    /// The bounds the fetch runs under.
    config: ObicoVisionConfig,
    /// Obico's own API, when this adapter was given a way to reach it.
    api: Option<ObicoApi>,
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
        Ok(Self {
            client,
            config,
            api: None,
        })
    }

    /// The same adapter, able to reach Obico's own API when one is given.
    #[must_use]
    pub fn with_api(mut self, api: Option<ObicoApi>) -> Self {
        self.api = api;
        self
    }

    /// The bounds this adapter runs under.
    #[must_use]
    pub const fn config(&self) -> &ObicoVisionConfig {
        &self.config
    }

    /// Acknowledge the alert Obico holds against one printer's current print,
    /// wherever this is polled.
    ///
    /// The supervision core asks for an acknowledgement from its own expiry
    /// driver too — a thread that polls a port's futures with no reactor, as
    /// `printobserver_core::block_on` says every port must allow — and this
    /// adapter's HTTP client needs Tokio's. So a call made inside a Tokio
    /// runtime runs on it, and one made anywhere else runs on a current-thread
    /// runtime of its own, with a client of its own, built for the call: a
    /// client's pooled connections belong to the runtime they were opened on.
    async fn acknowledge_anywhere(&self, provider_printer_id: i64) -> Result<(), VisionError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return self.acknowledge(&self.client, provider_printer_id).await;
        }
        let unavailable = |error: &dyn fmt::Display| VisionError::Unreachable {
            detail: format!("no runtime could be built to reach Obico on: {error}"),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| unavailable(&error))?;
        let client = reqwest::Client::builder()
            .timeout(self.config.fetch_timeout)
            .build()
            .map_err(|error| unavailable(&error))?;
        runtime.block_on(self.acknowledge(&client, provider_printer_id))
    }

    /// Acknowledge the alert Obico holds against one printer's current print.
    ///
    /// What a refusal says names the address and the status Obico answered,
    /// and never the token.
    async fn acknowledge(
        &self,
        client: &reqwest::Client,
        provider_printer_id: i64,
    ) -> Result<(), VisionError> {
        let Some(api) = &self.api else {
            return Err(VisionError::NotConfigured {
                detail: "no [obico] url and access_token are configured, so Obico cannot be \
                         told its detection was handled"
                    .to_owned(),
            });
        };
        let url = format!(
            "{}{}",
            api.url.as_str().trim_end_matches('/'),
            acknowledgement_path(provider_printer_id)
        );
        let response = client
            .post(&url)
            .bearer_auth(&api.access_token.0)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    VisionError::TimedOut
                } else {
                    VisionError::Unreachable {
                        detail: format!("{url} could not be reached: {}", error.without_url()),
                    }
                }
            })?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        Err(VisionError::Unreachable {
            detail: format!("{url} answered {status} acknowledging the alert"),
        })
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

    fn clear_detection(&self, detection: Detection) -> BoxFuture<'_, Result<(), VisionError>> {
        Box::pin(async move {
            self.acknowledge_anywhere(detection.provider_printer_id)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessToken, ObicoApi, ObicoVision, ObicoVisionConfig, ObicoVisionError};
    use printobserver_vision_api::WebAddress;

    /// A host that cannot carry the adapter is told which half failed.
    #[test]
    fn a_client_that_cannot_be_built_says_so() {
        let error = ObicoVisionError {
            detail: "no TLS backend".to_owned(),
        };
        assert_eq!(error.detail(), "no TLS backend");
        assert!(error.to_string().contains("no TLS backend"));
    }

    /// The adapter runs under the bounds it was given.
    #[test]
    fn the_adapter_carries_the_bounds_it_was_given() {
        let config = ObicoVisionConfig {
            max_image_bytes: 17,
            ..ObicoVisionConfig::default()
        };
        let vision = ObicoVision::new(config).expect("the adapter builds");
        assert_eq!(*vision.config(), config);
    }

    /// A token is printable text with no space in it, and nothing else.
    #[test]
    fn an_access_token_is_printable_text_with_no_space() {
        assert!(AccessToken::new(" a-token ").is_ok());
        for refused in ["", "   ", "two words", "a\ttab", "non-ascii-é"] {
            assert!(
                AccessToken::new(refused).is_err(),
                "{refused:?} was accepted"
            );
        }
    }

    /// Neither the API's nor the adapter's debug form shows the token.
    #[test]
    fn no_debug_form_shows_the_access_token() {
        let token = AccessToken::new("a-token-nobody-may-read").expect("a token");
        let api = ObicoApi::new(
            WebAddress::new("http://127.0.0.1:3334").expect("an address"),
            token.clone(),
        );
        let vision = ObicoVision::new(ObicoVisionConfig::default())
            .expect("the adapter builds")
            .with_api(Some(api.clone()));
        for shown in [
            format!("{api:?}"),
            format!("{vision:?}"),
            format!("{token:?}"),
            token.to_string(),
        ] {
            assert!(!shown.contains("a-token-nobody-may-read"), "{shown}");
        }
    }
}
