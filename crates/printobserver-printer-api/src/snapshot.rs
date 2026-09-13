//! Normalized observations of a printer and of the job it is running.
//!
//! These are what [`PrinterPort::snapshot`](crate::PrinterPort::snapshot) and
//! [`PrinterPort::job`](crate::PrinterPort::job) answer, declared beside the
//! port whose methods carry them. Every numeric field a printer can report
//! implausibly is typed [`Reported<f64>`] and read against the plausibility
//! range [`ranges`](crate::ranges) declares for it: a flag that disagrees with
//! the value under that range is refused on parse, so a snapshot never carries
//! a reading whose flag was written by something other than its range.

use printobserver_types::contract::Sample;
use printobserver_types::reported::deserialize_ranged;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Deserializer, Serialize};
use printobserver_types::{Reported, Timestamp};

use crate::state::PrinterState;

use crate::ranges::{
    COMPLETION_RANGE, FAN_PERCENT_RANGE, FEEDRATE_FACTOR_RANGE, FLOWRATE_FACTOR_RANGE,
    HEATER_ACTUAL_C_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE,
};

/// One heater, as a source reported it.
///
/// Every field is optional, because a source that reports no heater at all
/// reports none of these; each is a plausibility-ranged reported value in
/// degrees Celsius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct HeaterSnapshot {
    /// The temperature the heater is at.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_heater_actual_c"
    )]
    pub actual_c: Option<Reported<f64>>,
    /// The temperature the heater is driving towards.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_heater_target_c"
    )]
    pub target_c: Option<Reported<f64>>,
    /// The offset applied to this heater's target.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_heater_offset_c"
    )]
    pub offset_c: Option<Reported<f64>>,
}

/// A printer, as a source reported it at one instant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
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
        deserialize_with = "de_feedrate_factor"
    )]
    pub feedrate_factor: Option<Reported<f64>>,
    /// The flowrate multiplier, where one means one hundred percent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_flowrate_factor"
    )]
    pub flowrate_factor: Option<Reported<f64>>,
    /// The part-cooling fan, in percent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_fan_percent"
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
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
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
        deserialize_with = "de_completion"
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

/// Read `PrinterSnapshot::feedrate_factor` against its declared range.
fn de_feedrate_factor<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    deserialize_ranged(deserializer, "feedrate_factor", FEEDRATE_FACTOR_RANGE)
}

/// Read `PrinterSnapshot::flowrate_factor` against its declared range.
fn de_flowrate_factor<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    deserialize_ranged(deserializer, "flowrate_factor", FLOWRATE_FACTOR_RANGE)
}

/// Read `PrinterSnapshot::fan_percent` against its declared range.
fn de_fan_percent<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    deserialize_ranged(deserializer, "fan_percent", FAN_PERCENT_RANGE)
}

/// Read `JobSnapshot::completion` against its declared range.
fn de_completion<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    deserialize_ranged(deserializer, "completion", COMPLETION_RANGE)
}

/// Read `HeaterSnapshot::actual_c` against its declared range.
fn de_heater_actual_c<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    deserialize_ranged(deserializer, "actual_c", HEATER_ACTUAL_C_RANGE)
}

/// Read `HeaterSnapshot::target_c` against its declared range.
fn de_heater_target_c<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    deserialize_ranged(deserializer, "target_c", HEATER_TARGET_C_RANGE)
}

/// Read `HeaterSnapshot::offset_c` against its declared range.
fn de_heater_offset_c<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    deserialize_ranged(deserializer, "offset_c", HEATER_OFFSET_C_RANGE)
}

impl Sample for HeaterSnapshot {
    fn sample_full() -> Self {
        Self {
            actual_c: Some(Reported::new(214.5, HEATER_ACTUAL_C_RANGE)),
            target_c: Some(Reported::new(215.0, HEATER_TARGET_C_RANGE)),
            offset_c: Some(Reported::new(0.0, HEATER_OFFSET_C_RANGE)),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            actual_c: None,
            target_c: None,
            offset_c: None,
        }
    }
}

impl Sample for PrinterSnapshot {
    fn sample_full() -> Self {
        Self {
            connection: PrinterState::Printing,
            tools: vec![HeaterSnapshot::sample_full()],
            bed: Some(HeaterSnapshot::sample_full()),
            chamber: Some(HeaterSnapshot::sample_full()),
            feedrate_factor: Some(Reported::new(1.0, FEEDRATE_FACTOR_RANGE)),
            flowrate_factor: Some(Reported::new(1.0, FLOWRATE_FACTOR_RANGE)),
            fan_percent: Some(Reported::new(100.0, FAN_PERCENT_RANGE)),
            observed_at: Timestamp::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            connection: PrinterState::Printing,
            tools: vec![],
            bed: None,
            chamber: None,
            feedrate_factor: None,
            flowrate_factor: None,
            fan_percent: None,
            observed_at: Timestamp::sample_full(),
        }
    }
}

impl Sample for JobSnapshot {
    fn sample_full() -> Self {
        Self {
            file_name: Some("benchy.gcode".to_owned()),
            file_origin: Some("local".to_owned()),
            size_bytes: Some(4_194_304),
            estimated_print_time_s: Some(7200),
            completion: Some(Reported::new(0.42, COMPLETION_RANGE)),
            print_time_s: Some(3024),
            print_time_left_s: Some(4176),
            state: PrinterState::Printing,
            error: Some("none".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            file_name: None,
            file_origin: None,
            size_bytes: None,
            estimated_print_time_s: None,
            completion: None,
            print_time_s: None,
            print_time_left_s: None,
            state: PrinterState::Printing,
            error: None,
        }
    }
}
