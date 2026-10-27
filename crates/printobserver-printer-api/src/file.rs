//! What a printer knows of the files it holds, and of the filament they use.
//!
//! These are what the three file reads of
//! [`PrinterPort`](crate::PrinterPort) answer — the facts of one file, the
//! files that carry a print tally, and the filament a file's own moves extrude
//! up to a position in it — declared beside the port whose methods carry them.
//! Every length is the filament fed in, in millimetres, per tool.

use printobserver_types::Timestamp;
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// The filament one tool feeds, as a length and, when it is known, a volume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ToolFilament {
    /// The tool, in the printer's own numbering.
    pub tool: i64,
    /// The length of filament fed, in millimetres.
    pub length_mm: f64,
    /// The volume of filament fed, in cubic centimetres, when the source
    /// reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_cm3: Option<f64>,
}

/// The last time a file was printed, and whether that print completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FilePrint {
    /// When it was printed.
    pub at: Timestamp,
    /// Whether that print completed.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub success: bool,
}

/// One file the printer holds, as its source describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct FileFacts {
    /// The file's name, as the source reported it.
    pub file_name: String,
    /// Where the file lives, in the source's own vocabulary.
    pub origin: String,
    /// The file's size in bytes, when the source reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    /// The filament the source's analysis of the file says each tool uses.
    ///
    /// Absent when the file has no analysis: one still pending, or a file the
    /// source never analyses, such as one on an SD card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filament: Option<Vec<ToolFilament>>,
    /// How many prints of the file completed, by the source's own tally.
    pub successes: i64,
    /// How many prints of the file did not, by the source's own tally.
    pub failures: i64,
    /// The file's most recent print, when it has been printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_print: Option<FilePrint>,
}

impl Sample for ToolFilament {
    fn sample_full() -> Self {
        Self {
            tool: 0,
            length_mm: 1_000.0,
            volume_cm3: Some(2.41),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            volume_cm3: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for FilePrint {
    fn sample_full() -> Self {
        Self {
            at: Timestamp::sample_full(),
            success: true,
        }
    }
}

impl Sample for FileFacts {
    fn sample_full() -> Self {
        Self {
            file_name: "benchy.gcode".to_owned(),
            origin: "local".to_owned(),
            size_bytes: Some(4_194_304),
            filament: Some(vec![ToolFilament::sample_full()]),
            successes: 3,
            failures: 1,
            last_print: Some(FilePrint::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            file_name: "benchy.gcode".to_owned(),
            origin: "sdcard".to_owned(),
            size_bytes: None,
            filament: None,
            successes: 0,
            failures: 0,
            last_print: None,
        }
    }
}

/// The XY rectangle every extruding move of a file lies inside, in
/// millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct Footprint {
    /// The smallest X.
    pub min_x: f64,
    /// The smallest Y.
    pub min_y: f64,
    /// The largest X.
    pub max_x: f64,
    /// The largest Y.
    pub max_y: f64,
}

/// What a file printed up to a byte position in it: the highest Z any
/// extruding move reached, and where in XY it laid material.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PrintedExtent {
    /// The highest Z an extruding move started or ended at, in millimetres.
    pub tallest_mm: f64,
    /// The XY rectangle of every extruding move.
    pub footprint: Footprint,
}

impl Sample for Footprint {
    fn sample_full() -> Self {
        Self {
            min_x: 90.0,
            min_y: 90.0,
            max_x: 110.0,
            max_y: 110.0,
        }
    }

    fn sample_minimal() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        }
    }
}

impl Sample for PrintedExtent {
    fn sample_full() -> Self {
        Self {
            tallest_mm: 6.0,
            footprint: Footprint::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            tallest_mm: 0.2,
            footprint: Footprint::sample_minimal(),
        }
    }
}
