//! Asking the failure detector itself whether it is answering.
//!
//! `Obico`'s web service answers its own API while the detector behind it is
//! down, so a call to the API says nothing about detection. The detector — the
//! `ml_api` service — answers an unauthenticated `GET /hc/` of its own, and
//! that is the check: `200` is available; a refused or reset connection, any
//! other answer, or none within [`HEALTH_TIMEOUT`] is unavailable. No
//! credential is sent with it, because the route takes none and a credential
//! sent where none is needed is one more place it can be read.

use core::time::Duration;

use printobserver_core::DetectorProbe;
use printobserver_vision_api::{BoxFuture, WebAddress};
use reqwest::Client;

/// How long the detector has to answer its health check.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

/// The detector's own health route, under its address.
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
pub const HEALTH_PATH: &str = "hc/";

/// The failure detector's health check, against one address.
#[derive(Debug, Clone)]
pub struct DetectorHealthCheck {
    /// The client it is asked with, carrying no credential.
    client: Client,
    /// The health route.
    url: String,
}

impl DetectorHealthCheck {
    /// The check against the detector service one address names.
    ///
    /// # Errors
    ///
    /// Answers why the HTTP client could not be built, which is a host that
    /// cannot carry one.
    pub fn new(ml_api_url: &WebAddress) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(HEALTH_TIMEOUT)
            .build()
            .map_err(|error| error.to_string())?;
        let base = ml_api_url.as_str().trim_end_matches('/');
        Ok(Self {
            client,
            url: format!("{base}/{HEALTH_PATH}"),
        })
    }

    /// Ask once.
    async fn ask(&self) -> Result<String, String> {
        match self.client.get(&self.url).send().await {
            Ok(response) if response.status().as_u16() == 200 => {
                Ok("the detector's health check answered 200".to_owned())
            }
            Ok(response) => Err(format!(
                "the detector's health check answered {}",
                response.status().as_u16()
            )),
            Err(error) if error.is_timeout() => Err(format!(
                "the detector's health check did not answer within {}s",
                HEALTH_TIMEOUT.as_secs()
            )),
            Err(error) => Err(format!(
                "the detector's health check could not be reached: {error}"
            )),
        }
    }
}

impl DetectorProbe for DetectorHealthCheck {
    fn check(&self) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(self.ask())
    }
}
