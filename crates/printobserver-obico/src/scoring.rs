//! Reading Obico's own record of the printer's live prediction, for the
//! detector's status.
//!
//! Obico keeps, on the printer, the print it is watching (`current_print`), its
//! latest snapshot (`pic`) and the latest score of it (`normalized_p`), and
//! answers all three on `GET /api/v1/printers/{id}/` — the one read this makes,
//! once per observation interval, with the bearer `access_token` of
//! `[obico]`. Obico refuses the printer's own token there. A print's
//! per-frame `prediction_json` is not read: Obico writes it only once the
//! print has ended.
//!
//! # What of the answer is kept
//!
//! The answer carries the printer's own credential, `auth_token`, beside the
//! three fields this needs. So the answer is read into a shape naming only
//! those three, and nothing else of it is held, logged, stored or forwarded;
//! a refusal names the address and the status Obico answered, and never the
//! body. The snapshot and the score are kept only as a fingerprint that moves
//! when either does, which is the evidence the supervision core reads
//! ([`printobserver_core::scoring`]).
//!
//! # Which printer
//!
//! The printer is the one `[obico] printer_id` names, when it names one, and
//! otherwise the one the last body Obico posted named as `printer.id` — every
//! body it posts names it. Until one is known no read is made, and the status
//! says why.

use core::fmt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};
use std::sync::{Arc, Mutex, PoisonError};

use printobserver_core::{ScoringProbe, ScoringRead, ScoringReading};
use printobserver_types::Timestamp;
use printobserver_types::serde::Deserialize;
use printobserver_types::serde_json::Value;
use printobserver_vision_api::{BoxFuture, ProviderPrint};

use crate::events::ObicoFailureAlertPayload;
use crate::vision::{DEFAULT_FETCH_TIMEOUT, ObicoApi, ObicoVisionError};
use printobserver_types::EventPayload as _;

/// The path, under Obico's own address, one printer's record is read at.
fn printer_path(printer_id: ObicoPrinterId) -> String {
    let id = printer_id.get();
    // llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
    format!("/api/v1/printers/{id}/")
}

/// Obico's own identifier for one printer: a whole number from 1, which is
/// what Obico's database mints and the number in its printer page's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObicoPrinterId(i64);

impl ObicoPrinterId {
    /// The printer this number names.
    ///
    /// # Errors
    ///
    /// Answers why a number below 1 names no printer of Obico's.
    pub const fn new(id: i64) -> Result<Self, &'static str> {
        if id < 1 {
            return Err("it must be Obico's own identifier for a printer, a whole number from 1");
        }
        Ok(Self(id))
    }

    /// The number itself.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// The printer the last body Obico posted named, shared between the adapter
/// that reads those bodies and the reader of the printer's record.
#[derive(Debug, Clone, Default)]
pub struct KnownPrinter(Arc<Mutex<Option<ObicoPrinterId>>>);

impl KnownPrinter {
    /// Nothing known yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Note the printer one body Obico posted names, when it names one.
    pub(crate) fn note_body(&self, body: &[u8]) {
        let named = printobserver_types::serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|document| document.get("printer")?.get("id")?.as_i64())
            .and_then(|id| ObicoPrinterId::new(id).ok());
        if let Some(id) = named {
            *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(id);
        }
    }

    /// The printer last named, when one has been.
    #[must_use]
    pub fn get(&self) -> Option<ObicoPrinterId> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Obico's printer answer, as far as the status reads it: which printer it
/// is, and the three fields the status needs. What is not named here — the
/// printer's own `auth_token` above all — is never read into anything.
///
/// Each of the four is one Obico always sends, `null` where it has nothing, so
/// an answer missing one is no printer record of Obico's and is refused rather
/// than read as a printer watching nothing.
#[derive(Deserialize)]
#[serde(crate = "printobserver_types::serde")]
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
struct PrinterAnswer {
    /// Which printer this is.
    id: i64,
    /// The print Obico is watching, when it is watching one.
    #[serde(deserialize_with = "present")]
    current_print: Option<CurrentPrint>,
    /// The latest snapshot, when Obico has taken one.
    #[serde(deserialize_with = "present")]
    pic: Option<Snapshot>,
    /// The latest score of it, when Obico has scored one.
    #[serde(deserialize_with = "present")]
    normalized_p: Option<f64>,
}

/// The latest snapshot Obico holds of the printer: where it is kept, which a
/// new snapshot moves.
#[derive(Deserialize)]
#[serde(crate = "printobserver_types::serde")]
struct Snapshot {
    /// Where Obico keeps it.
    img_url: String,
}

/// The print Obico is watching, as far as it is matched to one of this
/// system's.
#[derive(Deserialize)]
#[serde(crate = "printobserver_types::serde")]
struct CurrentPrint {
    /// Obico's own identifier for it.
    id: i64,
    /// The file it is printing.
    #[serde(deserialize_with = "present")]
    filename: Option<String>,
    /// When it started, as Obico writes an instant: refused when it is text
    /// that is not one, because an identity matched on a start nobody can read
    /// is matched on nothing.
    #[serde(deserialize_with = "an_instant")]
    started_at: Option<Timestamp>,
}

