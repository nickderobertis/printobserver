//! `printobserver-octoprint`.
//!
//! Owns: the `OctoPrint` adapter — the one implementation of
//! `printobserver-printer-api` that talks to an `OctoPrint` instance over its
//! HTTP API.
//!
//! May depend on: `printobserver-types` and `printobserver-printer-api`, plus
//! whatever it needs to reach `OctoPrint`. Never another implementation crate,
//! and never `printobserver-core`.
