//! Reported numbers, and the plausibility ranges this crate declares for them.
//!
//! A numeric field this crate declares a range for is typed [`Reported<f64>`],
//! which serializes as an object carrying a `value` and a boolean
//! `out_of_range` that is true exactly when the reported `value` is non-finite
//! or lies outside the range this crate declares for that field. A value
//! outside the range is carried through as reported with that flag set, never
//! coerced, never clamped and never dropped.
//!
//! The non-finite half of that definition is deliberate, because `f64` admits
//! inhabitants an ordinary range comparison answers wrongly. `NaN` compares
//! neither below a minimum nor above a maximum, so `value < min || value > max`
//! would report the most implausible reading this type can hold as a plausible
//! one; the two infinities lie outside every finite range. All three are what a
//! disconnected thermistor or a division by a zero-valued reading produces,
//! which is exactly the class the flag exists to carry, so all three are
//! `out_of_range`.
//!
//! On the wire the type is closed instead: the JSON number grammar denotes no
//! non-finite value, so a `Reported<f64>` never arrives by parse carrying one,
//! and this crate refuses a non-finite `value` on emission rather than taking
//! the serializer's own default of emitting `null` — which would parse back as
//! a field the source did not report and turn an implausible reading into no
//! reading at all.
//!
//! **These are validity ranges on what a printer can plausibly report, and they
//! are not the operator's safety envelope.** A reading outside one of them says
//! the source reported something implausible — a disconnected thermistor, a
//! firmware that answers in another unit. What an actor is *allowed to ask for*
//! is [`SafetyEnvelope`](crate::SafetyEnvelope), which is server configuration
//! and is narrower by orders of magnitude; nothing in this crate reads these
//! ranges as a bound on an action, and the two are never intersected, compared
//! or substituted for one another.

use core::fmt;
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::ser::SerializeStruct as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _, ser::Error as _};

/// An inclusive pair of 64-bit floats.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Range {
    /// The lowest value the range admits, inclusive.
    pub min: f64,
    /// The highest value the range admits, inclusive.
    pub max: f64,
}

impl Range {
    /// The range from `min` to `max`, both inclusive.
    #[must_use]
    pub const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    /// Whether `value` lies inside this range.
    ///
    /// A non-finite value lies inside no range: `NaN` compares neither below
    /// the minimum nor above the maximum, and the two infinities lie outside
    /// every finite range.
    #[must_use]
    pub fn contains(&self, value: f64) -> bool {
        value.is_finite() && value >= self.min && value <= self.max
    }
}

/// The plausibility range of `PrinterSnapshot::feedrate_factor`.
///
/// A multiplier where one means one hundred percent, spanning what the
/// firmware's own speed command accepts. A validity range on what a printer can
/// plausibly report, not a bound on what an actor may ask for.
pub const FEEDRATE_FACTOR_RANGE: Range = Range::new(0.1, 10.0);

/// The plausibility range of `PrinterSnapshot::flowrate_factor`.
///
/// A multiplier where one means one hundred percent, spanning what the
/// firmware's own flow command accepts. A validity range on what a printer can
/// plausibly report, not a bound on what an actor may ask for.
pub const FLOWRATE_FACTOR_RANGE: Range = Range::new(0.1, 10.0);

/// The plausibility range of `PrinterSnapshot::fan_percent`, in percent.
///
/// A validity range on what a printer can plausibly report, not a bound on what
/// an actor may ask for.
pub const FAN_PERCENT_RANGE: Range = Range::new(0.0, 100.0);

/// The plausibility range of `JobSnapshot::completion`, as a fraction.
///
/// A validity range on what a printer can plausibly report, not a bound on what
/// an actor may ask for.
pub const COMPLETION_RANGE: Range = Range::new(0.0, 1.0);

/// The plausibility range of `HeaterSnapshot::actual_c`, in degrees Celsius.
///
/// A validity range on what a printer can plausibly report, not a bound on what
/// an actor may ask for.
pub const HEATER_ACTUAL_C_RANGE: Range = Range::new(-20.0, 500.0);

/// The plausibility range of `HeaterSnapshot::target_c`, in degrees Celsius.
///
/// A validity range on what a printer can plausibly report, not a bound on what
/// an actor may ask for.
pub const HEATER_TARGET_C_RANGE: Range = Range::new(0.0, 500.0);

/// The plausibility range of `HeaterSnapshot::offset_c`, in degrees Celsius.
///
/// A validity range on what a printer can plausibly report, not a bound on what
/// an actor may ask for.
pub const HEATER_OFFSET_C_RANGE: Range = Range::new(-50.0, 50.0);

