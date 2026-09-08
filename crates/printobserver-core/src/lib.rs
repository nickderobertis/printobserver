//! `printobserver-core`.
//!
//! Owns: the supervision logic — the loop that turns printer state and vision
//! observations into supervisory decisions, and the policy that decides which
//! decisions are allowed to reach the printer.
//!
//! May depend on: `printobserver-types` and the four port crates
//! (`printobserver-printer-api`, `printobserver-vision-api`,
//! `printobserver-supervisor-api`, `printobserver-store-api`), and NO
//! implementation crate. This is the rule the whole design rests on and `just
//! check-repo` enforces it.
