//! `printobserver-vision-api`.
//!
//! Owns: the port a print-failure vision service speaks through: the trait for
//! submitting a frame and receiving an observation, plus that port's own error
//! type.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
