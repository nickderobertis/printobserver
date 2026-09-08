//! Normalized observations of a printer and of the job it is running.

use serde::{Deserialize, Serialize};

use schemars::JsonSchema;

use crate::reported::Reported;
use crate::timestamp::Timestamp;

/// The state a source reports a printer or a print to be in.
///
/// The `unknown` arm exists so that a state nobody anticipated is recorded
/// carrying the source's own word for it rather than lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PrinterState {
    /// Connected and idle.
    Operational,
    /// Printing, but paused.
    Paused,
    /// Printing.
    Printing,
    /// Cancelling a print.
    Cancelling,
    /// In an error state.
    Error,
    /// Not reachable.
    Offline,
    /// A state this vocabulary does not name, in the source's own word for it.
    Unknown(String),
}

/// One heater, as a source reported it.
///
/// Every field is optional, because a source that reports no heater at all
/// reports none of these; each is a plausibility-ranged reported value in
/// degrees Celsius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct HeaterSnapshot {
    /// The temperature the heater is at.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::reported::de_heater_actual_c"
    )]
    pub actual_c: Option<Reported<f64>>,
    /// The temperature the heater is driving towards.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::reported::de_heater_target_c"
    )]
    pub target_c: Option<Reported<f64>>,
    /// The offset applied to this heater's target.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::reported::de_heater_offset_c"
    )]
    pub offset_c: Option<Reported<f64>>,
}

/// A printer, as a source reported it at one instant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrinterSnapshot {
    /// The state the printer is in.
    pub connection: PrinterState,
    /// The tool heaters, indexed by tool number.
    pub tools: Vec<HeaterSnapshot>,
    /// The bed heater, when the printer reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed: Option<HeaterSnapshot>,
    /// The chamber heater, when the printer reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamber: Option<HeaterSnapshot>,
    /// The feedrate multiplier, where one means one hundred percent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::reported::de_feedrate_factor"
    )]
    pub feedrate_factor: Option<Reported<f64>>,
    /// The flowrate multiplier, where one means one hundred percent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::reported::de_flowrate_factor"
    )]
    pub flowrate_factor: Option<Reported<f64>>,
    /// The part-cooling fan, in percent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::reported::de_fan_percent"
    )]
    pub fan_percent: Option<Reported<f64>>,
    /// The instant this observation was taken.
    pub observed_at: Timestamp,
}

/// The job a printer reports it is running.
///
/// Every field but `state` is optional: an absent one means the source did not
/// report it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobSnapshot {
    /// The name of the file being printed, as the source reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Where the file lives, in the source's own vocabulary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_origin: Option<String>,
    /// The file's size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    /// The whole print's estimated duration, in whole seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_print_time_s: Option<i64>,
    /// How far through the print is, as a fraction from zero to one.
    ///
    /// Normalized to a fraction here regardless of how the source expresses it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::reported::de_completion"
    )]
    pub completion: Option<Reported<f64>>,
    /// How long the print has been running, in whole seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_time_s: Option<i64>,
    /// How long the print has left, in whole seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_time_left_s: Option<i64>,
    /// The state the source reports the job to be in.
    pub state: PrinterState,
    /// The error the source reports, when it reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