/// A value a source reported, carrying whether it is outside the range this
/// crate declares for the field it was reported in.
///
/// The value is carried exactly as reported: it is never coerced, clamped or
/// dropped, whatever the flag says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reported<T> {
    /// The value exactly as the source reported it.
    value: T,
    /// Whether the value is non-finite or outside the declared range.
    out_of_range: bool,
}

impl Reported<f64> {
    /// The value as reported, flagged against the range declared for its field.
    #[must_use]
    pub fn new(value: f64, range: Range) -> Self {
        Self {
            value,
            out_of_range: !range.contains(value),
        }
    }

    /// The value exactly as the source reported it.
    #[must_use]
    pub const fn value(&self) -> f64 {
        self.value
    }

    /// Whether the value is non-finite or outside its field's declared range.
    #[must_use]
    pub const fn out_of_range(&self) -> bool {
        self.out_of_range
    }
}

/// The wire shape of a [`Reported<f64>`]: the pair, and nothing else.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportedWire {
    value: f64,
    out_of_range: bool,
}

impl Serialize for Reported<f64> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !self.value.is_finite() {
            return Err(S::Error::custom(format!(
                "a non-finite reported value ({}) has no JSON number spelling, and emitting it \
                 as `null` would parse back as a field the source did not report",
                self.value
            )));
        }
        let mut state = serializer.serialize_struct("Reported", 2)?;
        state.serialize_field("value", &self.value)?;
        state.serialize_field("out_of_range", &self.out_of_range)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Reported<f64> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ReportedWire::deserialize(deserializer)?;
        Ok(Self {
            value: wire.value,
            out_of_range: wire.out_of_range,
        })
    }
}

impl JsonSchema for Reported<f64> {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Reported")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "title": "Reported",
            "description": "A value as a source reported it, flagged when it is outside the \
                            plausibility range this crate declares for its field.",
            "properties": {
                "value": { "type": "number" },
                "out_of_range": { "type": "boolean" }
            },
            "required": ["value", "out_of_range"],
            "additionalProperties": false
        })
    }
}

impl fmt::Display for Reported<f64> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.out_of_range {
            write!(formatter, "{} (out of range)", self.value)
        } else {
            write!(formatter, "{}", self.value)
        }
    }
}

/// Read an optional reported value, refusing a flag that disagrees with the
/// value under the range declared for the field it was read in.
fn read_ranged<'de, D: Deserializer<'de>>(
    deserializer: D,
    field: &str,
    range: Range,
) -> Result<Option<Reported<f64>>, D::Error> {
    let Some(reported) = Option::<Reported<f64>>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let expected = !range.contains(reported.value);
    if reported.out_of_range != expected {
        return Err(D::Error::custom(format!(
            "`{field}` reported value {} with out_of_range {}, but the declared range \
             {} to {} makes it {expected}",
            reported.value, reported.out_of_range, range.min, range.max
        )));
    }
    Ok(Some(reported))
}

/// Read `PrinterSnapshot::feedrate_factor` against its declared range.
pub(crate) fn de_feedrate_factor<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    read_ranged(deserializer, "feedrate_factor", FEEDRATE_FACTOR_RANGE)
}

/// Read `PrinterSnapshot::flowrate_factor` against its declared range.
pub(crate) fn de_flowrate_factor<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    read_ranged(deserializer, "flowrate_factor", FLOWRATE_FACTOR_RANGE)
}

/// Read `PrinterSnapshot::fan_percent` against its declared range.
pub(crate) fn de_fan_percent<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    read_ranged(deserializer, "fan_percent", FAN_PERCENT_RANGE)
}

/// Read `JobSnapshot::completion` against its declared range.
pub(crate) fn de_completion<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    read_ranged(deserializer, "completion", COMPLETION_RANGE)
}

/// Read `HeaterSnapshot::actual_c` against its declared range.
pub(crate) fn de_heater_actual_c<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    read_ranged(deserializer, "actual_c", HEATER_ACTUAL_C_RANGE)
}

/// Read `HeaterSnapshot::target_c` against its declared range.
pub(crate) fn de_heater_target_c<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    read_ranged(deserializer, "target_c", HEATER_TARGET_C_RANGE)
}

/// Read `HeaterSnapshot::offset_c` against its declared range.
pub(crate) fn de_heater_offset_c<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Reported<f64>>, D::Error> {
    read_ranged(deserializer, "offset_c", HEATER_OFFSET_C_RANGE)
}
