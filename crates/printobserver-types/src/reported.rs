//! Reported numbers, and the rule a plausibility range flags them under.
//!
//! A numeric field a domain declares a range for is typed [`Reported<f64>`],
//! which serializes as an object carrying a `value` and a boolean
//! `out_of_range` that is true exactly when the reported `value` is non-finite
//! or lies outside the range declared for that field. A value outside the
//! range is carried through as reported with that flag set, never coerced,
//! never clamped and never dropped. The ranges themselves are the declaring
//! domain's — the printer port's, for what a printer reports — and this module
//! holds the representation and [`deserialize_ranged`], the one reading of a
//! field against its range, rather than any range.
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
//! **A plausibility range is a validity range on what a source can plausibly
//! report, and it is not the operator's safety envelope.** A reading outside
//! one says the source reported something implausible — a disconnected
//! thermistor, a firmware that answers in another unit. What an actor is
//! *allowed to ask for* is [`SafetyEnvelope`](crate::SafetyEnvelope), which is
//! server configuration and is narrower by orders of magnitude; nothing reads
//! a plausibility range as a bound on an action, and the two are never
//! intersected, compared or substituted for one another.

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
///
/// This is what a `deserialize_with` on a ranged field calls, naming the field
/// and the range the declaring domain holds for it, so that a value never
/// arrives carrying a flag some other range wrote.
///
/// # Errors
///
/// Answers the deserializer's own error for a value that is not a reported
/// pair, and a refusal naming the field, the value and the range when the pair
/// carries a flag the range disagrees with.
pub fn deserialize_ranged<'de, D: Deserializer<'de>>(
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
