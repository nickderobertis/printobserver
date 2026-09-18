//! The scripted `OctoPrint` environment, as this crate's tier finds it.
//!
//! `just octoprint-up` provisions a real `OctoPrint` with its virtual printer,
//! starts a hold print, and writes what it started to `.octoprint-env`. This
//! reads that record. There is no fallback and no skip: a tier that quietly
//! passed when the environment was not up would prove nothing, so an absent
//! record is a panic naming the recipe that produces one.

use std::path::{Path, PathBuf};

use crate::world::Printer;

/// Where `just octoprint-up` keeps what it started.
const STATE_DIR: &str = ".octoprint-env";

/// The record it writes there.
const RECORD: &str = "instance.json";

/// The file it writes the provisioned key to.
const API_KEY_FILE: &str = "api-key";

/// The file it uploads and starts, which is the one this tier asks for.
const HOLD_PRINT: &str = "hold.gcode";

/// Where the committed copy of that file lives, under the workspace root.
const HOLD_PRINT_SOURCE: &str = "tools/octoprint-env/gcode/hold.gcode";

/// The instance `just octoprint-up` started, as this walk's printer.
///
/// # Panics
///
/// Panics when the environment is not up, naming the recipe that brings it up.
pub fn scripted() -> Printer {
    let state = workspace_root().join(STATE_DIR);
    let record = read_or_explain(&state.join(RECORD));
    let api_key = read_or_explain(&state.join(API_KEY_FILE));
    let record: printobserver_types::serde_json::Value =
        printobserver_types::serde_json::from_str(&record).expect("the record is JSON");
    assert!(
        record["printing"].as_bool().unwrap_or(false),
        "the scripted environment started no print, and this walk needs a machine that is \
         doing something"
    );
    Printer::Scripted {
        url: record["url"]
            .as_str()
            .expect("the record names a URL")
            .to_owned(),
        api_key: api_key.trim().to_owned(),
        file: HOLD_PRINT.to_owned(),
        runs_for_s: dwell_seconds(&read_or_explain(&workspace_root().join(HOLD_PRINT_SOURCE))),
    }
}

/// How long a print of `gcode` runs for: the sum of its dwells, in seconds.
///
/// The hold print moves nothing and heats nothing, so its dwells are its whole
/// running time, on the virtual printer and on a real one alike. Read from the
/// file rather than from the machine's own estimate: `OctoPrint` estimates
/// what is left from how far through the file's *bytes* the print is, and
/// most of this file's bytes are its comment header, so that estimate reads
/// as seconds from the end the moment a print of it starts.
pub fn dwell_seconds(gcode: &str) -> i64 {
    gcode
        .lines()
        .map(|line| line.split(';').next().unwrap_or_default().trim())
        .filter(|command| command.starts_with("G4 ") || *command == "G4")
        .flat_map(|command| command.split_whitespace().skip(1))
        .filter_map(|word| {
            let (unit, amount) = word.split_at(1);
            let amount: i64 = amount.parse().ok()?;
            match unit {
                "S" => Some(amount),
                "P" => Some(amount / 1000),
                _ => None,
            }
        })
        .sum()
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

#[cfg(test)]
mod tests {
    use super::{HOLD_PRINT_SOURCE, dwell_seconds, workspace_root};

    #[test]
    fn a_print_runs_for_the_sum_of_its_dwells_in_either_unit() {
        let gcode = "M117 hold ; G4 S99 in a comment is no dwell\nG21\nG4 S10\nG4 P2500 ; ms\nG4 S10\nG1 X10 F600\nM84\n";

        assert_eq!(dwell_seconds(gcode), 22);
    }

    #[test]
    fn a_print_with_no_dwell_runs_for_no_time() {
        assert_eq!(dwell_seconds("G28\nG1 X10\n"), 0);
    }

    #[test]
    fn the_committed_hold_print_dwells_for_longer_than_the_bring_up_promises() {
        // `octoprint_env.py`'s HOLD_SECONDS is the minimum the bring-up states
        // the print keeps running for after it answers; the file's dwells are
        // what make that promise true, and the margin lives in the file.
        let hold = std::fs::read_to_string(workspace_root().join(HOLD_PRINT_SOURCE))
            .expect("the committed hold print reads");

        assert!(
            dwell_seconds(&hold) >= 2 * 150,
            "the hold print dwells for {}s",
            dwell_seconds(&hold)
        );
    }
}
