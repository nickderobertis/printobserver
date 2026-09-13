//! The identifiers the supervision domain mints for its own records.
//!
//! Declared through the type crate's exported
//! [`identifier!`](printobserver_types::identifier) rule, so an action's and
//! an intervention's identifiers are minted, displayed, parsed and described
//! exactly as the three the envelope reaches are — and declared here rather
//! than centrally, because nothing outside this domain keys anything by them.

use printobserver_types::identifier;

identifier!(ActionId, "one requested action");
identifier!(InterventionId, "one bounded intervention");
