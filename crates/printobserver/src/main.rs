//! `printobserver`.
//!
//! Owns: the `printobserver` command — the single installable artifact of this
//! repository. Its `server` subcommand runs the supervisor; the rest of its
//! subcommands are the operator's surface onto a running one.
//!
//! May depend on: `printobserver-server`, `printobserver-core`,
//! `printobserver-types`, `printobserver-sdk` and the implementation crates —
//! it is the outermost composition root.

fn main() {}
