//! `printobserver-store-api`.
//!
//! Owns: the port durable state speaks through: the trait for recording and
//! replaying observations, decisions and job history, plus that port's own
//! error type.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
