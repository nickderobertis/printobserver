//! `printobserver-sdk`.
//!
//! Owns: the Rust client of the printobserver server's HTTP surface: a typed,
//! async client and its request/response types.
//!
//! May depend on: `printobserver-types` and an HTTP client. Never
//! `printobserver-core`, never the server, and never an implementation crate —
//! a client that could reach them would be a second copy of the server.