/// Read a value that may be `null` but may not be missing.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: printobserver_types::serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

/// Read an instant Obico writes, `null` when it wrote none; text that is not
/// an instant is refused.
fn an_instant<'de, D>(deserializer: D) -> Result<Option<Timestamp>, D::Error>
where
    D: printobserver_types::serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|text| {
            text.parse::<Timestamp>().map_err(|_| {
                printobserver_types::serde::de::Error::custom("`started_at` is not an instant")
            })
        })
        .transpose()
}

impl PrinterAnswer {
    /// What the supervision core reads of it.
    fn reading(&self) -> ScoringReading {
        let mut hasher = DefaultHasher::new();
        self.pic
            .as_ref()
            .map(|snapshot| snapshot.img_url.as_str())
            .hash(&mut hasher);
        self.normalized_p.map(f64::to_bits).hash(&mut hasher);
        ScoringReading {
            print: self.current_print.as_ref().map(|print| ProviderPrint {
                id: print.id,
                file_name: print.filename.clone().filter(|name| !name.is_empty()),
                started_at: print.started_at,
            }),
            evidence: hasher.finish(),
        }
    }
}

/// The reader of Obico's printer record.
#[derive(Clone)]
pub struct ObicoScoring {
    /// The HTTP client it is read with.
    client: reqwest::Client,
    /// Obico's own API.
    api: ObicoApi,
    /// The printer `[obico] printer_id` names, when it names one.
    configured: Option<ObicoPrinterId>,
    /// The printer the last body Obico posted named.
    known: KnownPrinter,
}

impl fmt::Debug for ObicoScoring {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObicoScoring")
            .field("api", &self.api)
            .field("configured", &self.configured)
            .finish_non_exhaustive()
    }
}

impl ObicoScoring {
    /// The reader against one API, of the printer configured or, failing
    /// that, the one Obico last named.
    ///
    /// # Errors
    ///
    /// Returns [`ObicoVisionError`] when the HTTP client cannot be built.
    pub fn new(
        api: ObicoApi,
        configured: Option<ObicoPrinterId>,
        known: KnownPrinter,
    ) -> Result<Self, ObicoVisionError> {
        let client = reqwest::Client::builder()
            .timeout(DEFAULT_FETCH_TIMEOUT)
            .build()
            .map_err(|error| ObicoVisionError::of(error.to_string()))?;
        Ok(Self {
            client,
            api,
            configured,
            known,
        })
    }

    /// Read the record once, wherever this is polled: on the caller's own
    /// runtime when it is inside one, and on one of its own otherwise, as an
    /// acknowledgement is.
    async fn read_anywhere(&self) -> ScoringRead {
        let Some(printer_id) = self.configured.or_else(|| self.known.get()) else {
            return ScoringRead::NotRead(
                "no Obico printer is known yet: no [obico] printer_id is configured and Obico \
                 has posted nothing naming one since this server started"
                    .to_owned(),
            );
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            return self.read(&self.client, printer_id).await;
        }
        let built = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                reqwest::Client::builder()
                    .timeout(DEFAULT_FETCH_TIMEOUT)
                    .build()
                    .map(|client| (runtime, client))
                    .map_err(|error| error.to_string())
            });
        match built {
            Ok((runtime, client)) => runtime.block_on(self.read(&client, printer_id)),
            Err(error) => ScoringRead::Failed(format!(
                "no runtime could be built to reach Obico on: {error}"
            )),
        }
    }

    /// Read one printer's record.
    async fn read(&self, client: &reqwest::Client, printer_id: ObicoPrinterId) -> ScoringRead {
        let url = format!(
            "{}{}",
            self.api.url().as_str().trim_end_matches('/'),
            printer_path(printer_id)
        );
        let response = match client
            .get(&url)
            .bearer_auth(self.api.token_text())
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                return ScoringRead::Failed(format!(
                    "{url} did not answer within {}s",
                    DEFAULT_FETCH_TIMEOUT.as_secs()
                ));
            }
            Err(error) => {
                return ScoringRead::Failed(format!(
                    "{url} could not be reached: {}",
                    error.without_url()
                ));
            }
        };
        let status = response.status();
        if !status.is_success() {
            return ScoringRead::Failed(format!("{url} answered {status}"));
        }
        let Ok(body) = response.bytes().await else {
            return ScoringRead::Failed(format!("{url} answered {status} and then broke off"));
        };
        match printobserver_types::serde_json::from_slice::<PrinterAnswer>(&body) {
            Ok(answer) if answer.id == printer_id.get() => ScoringRead::Read(answer.reading()),
            Ok(_) => ScoringRead::Failed(format!(
                "{url} answered {status} with the record of another printer"
            )),
            // The body is not quoted: it carries the printer's own credential.
            Err(_) => ScoringRead::Failed(format!(
                "{url} answered {status} with a body that is not Obico's printer record"
            )),
        }
    }
}

impl ScoringProbe for ObicoScoring {
    fn read_scoring(&self) -> BoxFuture<'_, ScoringRead> {
        Box::pin(self.read_anywhere())
    }

    fn alert_kind(&self) -> &str {
        ObicoFailureAlertPayload::KIND
    }
}
