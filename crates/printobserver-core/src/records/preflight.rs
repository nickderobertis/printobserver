//! Whether the spools loaded hold enough for a print, were sliced for, and
//! suit the sheet it goes down on, asked before it starts.

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{FileName, Timestamp};

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

/// A name that is never blank, trimmed of surrounding space as it is read.
fn named_text(text: &str, what: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(format!("{what} is named by something other than space"));
    }
    Ok(trimmed.to_owned())
}

/// A filament material as a spool's registration or a slicer's settings name
/// it, such as `PLA`: trimmed, and never blank. On the wire it is a string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(
    crate = "printobserver_types::serde",
    try_from = "String",
    into = "String"
)]
pub struct MaterialName(String);

impl JsonSchema for MaterialName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("MaterialName")
    }

    fn json_schema(
        _generator: &mut printobserver_types::schemars::SchemaGenerator,
    ) -> printobserver_types::schemars::Schema {
        printobserver_types::schemars::json_schema!({
            "type": "string",
            "title": "MaterialName",
            "description": "A filament material as a spool's registration or a slicer's settings name it, such as `PLA`: never blank, and trimmed of surrounding space as it is read.",
            "minLength": 1,
            "pattern": "\\S"
        })
    }
}

impl MaterialName {
    /// The name, as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether two names name one material: without regard to case.
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        self.0.to_lowercase() == other.0.to_lowercase()
    }
}

impl TryFrom<String> for MaterialName {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        named_text(&text, "a material").map(Self)
    }
}

impl From<MaterialName> for String {
    fn from(name: MaterialName) -> Self {
        name.0
    }
}

impl core::fmt::Display for MaterialName {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A build sheet's identifier as the sheet table names it, such as
/// `smooth-pei`: trimmed, and never blank. On the wire it is a string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(
    crate = "printobserver_types::serde",
    try_from = "String",
    into = "String"
)]
pub struct SheetId(String);

impl JsonSchema for SheetId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("SheetId")
    }

    fn json_schema(
        _generator: &mut printobserver_types::schemars::SchemaGenerator,
    ) -> printobserver_types::schemars::Schema {
        printobserver_types::schemars::json_schema!({
            "type": "string",
            "title": "SheetId",
            "description": "A build sheet's identifier as the sheet table names it, such as `smooth-pei`: never blank, and trimmed of surrounding space as it is read.",
            "minLength": 1,
            "pattern": "\\S"
        })
    }
}

impl SheetId {
    /// The identifier, as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether two identifiers name one sheet: without regard to case.
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        self.0.to_lowercase() == other.0.to_lowercase()
    }
}

impl TryFrom<String> for SheetId {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        named_text(&text, "a sheet").map(Self)
    }
}

impl From<SheetId> for String {
    fn from(sheet: SheetId) -> Self {
        sheet.0
    }
}

impl core::fmt::Display for SheetId {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.0)
    }
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
    pub loaded: MaterialName,
    /// The material the file's own `filament_type` names for the tool.
    pub sliced_for: MaterialName,
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
    pub loaded: MaterialName,
    /// The file that was to be compared.
    pub file_name: FileName,
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
        loaded: MaterialName,
        /// The file whose settings could not be read.
        file_name: FileName,
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
    pub sheet: SheetId,
    /// The material of the spool loaded on tool 0.
    pub material: MaterialName,
    /// Every sheet the table allows that material.
    pub allowed: Vec<SheetId>,
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
        sheet: SheetId,
        /// The material of the spool loaded on tool 0.
        material: MaterialName,
    },
    /// No entry of the table lists the sheet.
    UnknownSheet {
        /// The build sheet the manifest declares.
        sheet: SheetId,
        /// The material of the spool loaded on tool 0.
        material: MaterialName,
        /// Every sheet some entry lists.
        known: Vec<SheetId>,
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
