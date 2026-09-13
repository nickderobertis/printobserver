//! The plausibility ranges the printer port declares for what a printer reports.
//!
//! A numeric field of a snapshot this port answers is typed
//! [`Reported<f64>`](printobserver_types::Reported), carrying whether the value
//! lies outside the range declared here for it. **These are validity ranges on
//! what a printer can plausibly report, and they are not the operator's safety
//! envelope.** A reading outside one of them says the source reported something
//! implausible — a disconnected thermistor, a firmware that answers in another
//! unit. What an actor is *allowed to ask for* is
//! the supervision domain's `SafetyEnvelope`, which is server
//! configuration and is narrower by orders of magnitude; nothing reads these
//! ranges as a bound on an action, and the two are never intersected, compared
//! or substituted for one another.
//!
//! [`RANGED_FIELDS`] is the one table of which field carries which range, read
//! by the tests that drive every ranged field at its boundary.

use printobserver_types::Range;

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

/// One field this port declares a plausibility range for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangedField {
    /// The type the field belongs to.
    pub type_name: &'static str,
    /// The field's own name.
    pub field: &'static str,
    /// The plausibility range declared for it.
    pub range: Range,
}

/// Every field this port types as `Reported<f64>`, with its declared range.
///
/// These are validity ranges on what a printer can plausibly report. They are
/// not the operator's safety envelope, and nothing intersects, compares or
/// substitutes the two.
pub const RANGED_FIELDS: [RangedField; 7] = [
    RangedField {
        type_name: "PrinterSnapshot",
        field: "feedrate_factor",
        range: FEEDRATE_FACTOR_RANGE,
    },
    RangedField {
        type_name: "PrinterSnapshot",
        field: "flowrate_factor",
        range: FLOWRATE_FACTOR_RANGE,
    },
    RangedField {
        type_name: "PrinterSnapshot",
        field: "fan_percent",
        range: FAN_PERCENT_RANGE,
    },
    RangedField {
        type_name: "JobSnapshot",
        field: "completion",
        range: COMPLETION_RANGE,
    },
    RangedField {
        type_name: "HeaterSnapshot",
        field: "actual_c",
        range: HEATER_ACTUAL_C_RANGE,
    },
    RangedField {
        type_name: "HeaterSnapshot",
        field: "target_c",
        range: HEATER_TARGET_C_RANGE,
    },
    RangedField {
        type_name: "HeaterSnapshot",
        field: "offset_c",
        range: HEATER_OFFSET_C_RANGE,
    },
];
