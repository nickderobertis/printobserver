//! Whether the spools loaded hold enough for a print, asked before it starts.

use printobserver_types::Timestamp;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

use super::ids::SpoolId;
use super::spool::UnverifiedMark;

/// What a filament check found, for one tool or for a whole print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PreflightVerdict {
    /// The spool holds what the print needs and its buffer.
    Passed,
    /// No tracked spool is loaded on the tool, so nothing was checked.
    Unchecked,
    /// The spool holds less than the print needs and its buffer.
    Insufficient,
    /// The spool's figure is in doubt.
    UnverifiedSpool,
    /// `OctoPrint`'s analysis of the file had still not appeared when the
    /// start's wait for it elapsed, or has not appeared yet when the check is
    /// only read.
    NoAnalysis,
    /// The start went ahead despite the check, because the actor said so.
    Overridden,
    /// The check is set to warn, and let an insufficient, unverified or
    /// unanalysed start go ahead.
    Warned,
}

/// What a filament check found for one tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PreflightTool {
    /// The tool, in the printer's own numbering.
    pub tool: i64,
    /// The spool loaded on it, when one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spool_id: Option<SpoolId>,
    /// That spool's name, beside its identifier, so a reader need not look it
    /// up.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spool_name: Option<String>,
    /// What the print needs from it, in grams, when the file's analysis says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_g: Option<f64>,
    /// The margin asked for beyond that, in grams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_g: Option<f64>,
    /// What the spool holds, in grams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_g: Option<f64>,
    /// The loaded spool's marks at the check, oldest first, whatever the
    /// verdict, so a spool both short and unverified says both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<UnverifiedMark>,
    /// What the check found for this tool.
    pub verdict: PreflightVerdict,
}

/// What a filament check found for one print.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct Preflight {
    /// The file the print is of.
    pub file_name: String,
    /// What it found for each tool.
    pub tools: Vec<PreflightTool>,
    /// The worst of the tools' verdicts, in the order insufficient,
    /// unverified spool, no analysis, warned, overridden, unchecked, passed.
    pub verdict: PreflightVerdict,
    /// When the check was made.
    pub checked_at: Timestamp,
}
