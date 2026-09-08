//! `printobserver-types`.
//!
//! Owns: the shared domain vocabulary every other crate speaks — printer and
//! job identity, observed printer state, vision observations, supervisory
//! decisions, and their serialized forms. It holds data and total functions
//! over that data, never I/O.
//!
//! May depend on: no crate of this workspace. It is the root of the graph, so a
//! type it does not hold is a type the rest of the workspace cannot agree on.
