//! `printobserver-printer-api`.
//!
//! Owns: the port a 3D printer speaks through: the trait for observing printer
//! and job state and for issuing the commands that change it, plus that port's
//! own error type.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
