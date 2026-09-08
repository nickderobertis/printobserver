//! `printobserver-server`.
//!
//! Owns: the long-running service: the HTTP surface the clients call, the
//! supervision loop's lifecycle, and the composition root that chooses which
//! implementation backs each port.
//!
//! May depend on: `printobserver-types`, the four port crates,
//! `printobserver-core`, and the implementation crates — it is a composition
//! root, so naming an implementation is its job rather than a violation.
