//! `printobserver-store-sqlite`.
//!
//! Owns: the `SQLite` adapter — the one implementation of
//! `printobserver-store-api`, including its schema and migrations.
//!
//! May depend on: `printobserver-types` and `printobserver-store-api`, plus its
//! `SQLite` driver. Never another implementation crate, and never
//! `printobserver-core`.
