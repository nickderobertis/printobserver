//! `printobserver-obico`.
//!
//! Owns: the `Obico` adapter — the one implementation of
//! `printobserver-vision-api` that submits frames to `Obico`'s
//! failure-detection service.
//!
//! May depend on: `printobserver-types` and `printobserver-vision-api`, plus
//! whatever it needs to reach `Obico`. Never another implementation crate, and
//! never `printobserver-core`.
