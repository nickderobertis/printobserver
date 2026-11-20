//! The panels crate over recorded real-print frames: the committed
//! calibration, frames copied from the verified cases as plain fixtures
//! (`tests/fixtures/frames.json` records each one's source and capture time),
//! and the outlines and toolpaths the real `GcodeIndex` builds and spools from
//! each case's own G-code, layers chosen by the height each frame was
//! recorded at.
//!
//! A binary and an Nx project of their own, `printobserver-panels-fixtures`,
//! because decoding and composing full-size frames is the slowest thing the
//! crate's tests would otherwise do.

mod alignment;
mod composite;
mod drift;
mod placement;
mod support;
