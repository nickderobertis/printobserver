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
//!
//! # The policy, which is the reason this layer exists
//!
//! Obico can already see a failure and `OctoPrint` can already drive the
//! machine. What does not exist without this crate is the thing in between that
//! decides what an agent is allowed to do about it, and its whole value is in
//! being thin, bounded, and the **single** place that decision is made — so
//! that an operator's own commands are bounded by exactly the same rules an
//! agent's are.
//!
//! * **Effective bounds** are the configured [safety
//!   envelope](printobserver_types::SafetyEnvelope) intersected with the active
//!   print's manifest. A manifest may only narrow; a range wider than the
//!   envelope's is narrowed to the envelope's and the narrowing is recorded on
//!   the print. See [`bounds`].
//! * **Every action from every actor passes one decision**, taken by
//!   [`decision::decide`] and issued by the one function named in
//!   [`supervisor`]. There is no path by which an action reaches the printer
//!   without it, and that is structural: no other item of this crate names an
//!   action method of the printer port, and no item outside that one module can
//!   obtain the handle to call one on.
//! * **A request outside the bounds is rejected, not clamped**, and the
//!   rejection is persisted with the value asked for and the range allowed, so
//!   the caller can ask again inside the range.
//! * **A bounded intervention expires on its own.** Core reads the current time
//!   through an injected [`Clock`] rather than the system's, so expiry is
//!   driven by time passing and by nothing else. See [`expiry`].
//! * **Nothing in this crate, and nothing reachable from it, sends a command
//!   string to a printer.** The printer port declares no method that admits
//!   one: no method admits a byte sequence at all, and the only string anywhere
//!   in the closure its parameters reach is the one
//!   [`FileName`](printobserver_types::FileName) wraps.

pub mod actions;
pub mod block_on;
pub mod bounds;
pub mod clock;
pub mod config;
pub mod decision;
pub mod error;
pub mod events;
pub mod expiry;
pub mod supervisor;
pub mod turn_lock;

pub use actions::{ActionOutcome, reported_value};
pub use block_on::block_on;
pub use bounds::{Bounds, effective_bounds};
pub use clock::{Clock, SystemClock, plus_seconds, seconds_between, unix_seconds};
pub use config::{CoreConfig, DEFAULT_EXPIRY_POLL, DEFAULT_RECENT_EVENTS, PRINT_ID_PLACEHOLDER};
pub use decision::{DecisionInput, adjustment, decide, valid_from};
pub use error::CoreError;
pub use events::TERMINAL_STATES;
pub use expiry::{rejection_detail, restoring_action};
pub use supervisor::{Issued, Supervisor};
pub use turn_lock::{TurnGuard, TurnLocks};
