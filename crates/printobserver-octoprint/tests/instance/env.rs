//! The scripted `OctoPrint` environment, as this crate's integration tier finds it.
//!
//! `just octoprint-up` provisions a real `OctoPrint` with its virtual printer,
//! starts a hold print, and writes what it started to `.octoprint-env`. This
//! reads that record. There is no fallback and no skip: a tier that quietly
//! passed when the environment was not up would prove nothing, so an absent
//! record is a panic naming the recipe that produces one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use printobserver_octoprint::{FanSupport, OctoPrintConfig, OctoPrintPrinter};

/// Where `just octoprint-up` keeps what it started.
const STATE_DIR: &str = ".octoprint-env";

/// The record it writes there.
const RECORD: &str = "instance.json";

/// The file it writes the provisioned key to.
const API_KEY_FILE: &str = "api-key";

/// How long one request to the instance may take.
///
/// Longer than the adapter's own default, because a virtual printer part-way
/// through a ten-second dwell answers when the dwell does.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The running instance, and what reaches it.
#[derive(Debug, Clone)]
pub struct Scripted {
    /// Where it answers.
    pub url: String,
    /// The key it was provisioned with.
    pub api_key: String,
    /// The minimum the hold print keeps running for, from when `up` answered.
    pub hold_seconds: u64,
    /// Whether `up` started a print.
    pub printing: bool,
}

impl Scripted {
    /// A printer speaking to this instance directly.
    pub fn printer(&self) -> OctoPrintPrinter {
        OctoPrintPrinter::new(self.config(&self.url))
    }

    /// A printer speaking to whatever is in front of this instance.
    pub fn printer_at(&self, url: &str) -> OctoPrintPrinter {
        OctoPrintPrinter::new(self.config(url))
    }

    /// A printer speaking to this instance with a key it does not know.
    pub fn printer_with_a_wrong_key(&self) -> OctoPrintPrinter {
        OctoPrintPrinter::new(
            OctoPrintConfig::new(&self.url, "not-the-provisioned-key")
                .expect("a configuration")
                .with_timeout(TIMEOUT),
        )
    }

    /// A printer whose operator says the machine has no commandable fan.
    pub fn printer_with_no_fan(&self) -> OctoPrintPrinter {
        OctoPrintPrinter::new(self.config(&self.url).with_fan(FanSupport::Absent))
    }

    /// The configuration reaching one URL with this instance's key.
    fn config(&self, url: &str) -> OctoPrintConfig {
        OctoPrintConfig::new(url, self.api_key.clone())
            .expect("a configuration")
            .with_timeout(TIMEOUT)
    }
}

/// The instance `just octoprint-up` started.
///
/// # Panics
///
/// Panics when the environment is not up, naming the recipe that brings it up.
pub fn scripted() -> Scripted {
    let state = workspace_root().join(STATE_DIR);
    let record_path = state.join(RECORD);
    let record = read_or_explain(&record_path);
    let key = read_or_explain(&state.join(API_KEY_FILE));
    let record: serde_json::Value = serde_json::from_str(&record)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", record_path.display()));
    Scripted {
        url: record["url"]
            .as_str()
            .expect("the record names a URL")
            .to_owned(),
        api_key: key.trim().to_owned(),
        hold_seconds: record["hold_seconds"]
            .as_u64()
            .expect("the record names the hold"),
        printing: record["printing"].as_bool().unwrap_or(false),
    }
}

/// One file of the state directory, or a panic saying how to produce it.
fn read_or_explain(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "the scripted OctoPrint environment is not up: {} could not be read ({error}). \
             Run `just octoprint-up` first; this tier drives a real OctoPrint and has no \
             fixture to fall back to.",
            path.display()
        )
    })
}

/// The root of this workspace, which is where the state directory lives.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}
