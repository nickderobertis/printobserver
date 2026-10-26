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
    /// What the check found for it, with the evidence that finding rests on.
    pub result: PreflightToolResult,
}

/// The spool a check judged, named beside its identifier so a reader need
/// not look it up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PreflightSpool {
    /// The spool's identifier.
    pub id: SpoolId,
    /// The spool's name.
    pub name: String,
}

/// What a print needs from one spool, in grams: both figures a comparison
/// with what the spool holds uses, or neither.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PreflightNeed {
    /// What the print needs from the spool, by the file's analysis and the
    /// spool's own diameter and density.
    pub required_g: f64,
    /// The margin asked for beyond that.
    pub buffer_g: f64,
}

/// A spool that holds less than the print needs and its buffer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ShortSpool {
    /// The spool.
    pub spool: PreflightSpool,
    /// What the spool holds, in grams.
    pub remaining_g: f64,
    /// What the print needs from it.
    pub need: PreflightNeed,
    /// The spool's marks at the check, oldest first, so a spool both short
    /// and unverified says both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<UnverifiedMark>,
}

/// A spool whose figure is in doubt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct DoubtedSpool {
    /// The spool.
    pub spool: PreflightSpool,
    /// What the spool holds, in grams.
    pub remaining_g: f64,
    /// What the print needs from it, when the file's analysis says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub need: Option<PreflightNeed>,
    /// Every reason the spool's figure is in doubt, oldest first; never empty.
    #[schemars(length(min = 1))]
    pub marks: Vec<UnverifiedMark>,
}

/// A spool on a tool of a file with no analysis, so what it needs is unknown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct UnanalysedSpool {
    /// The spool.
    pub spool: PreflightSpool,
    /// What the spool holds, in grams.
    pub remaining_g: f64,
}

/// What a check that refuses a start found for one tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "verdict",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PreflightObjection {
    /// The spool holds less than the print needs and its buffer.
    Insufficient(ShortSpool),
    /// The spool's figure is in doubt.
    UnverifiedSpool(DoubtedSpool),
    /// The file has no analysis, so what the spool must hold is unknown.
    NoAnalysis(UnanalysedSpool),
}

impl PreflightObjection {
    /// The verdict this objection is.
    #[must_use]
    pub const fn verdict(&self) -> PreflightVerdict {
        match self {
            Self::Insufficient(_) => PreflightVerdict::Insufficient,
            Self::UnverifiedSpool(_) => PreflightVerdict::UnverifiedSpool,
            Self::NoAnalysis(_) => PreflightVerdict::NoAnalysis,
        }
    }
}

/// What a filament check found for one tool, tagged by its verdict, each
/// verdict carrying exactly the evidence it rests on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "verdict",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PreflightToolResult {
    /// The spool holds what the print needs and its buffer, and its figure is
    /// not in doubt.
    Passed {
        /// The spool.
        spool: PreflightSpool,
        /// What the spool holds, in grams.
        remaining_g: f64,
        /// What the print needs from it.
        need: PreflightNeed,
    },
    /// No tracked spool is loaded on the tool, so nothing was checked.
    Unchecked,
    /// The spool holds less than the print needs and its buffer.
    Insufficient(ShortSpool),
    /// The spool's figure is in doubt.
    UnverifiedSpool(DoubtedSpool),
    /// `OctoPrint`'s analysis of the file had not appeared, so what the spool
    /// must hold is unknown.
    NoAnalysis(UnanalysedSpool),
    /// The start went ahead despite this objection, because an operator said
    /// so.
    Overridden {
        /// What the check objected to.
        objection: PreflightObjection,
    },
    /// The check is set to warn, and let the start go ahead despite this
    /// objection.
    Warned {
        /// What the check objected to.
        objection: PreflightObjection,
    },
}

impl PreflightToolResult {
    /// The verdict this result is.
    #[must_use]
    pub const fn verdict(&self) -> PreflightVerdict {
        match self {
            Self::Passed { .. } => PreflightVerdict::Passed,
            Self::Unchecked => PreflightVerdict::Unchecked,
            Self::Insufficient(_) => PreflightVerdict::Insufficient,
            Self::UnverifiedSpool(_) => PreflightVerdict::UnverifiedSpool,
            Self::NoAnalysis(_) => PreflightVerdict::NoAnalysis,
            Self::Overridden { .. } => PreflightVerdict::Overridden,
            Self::Warned { .. } => PreflightVerdict::Warned,
        }
    }
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
