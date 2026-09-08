//! `printobserver-supervisor-api`.
//!
//! Owns: the port the supervising agent speaks through: the trait that turns
//! observations into a decision about the running print, plus that port's own
//! error type.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
