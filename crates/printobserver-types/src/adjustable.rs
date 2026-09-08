//! The closed set of things an adjustment may change.

use core::fmt;
use core::str::FromStr;
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// The prefix the tool-target adjustable spells its tool number after.
const TOOL_TARGET: &str = "tool_target";

/// One thing an adjustment may change.
///
/// Serialized as a string rather than an object, because this is the key type
/// of the manifest's, the envelope's and the effective bounds' maps, and a JSON
/// object key is a string. The spellings are `feedrate`, `flowrate`,
/// `bed_target`, `fan`, and `tool_target:<number>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Adjustable {
    /// The feedrate factor.
    Feedrate,
    /// The flowrate factor.
    Flowrate,
    /// One tool's target temperature, carrying that tool's number.
    ToolTarget {
        /// The tool number, in the printer's own numbering.
        tool: i64,
    },
    /// The bed's target temperature.
    BedTarget,
    /// The part-cooling fan.
    Fan,
}

/// Why a string is no adjustable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustableError {
    /// What is wrong with the string.
    detail: String,
}

impl AdjustableError {
    /// What is wrong with the string.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for AdjustableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "not an adjustable: {}", self.detail)
    }
}

impl core::error::Error for AdjustableError {}

impl fmt::Display for Adjustable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Feedrate => formatter.write_str("feedrate"),
            Self::Flowrate => formatter.write_str("flowrate"),
            Self::ToolTarget { tool } => write!(formatter, "{TOOL_TARGET}:{tool}"),
            Self::BedTarget => formatter.write_str("bed_target"),
            Self::Fan => formatter.write_str("fan"),
        }
    }
}

impl FromStr for Adjustable {
    type Err = AdjustableError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "feedrate" => return Ok(Self::Feedrate),
            "flowrate" => return Ok(Self::Flowrate),
            "bed_target" => return Ok(Self::BedTarget),
            "fan" => return Ok(Self::Fan),
            _ => {}
        }
        let Some(number) = text.strip_prefix(concat!("tool_target", ":")) else {
            return Err(AdjustableError {
                detail: format!("{text:?} names no adjustable"),
            });
        };
        number
            .parse()
            .map(|tool| Self::ToolTarget { tool })
            .map_err(|error| AdjustableError {
                detail: format!("{text:?} carries no tool number: {error}"),
            })
    }
}

impl Serialize for Adjustable {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Adjustable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

impl JsonSchema for Adjustable {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Adjustable")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "title": "Adjustable",
            "description": "One thing an adjustment may change.",
            "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$"
        })
    }
}
