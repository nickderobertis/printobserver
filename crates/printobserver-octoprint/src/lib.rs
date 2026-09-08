//! `printobserver-octoprint`.
//!
//! Owns: the `OctoPrint` adapter — the one implementation of
//! `printobserver-printer-api` that talks to an `OctoPrint` instance over its
//! HTTP API, the narrow HTTP client it talks with, the conversions between
//! `OctoPrint`'s units and the contract's, and the closed set of G-code
//! commands the fan is expressed as.
//!
//! May depend on: `printobserver-types` and `printobserver-printer-api`, plus
//! whatever it needs to reach `OctoPrint`. Never another implementation crate,
//! and never `printobserver-core`.
//!
//! # This is the only crate that knows what an `OctoPrint` is
//!
//! Everything above this crate is written as though printers were normal, so
//! every `OctoPrint` path, header, status meaning and unit lives here and a
//! check of this repository — `just check-repo`'s `octoprint-client` — refuses
//! one named in any other crate. If this crate's mapping is wrong, the belief
//! the layer above holds about the machine is wrong, which is why the mapping
//! is settled against a real `OctoPrint` by the integration tier rather than
//! against a recorded response.
//!
//! # The API key never leaves this crate legible
//!
//! [`ApiKey`] renders as [`config::REDACTED`] in `Display` and in `Debug`, and
//! it is read through [`ApiKey::expose`] in exactly one place: where the
//! `X-Api-Key` header is written. No log record this crate emits carries it, no
//! rendering of [`OctoPrintConfig`] carries it, and no variant of
//! `PrinterError` this crate constructs is built from it.
//!
//! # What this adapter cannot do, stated rather than discovered
//!
//! * **The fan has no endpoint.** `OctoPrint` offers none and reports nothing
//!   about fans, so a fan action is G-code sent through
//!   `POST /api/printer/command`, built from [`convert::COMMAND_SET`] — a fixed
//!   set of parameterized commands this crate declares — and a bounded number.
//!   A machine whose operator states it has no commandable part fan
//!   ([`FanSupport::Absent`]) answers `Unsupported` and sends nothing.
//! * **Three snapshot fields are always absent.** `GET /api/printer` reports no
//!   applied feedrate factor, no applied flowrate factor and no fan setting, so
//!   `PrinterSnapshot`'s three matching fields are absent against this source.
//!   That is what "absent" in that contract means: the source did not report it.
//! * **Filtration is not here.** It is not in the action vocabulary this port
//!   implements, and this crate implements no path to one.
//! * **Plain HTTP only.** The deployment is one board beside the printer on the
//!   machine's own network; a base URL spelled with any other scheme is refused
//!   where it is configured rather than at the first request.

pub mod client;
pub mod config;
pub mod convert;
mod http;
mod wire;

pub use client::OctoPrintPrinter;
pub use config::{
    ApiKey, ConfigError, DEFAULT_TIMEOUT, Endpoint, FanSupport, OctoPrintConfig, REDACTED, SCHEME,
};
pub use convert::{
    COMMAND_SET, FAN_PWM_PARAMETER, FAN_SET_COMMAND, fan_pwm_of_percent, fraction_of_completion,
    percent_of_multiplier,
};
