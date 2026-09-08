//! `printobserver-oneharness`.
//!
//! Owns: the oneharness adapter — the one implementation of
//! `printobserver-supervisor-api` that drives an agentic coding harness through
//! the `oneharness` CLI.
//!
//! May depend on: `printobserver-types` and `printobserver-supervisor-api`,
//! plus whatever it needs to drive oneharness. Never another implementation
//! crate, and never `printobserver-core`.
