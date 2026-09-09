//! The conversions this adapter owns, and the closed command set the fan uses.
//!
//! Every one of these is a place a reader can get the direction backwards, and
//! a conversion wrong in the same way in both directions round-trips perfectly
//! while commanding the wrong physical thing. So each is stated once, here,
//! with the units on both sides named — and each is pinned by the integration
//! tier against a figure in `OctoPrint`'s own units rather than against a value
//! this crate produced.
//!
//! # Completion
//!
//! `OctoPrint` reports `progress.completion` as a **percentage** — `12.5` is an
//! eighth of the way through. `JobSnapshot::completion` is a **fraction** from
//! zero to one. So the conversion divides by one hundred.
//!
//! # The two factors
//!
//! `PrinterPort::set_feedrate_factor` and its flowrate sibling take a
//! **multiplier**, where one means one hundred percent.
//! `POST /api/printer/printhead` and `POST /api/printer/tool` take a `factor`
//! that `OctoPrint` reads **as a percentage when it is a whole number and as a
//! multiplier when it is a fractional one** — its own `_convert_rate_value`
//! multiplies a float by one hundred and takes an integer as it stands. This
//! adapter sends the whole-number percentage, and it matters that it does: a
//! `1.5` emitted as the JSON number `1.5` would ask for one and a half times,
//! while a `150` emitted as `150.0` would ask for a hundred and fifty times.
//! [`percent_of_multiplier`] is that conversion and it answers an integer, which
//! is what forces the JSON number to be whole.
//!
//! # The fan
//!
//! `OctoPrint` has no fan endpoint at all, so the fan is expressed as G-code
//! through `POST /api/printer/command`. The G-code is `M106`, whose `S`
//! parameter is a **PWM duty from 0 to 255** on Marlin and on the Prusa
//! firmware this targets, while `PrinterPort::set_fan_percent` takes a
//! **percent**. So the conversion scales by 255/100 and rounds; the rounding
//! rule is **half away from zero**, which is a decision this crate makes rather
//! than one a caller discovers, because a percent scale onto 0..=255 lands on a
//! half at every other whole percent — 50 percent is 127.5, and this crate
//! sends 128.
//!
//! Sending the percent verbatim was considered and rejected: `M106 S100` asks a
//! real machine for about two fifths of its fan, which is exactly the confident
//! lie about a machine that this layer exists not to become.

/// The value `M106`'s `S` parameter takes at full fan.
pub const FAN_PWM_FULL_SCALE: f64 = 255.0;

/// The one parameterized command this adapter builds a fan action from.
///
/// `OctoPrint`'s `POST /api/printer/command` substitutes `parameters` into a
/// command by name, so the number and the text are separate all the way to the
/// instance: this string is a constant of this crate and the only thing a
/// caller influences is the number bound to [`FAN_PWM_PARAMETER`].
pub const FAN_SET_COMMAND: &str = "M106 S%(fan_pwm)s";

/// The parameter [`FAN_SET_COMMAND`] carries its number in.
pub const FAN_PWM_PARAMETER: &str = "fan_pwm";

/// The whole set of commands this adapter will ever send a printer.
///
/// It is fixed and it is closed: no method of the printer port takes text, and
/// nothing in this crate composes a command from anything a caller supplied.
/// The set has one member today because one parameterized command spells the
/// whole of the fan's range; a second adjustable that needed G-code would join
/// this array rather than build a string somewhere else.
pub const COMMAND_SET: [&str; 1] = [FAN_SET_COMMAND];

/// The fraction `OctoPrint`'s own percentage denotes.
#[must_use]
pub fn fraction_of_completion(completion: f64) -> f64 {
    completion / 100.0
}

/// The whole-number percentage `OctoPrint` reads a multiplier as.
///
/// A multiplier that denotes no whole percentage — one that is not a number, or
/// one no `i64` holds — becomes zero, which every endpoint that takes a factor
/// refuses: `OctoPrint`'s own accepted bands start at 50 for the feedrate and
/// at 75 for the flowrate. So a value the port's own contract says cannot
/// arrive is refused by the instance rather than rounded into something
/// plausible here.
#[must_use]
pub fn percent_of_multiplier(multiplier: f64) -> i64 {
    whole(multiplier * 100.0)
}

/// The PWM duty `M106`'s `S` parameter takes for a percentage of full fan.
///
/// Clamped to the range the parameter has, because a duty outside it denotes no
/// fan speed at all; a percentage that is not a number becomes zero.
#[must_use]
pub fn fan_pwm_of_percent(percent: f64) -> i64 {
    whole(percent / 100.0 * FAN_PWM_FULL_SCALE).clamp(0, 255)
}

/// The whole count of seconds a duration `OctoPrint` reports in seconds denotes.
///
/// `OctoPrint` reports its estimates as fractional seconds and the contract
/// carries whole ones, so this rounds — half away from zero, as everything else
/// here does.
#[must_use]
pub fn whole_seconds(seconds: f64) -> i64 {
    whole(seconds)
}

/// The whole number a value rounds to, half away from zero, or zero when it
/// denotes none.
///
/// Read back from the value's own decimal spelling rather than cast: `as` from
/// `f64` to `i64` saturates, so a magnitude no integer holds would arrive at the
/// printer as the largest one there is rather than as the nonsense it was.
fn whole(value: f64) -> i64 {
    if !value.is_finite() {
        return 0;
    }
    format!("{:.0}", value.round()).parse().unwrap_or(0)
}
