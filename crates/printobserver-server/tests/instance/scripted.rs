//! The scripted `OctoPrint` environment, as this tier finds it.
//!
//! `just octoprint-up` provisions a real `OctoPrint` with its virtual printer,
//! starts a hold print, and writes what it started to `.octoprint-env`. This
//! reads that record. There is no fallback and no skip: a tier that quietly
//! passed when the environment was not up would prove nothing, so an absent
//! record is a panic naming the recipe that produces one.

use std::path::{Path, PathBuf};

/// Where `just octoprint-up` keeps what it started.
const STATE_DIR: &str = ".octoprint-env";

/// The record it writes there.
const RECORD: &str = "instance.json";

/// The file it writes the provisioned key to.
const API_KEY_FILE: &str = "api-key";

/// The running instance, and what reaches it.
#[derive(Debug, Clone)]
pub struct Scripted {
    /// Where it answers.
    pub url: String,
    /// The key it was provisioned with.
    pub api_key: String,
    /// Whether `up` started a print.
    pub printing: bool,
}

/// The instance `just octoprint-up` started.
///
/// # Panics
///
/// Panics when the environment is not up, naming the recipe that brings it up.
pub fn scripted() -> Scripted {
    let state = workspace_root().join(STATE_DIR);
    let record = read_or_explain(&state.join(RECORD));
    let key = read_or_explain(&state.join(API_KEY_FILE));
    let record: printobserver_types::serde_json::Value =
        printobserver_types::serde_json::from_str(&record).expect("the record is JSON");
    Scripted {
        url: record["url"]
            .as_str()
            .expect("the record names a URL")
            .to_owned(),
        api_key: key.trim().to_owned(),
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
