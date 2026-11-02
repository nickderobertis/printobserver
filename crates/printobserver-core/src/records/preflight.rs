//! Whether the spools loaded hold enough for a print, were sliced for, and
//! suit the sheet it goes down on, asked before it starts.

use printobserver_types::Timestamp;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

use super::ids::SpoolId;
use super::spool::{UnverifiedMark, UnverifiedMarks};

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
    pub marks: UnverifiedMarks,
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
    ///
    /// The filament weight's alone: the material and sheet checks carry
    /// verdicts of their own, in `material` and `sheet`.
    pub verdict: PreflightVerdict,
    /// Whether the file was sliced for the material of the spool on tool 0.
    /// Absent from a check recorded before this check existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<MaterialResult>,
    /// Whether the declared build sheet suits that spool's material. Absent
    /// from a check recorded before this check existed, and from a read that
    /// was given no sheet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<SheetResult>,
    /// When the check was made.
    pub checked_at: Timestamp,
}

/// What the material check compared: the spool loaded on a tool and the
/// material the file says that tool was sliced for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct MaterialComparison {
    /// The tool, in the printer's own numbering.
    pub tool: i64,
    /// The spool loaded on it.
    pub spool: PreflightSpool,
    /// That spool's material.
    pub loaded: String,
    /// The material the file's own `filament_type` names for the tool.
    pub sliced_for: String,
}

/// A spool whose material the file could not be compared with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct UncomparedMaterial {
    /// The tool, in the printer's own numbering.
    pub tool: i64,
    /// The spool loaded on it.
    pub spool: PreflightSpool,
    /// That spool's material.
    pub loaded: String,
    /// The file that was to be compared.
    pub file_name: String,
}

/// Why the material check objects to a start, with what it compared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "cause",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum MaterialObjection {
    /// The file was sliced for another material than the loaded spool's.
    Mismatch(MaterialComparison),
    /// The file names no `filament_type`, so what it was sliced for is
    /// unknown.
    Undeclared(UncomparedMaterial),
    /// The file's settings could not be read from the printer.
    Unread {
        /// The tool, in the printer's own numbering.
        tool: i64,
        /// The spool loaded on it.
        spool: PreflightSpool,
        /// That spool's material.
        loaded: String,
        /// The file whose settings could not be read.
        file_name: String,
        /// What the printer answered instead, in its own words.
        detail: String,
    },
}

/// Whether a file was sliced for the material of the spool loaded on tool 0,
/// tagged by its verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "verdict",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum MaterialResult {
    /// The file was sliced for the loaded spool's material.
    Passed(MaterialComparison),
    /// No tracked spool is loaded on tool 0, or the check is off, so nothing
    /// was compared.
    Unchecked,
    /// The check refuses the start.
    Refused {
        /// What it objected to.
        objection: MaterialObjection,
    },
    /// The start went ahead despite this objection, because an operator said
    /// so.
    Overridden {
        /// What it objected to.
        objection: MaterialObjection,
    },
    /// The check is set to warn, and let the start go ahead despite this
    /// objection.
    Warned {
        /// What it objected to.
        objection: MaterialObjection,
    },
}

/// What the sheet check compared: the declared sheet, the loaded spool's
/// material, and the sheets the table allows that material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SheetComparison {
    /// The build sheet the manifest declares.
    pub sheet: String,
    /// The material of the spool loaded on tool 0.
    pub material: String,
    /// Every sheet the table allows that material.
    pub allowed: Vec<String>,
}

/// Why the sheet check objects to a start, with what it compared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "cause",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum SheetObjection {
    /// The table lists the material, and not on this sheet.
    NotAllowed(SheetComparison),
    /// The table has no entry for the material.
    UnlistedMaterial {
        /// The build sheet the manifest declares.
        sheet: String,
        /// The material of the spool loaded on tool 0.
        material: String,
    },
    /// No entry of the table lists the sheet.
    UnknownSheet {
        /// The build sheet the manifest declares.
        sheet: String,
        /// The material of the spool loaded on tool 0.
        material: String,
        /// Every sheet some entry lists.
        known: Vec<String>,
    },
}

/// Whether the declared build sheet suits the loaded spool's material, tagged
/// by its verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "verdict",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum SheetResult {
    /// The table allows the material on the declared sheet.
    Passed(SheetComparison),
    /// No tracked spool is loaded on tool 0, or the check is off, so nothing
    /// was compared.
    Unchecked,
    /// The manifest declares no sheet. This refuses every start, and no
    /// override lets it through.
    Undeclared,
    /// The check refuses the start.
    Refused {
        /// What it objected to.
        objection: SheetObjection,
    },
    /// The start went ahead despite this objection, because an operator said
    /// so.
    Overridden {
        /// What it objected to.
        objection: SheetObjection,
    },
    /// The check is set to warn, and let the start go ahead despite this
    /// objection.
    Warned {
        /// What it objected to.
        objection: SheetObjection,
    },
}
